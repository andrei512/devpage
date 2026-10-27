//! The macOS watcher: an FSEvents stream per deep folder and kqueue for the
//! shallow folders and the files directly in them, so fseventsd is never
//! asked about anything out of sight. Both report through the same handler,
//! as `notify` events.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, OsString, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Result, bail};
use dispatch2::{DispatchQueue, DispatchRetained};
use notify::event::{
    CreateKind, DataChange, EventKind, MetadataKind, ModifyKind, RemoveKind, RenameMode,
};
use objc2_core_foundation::{CFArray, CFString};
use objc2_core_services as fs;

use super::{BURST, Handler, WatchSet};

/// The platform's watcher over a watch set. Stops when dropped.
pub struct Platform {
    handler: Handler,
    set: WatchSet,
    /// The serial queue every stream calls back on.
    queue: DispatchRetained<DispatchQueue>,
    streams: BTreeMap<PathBuf, Stream>,
    kqueue: Kqueue,
}

impl Platform {
    pub fn new(_served: &Path, handler: Handler) -> Result<Self> {
        raise_open_files_limit();
        Ok(Self {
            kqueue: Kqueue::new(handler.clone())?,
            handler,
            set: WatchSet::default(),
            queue: DispatchQueue::new("devpage watch", None),
            streams: BTreeMap::new(),
        })
    }

    /// Watches `set` in place of the last one: only folders that changed are
    /// added or removed. Returns the folders newly reported on — each with
    /// whether everything under it is — that no watch covered before.
    pub fn apply(&mut self, set: WatchSet) -> Vec<(PathBuf, bool)> {
        // New watches start before old ones stop, so nothing in a folder
        // passing from one to the other goes unreported.
        let reported = |folder: &Path| self.set.deep.iter().any(|deep| folder.starts_with(deep));
        let mut fresh = Vec::new();
        for folder in &set.deep {
            if !self.streams.contains_key(folder)
                && let Some(stream) = Stream::start(folder, &self.handler, &self.queue)
            {
                self.streams.insert(folder.clone(), stream);
                if !reported(folder) {
                    fresh.push((folder.clone(), true));
                }
            }
        }
        // Every shallow folder, not only new ones: one removed and made again
        // lost its watch with the folder it was on.
        for folder in &set.shallow {
            if self.kqueue.watch(folder) && !reported(folder) {
                fresh.push((folder.clone(), false));
            }
        }
        self.streams.retain(|folder, _| set.deep.contains(folder));
        for folder in self.set.shallow.difference(&set.shallow) {
            self.kqueue.unwatch(folder);
        }
        self.set = set;
        fresh
    }

    /// Whether changes in `folder` are already reported: by a stream, which
    /// follows its path, or by a kqueue watch still open — a shallow folder
    /// deleted lost its watch, even if a folder has its name again.
    pub fn covers(&self, folder: &Path) -> bool {
        self.kqueue.watches(folder) || self.set.deep.iter().any(|deep| folder.starts_with(deep))
    }

    /// Whether a watched folder is `path` or inside it.
    pub fn watches_within(&self, path: &Path) -> bool {
        let mut folders = self.set.deep.iter().chain(&self.set.shallow);
        folders.any(|folder| folder.starts_with(path))
    }
}

/// kqueue holds a descriptor per watched file and folder, and a shell's
/// default limit is 256: raise it to what the system allows.
fn raise_open_files_limit() {
    // SAFETY: `getrlimit` and `setrlimit` only read and write `limit`.
    unsafe {
        let mut limit = std::mem::zeroed::<libc::rlimit>();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
            // macOS refuses a soft limit above OPEN_MAX, 10240.
            limit.rlim_cur = limit.rlim_max.min(10240);
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
        }
    }
}

/// One FSEvents stream over a deep folder: file events, coalesced by
/// fseventsd for [`BURST`], handed to the handler on `queue`.
struct Stream {
    stream: fs::FSEventStreamRef,
    queue: DispatchRetained<DispatchQueue>,
}

// SAFETY: a stream is stopped and released on its own queue, from any thread.
unsafe impl Send for Stream {}

impl Stream {
    fn start(
        folder: &Path,
        handler: &Handler,
        queue: &DispatchRetained<DispatchQueue>,
    ) -> Option<Self> {
        let paths = CFArray::from_retained_objects(&[CFString::from_str(folder.to_str()?)]);
        // The stream owns a handle on the handler, freed when it is released.
        let info = Box::into_raw(Box::new(handler.clone()));
        let mut context = fs::FSEventStreamContext {
            version: 0,
            info: info.cast(),
            retain: None,
            release: Some(release),
            copyDescription: None,
        };
        // SAFETY: `callback` matches the stream's callback type, and the
        // context, which FSEvents copies, carries what `release` frees.
        let stream = unsafe {
            fs::FSEventStreamCreate(
                None,
                Some(callback),
                &mut context,
                paths.as_opaque(),
                fs::kFSEventStreamEventIdSinceNow,
                BURST.as_secs_f64(),
                fs::kFSEventStreamCreateFlagFileEvents,
            )
        };
        if stream.is_null() {
            // SAFETY: the stream was not made, so nothing else owns `info`.
            drop(unsafe { Box::from_raw(info) });
            return None;
        }
        // SAFETY: `stream` is a valid stream, released here if it won't start.
        unsafe {
            fs::FSEventStreamSetDispatchQueue(stream, Some(queue));
            if !fs::FSEventStreamStart(stream) {
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
                return None;
            }
        }
        Some(Self {
            stream,
            queue: queue.clone(),
        })
    }
}

impl Drop for Stream {
    /// Stops the stream on its queue, so no callback is running when its
    /// handle on the handler is freed. Never dropped on the queue itself.
    fn drop(&mut self) {
        struct Send(fs::FSEventStreamRef);
        // SAFETY: the stream is only used on the queue from here on.
        unsafe impl std::marker::Send for Send {}
        let stream = Send(self.stream);
        self.queue.exec_sync(move || {
            // The whole wrapper moves in, not just its pointer.
            let Send(stream) = { stream };
            // SAFETY: `stream` is a valid, started stream, released once, here.
            unsafe {
                fs::FSEventStreamStop(stream);
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
            }
        });
    }
}

/// Frees a stream's handle on the handler, when the stream is released.
unsafe extern "C-unwind" fn release(info: *const c_void) {
    // SAFETY: `info` is the box `Stream::start` made, freed only here.
    drop(unsafe { Box::from_raw(info as *mut Handler) });
}

/// Hands each event of a stream to the handler.
unsafe extern "C-unwind" fn callback(
    _stream: fs::ConstFSEventStreamRef,
    info: *mut c_void,
    count: usize,
    paths: NonNull<c_void>,
    flags: NonNull<fs::FSEventStreamEventFlags>,
    _ids: NonNull<fs::FSEventStreamEventId>,
) {
    // SAFETY: `info` is the stream's handler; without `UseCFTypes` the paths
    // are C strings, `count` of them, as are the flags.
    let (handler, paths, flags) = unsafe {
        (
            &*(info as *const Handler),
            std::slice::from_raw_parts(paths.as_ptr() as *const *const libc::c_char, count),
            std::slice::from_raw_parts(flags.as_ptr(), count),
        )
    };
    for (&path, &flags) in paths.iter().zip(flags) {
        // SAFETY: each path is a NUL-terminated string FSEvents owns.
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(
            unsafe { CStr::from_ptr(path) }.to_bytes(),
        ));
        for kind in kinds(flags) {
            handler(Ok(notify::Event::new(kind).add_path(path.clone())));
        }
    }
}

/// What a stream's flags for one path say happened, oldest first. A rescan
/// request, with no path that matters, says nothing.
fn kinds(flags: u32) -> Vec<EventKind> {
    let is = |flag: u32| flags & flag != 0;
    let folder = is(fs::kFSEventStreamEventFlagItemIsDir);
    let mut kinds = Vec::new();
    if is(fs::kFSEventStreamEventFlagItemCreated) {
        kinds.push(EventKind::Create(if folder {
            CreateKind::Folder
        } else {
            CreateKind::File
        }));
    }
    if is(fs::kFSEventStreamEventFlagItemRemoved) {
        kinds.push(EventKind::Remove(if folder {
            RemoveKind::Folder
        } else {
            RemoveKind::File
        }));
    }
    if is(fs::kFSEventStreamEventFlagItemRenamed) {
        kinds.push(EventKind::Modify(ModifyKind::Name(RenameMode::Any)));
    }
    if is(fs::kFSEventStreamEventFlagItemInodeMetaMod
        | fs::kFSEventStreamEventFlagItemChangeOwner
        | fs::kFSEventStreamEventFlagItemXattrMod
        | fs::kFSEventStreamEventFlagItemFinderInfoMod)
    {
        kinds.push(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)));
    }
    if is(fs::kFSEventStreamEventFlagItemModified) {
        kinds.push(EventKind::Modify(ModifyKind::Data(DataChange::Content)));
    }
    kinds
}

/// kqueue over the shallow folders: each folder for its entries, and each
/// file directly in it for writes. A thread waits for events and hands them
/// to the handler; it ends when the kqueue is dropped.
struct Kqueue {
    queue: Arc<OwnedFd>,
    watches: Arc<Mutex<Watches>>,
    thread: Option<JoinHandle<()>>,
}

/// The ident of the user event that wakes the thread to stop.
const STOP: usize = 0;

impl Kqueue {
    fn new(handler: Handler) -> Result<Self> {
        // SAFETY: `kqueue` returns a new descriptor or -1.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            bail!("kqueue: {}", std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new descriptor nothing else owns.
        let queue = Arc::new(unsafe { OwnedFd::from_raw_fd(fd) });
        register(
            &queue,
            STOP,
            libc::EVFILT_USER,
            libc::EV_ADD | libc::EV_CLEAR,
            0,
            0,
        );
        let watches = Arc::new(Mutex::new(Watches {
            queue: queue.clone(),
            next: STOP + 1,
            by_id: HashMap::new(),
            folders: BTreeMap::new(),
        }));
        let thread = {
            let (queue, watches) = (queue.clone(), watches.clone());
            std::thread::Builder::new()
                .name("devpage kqueue".into())
                .spawn(move || wait(&queue, &watches, &handler))?
        };
        Ok(Self {
            queue,
            watches,
            thread: Some(thread),
        })
    }

    /// Starts watching a shallow folder unless it is watched: true when it
    /// wasn't and now is.
    fn watch(&self, folder: &Path) -> bool {
        lock(&self.watches).folder(folder)
    }

    /// Whether a shallow folder is watched now.
    fn watches(&self, folder: &Path) -> bool {
        lock(&self.watches).folders.contains_key(folder)
    }

    fn unwatch(&self, folder: &Path) {
        lock(&self.watches).forget(folder);
    }
}

impl Drop for Kqueue {
    fn drop(&mut self) {
        register(
            &self.queue,
            STOP,
            libc::EVFILT_USER,
            0,
            libc::NOTE_TRIGGER,
            0,
        );
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Waits for kqueue events until told to stop, handing the handler what
/// each says happened.
fn wait(queue: &OwnedFd, watches: &Mutex<Watches>, handler: &Handler) {
    // SAFETY: an all-zero kevent is a valid value to overwrite.
    let mut events = [unsafe { std::mem::zeroed::<libc::kevent>() }; 64];
    loop {
        // SAFETY: `events` has room for the count passed.
        let count = unsafe {
            libc::kevent(
                queue.as_raw_fd(),
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as i32,
                std::ptr::null(),
            )
        };
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let mut happened = Vec::new();
        {
            let mut watches = lock(watches);
            for event in &events[..count as usize] {
                if event.filter == libc::EVFILT_USER {
                    return;
                }
                watches.happened(event.udata as usize, event.fflags, &mut happened);
            }
        }
        for event in happened {
            handler(Ok(event));
        }
    }
}

/// Every open kqueue watch, by the id its events carry. Ids are never
/// reused, so an event for a watch closed in the meantime is recognised as
/// stale even when its descriptor number is taken again.
struct Watches {
    queue: Arc<OwnedFd>,
    next: usize,
    by_id: HashMap<usize, Watched>,
    /// Each shallow folder's watch id.
    folders: BTreeMap<PathBuf, usize>,
}

struct Watched {
    /// Closing it removes the watch from the kqueue.
    _fd: OwnedFd,
    path: PathBuf,
    what: What,
}

enum What {
    /// A shallow folder, with what it held when last read.
    Folder(HashMap<OsString, Held>),
    /// A file directly in one, with its folder's id.
    File(usize),
}

/// An entry of a shallow folder: its inode, whether it is a folder, and the
/// id of its watch while a file is watched.
struct Held {
    inode: u64,
    folder: bool,
    watch: Option<usize>,
}

const FOLDER_NOTES: u32 = libc::NOTE_WRITE | libc::NOTE_DELETE | libc::NOTE_RENAME;
const FILE_NOTES: u32 = libc::NOTE_WRITE
    | libc::NOTE_EXTEND
    | libc::NOTE_ATTRIB
    | libc::NOTE_DELETE
    | libc::NOTE_RENAME;

impl Watches {
    /// Opens `path` for events and adds a watch on it; its id, unless it
    /// can't be opened.
    fn open(&mut self, path: &Path, notes: u32, what: What) -> Option<usize> {
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // A symbolic link is watched itself, never what it points to, which
        // may lie outside the served folder.
        let flags = libc::O_EVTONLY | libc::O_CLOEXEC | libc::O_SYMLINK;
        // SAFETY: `name` is a NUL-terminated path; `open` returns a new
        // descriptor or -1.
        let fd = unsafe { libc::open(name.as_ptr(), flags) };
        if fd < 0 {
            return None;
        }
        // SAFETY: `fd` is a new descriptor nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let id = self.next;
        self.next += 1;
        register(
            &self.queue,
            fd.as_raw_fd() as usize,
            libc::EVFILT_VNODE,
            libc::EV_ADD | libc::EV_CLEAR,
            notes,
            id,
        );
        self.by_id.insert(
            id,
            Watched {
                _fd: fd,
                path: path.to_path_buf(),
                what,
            },
        );
        Some(id)
    }

    /// Watches a shallow folder and the files directly in it, unless it is
    /// watched: true when it wasn't and now is.
    fn folder(&mut self, folder: &Path) -> bool {
        if self.folders.contains_key(folder) {
            return false;
        }
        // The folder first, so an entry added while it is read is still heard of.
        let Some(id) = self.open(folder, FOLDER_NOTES, What::Folder(HashMap::new())) else {
            return false;
        };
        self.folders.insert(folder.to_path_buf(), id);
        self.reread(id, &mut Vec::new(), false);
        true
    }

    /// Stops watching a shallow folder and the files directly in it.
    fn forget(&mut self, folder: &Path) {
        if let Some(id) = self.folders.remove(folder)
            && let Some(Watched {
                what: What::Folder(held),
                ..
            }) = self.by_id.remove(&id)
        {
            for watch in held.values().filter_map(|held| held.watch) {
                self.by_id.remove(&watch);
            }
        }
    }

    /// Reads folder `id` again, watching its new and replaced files, and —
    /// when `report` — says what was added, removed or replaced.
    fn reread(&mut self, id: usize, happened: &mut Vec<notify::Event>, report: bool) {
        let Some(Watched {
            path: folder,
            what: What::Folder(before),
            ..
        }) = self.by_id.get_mut(&id)
        else {
            return;
        };
        let folder = folder.clone();
        let mut before = std::mem::take(before);
        let mut now = HashMap::new();
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let name = entry.file_name();
            let path = folder.join(&name);
            let is_folder = metadata.is_dir();
            let mut watch = None;
            match before.remove(&name) {
                Some(held) if held.inode == metadata.ino() && held.watch.is_some() => {
                    watch = held.watch;
                }
                Some(held) => {
                    if let Some(old) = held.watch {
                        self.by_id.remove(&old);
                    }
                    if held.inode != metadata.ino() && report {
                        happened.push(created(&path, is_folder));
                    }
                }
                None if report => happened.push(created(&path, is_folder)),
                None => {}
            }
            if !is_folder && watch.is_none() {
                watch = self.open(&path, FILE_NOTES, What::File(id));
            }
            now.insert(
                name,
                Held {
                    inode: metadata.ino(),
                    folder: is_folder,
                    watch,
                },
            );
        }
        for (name, held) in before {
            if let Some(old) = held.watch {
                self.by_id.remove(&old);
            }
            if report {
                let kind = if held.folder {
                    RemoveKind::Folder
                } else {
                    RemoveKind::File
                };
                happened
                    .push(notify::Event::new(EventKind::Remove(kind)).add_path(folder.join(name)));
            }
        }
        if let Some(Watched {
            what: What::Folder(held),
            ..
        }) = self.by_id.get_mut(&id)
        {
            *held = now;
        }
    }

    /// Records what the notes of watch `id` say happened.
    fn happened(&mut self, id: usize, notes: u32, happened: &mut Vec<notify::Event>) {
        let Some(watched) = self.by_id.get(&id) else {
            return; // closed since
        };
        let path = watched.path.clone();
        let gone = notes & (libc::NOTE_DELETE | libc::NOTE_RENAME) != 0;
        match watched.what {
            What::Folder(_) if gone => {
                happened.push(
                    notify::Event::new(EventKind::Remove(RemoveKind::Folder))
                        .add_path(path.clone()),
                );
                self.forget(&path);
            }
            What::Folder(_) => self.reread(id, happened, true),
            What::File(folder) => {
                let event = |kind| notify::Event::new(kind).add_path(path.clone());
                if notes & (libc::NOTE_WRITE | libc::NOTE_EXTEND) != 0 {
                    happened.push(event(EventKind::Modify(ModifyKind::Data(
                        DataChange::Content,
                    ))));
                }
                if notes & libc::NOTE_ATTRIB != 0 {
                    happened.push(event(EventKind::Modify(ModifyKind::Metadata(
                        MetadataKind::Any,
                    ))));
                }
                if notes & libc::NOTE_DELETE != 0 {
                    happened.push(event(EventKind::Remove(RemoveKind::File)));
                } else if notes & libc::NOTE_RENAME != 0 {
                    happened.push(event(EventKind::Modify(ModifyKind::Name(RenameMode::Any))));
                }
                if gone {
                    // Its folder re-watches whatever has the name next.
                    self.by_id.remove(&id);
                    if let Some(Watched {
                        what: What::Folder(held),
                        ..
                    }) = self.by_id.get_mut(&folder)
                        && let Some(held) = path.file_name().and_then(|name| held.get_mut(name))
                    {
                        held.watch = None;
                    }
                }
            }
        }
    }
}

fn created(path: &Path, folder: bool) -> notify::Event {
    let kind = if folder {
        CreateKind::Folder
    } else {
        CreateKind::File
    };
    notify::Event::new(EventKind::Create(kind)).add_path(path.to_path_buf())
}

/// Adds, changes or triggers one kqueue event.
fn register(queue: &OwnedFd, ident: usize, filter: i16, flags: u16, fflags: u32, id: usize) {
    let change = libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: id as *mut c_void,
    };
    // SAFETY: one change, no events asked for.
    unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &change,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        );
    }
}

fn lock(watches: &Mutex<Watches>) -> std::sync::MutexGuard<'_, Watches> {
    watches
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
