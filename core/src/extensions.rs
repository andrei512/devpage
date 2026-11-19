//! Extension contracts and captured state; the executable installs the native loader.
mod browser;
use crate::folder::ServedFolder;
use anyhow::{Result, anyhow, bail};
use browser::Browser;
pub use browser::imported_module;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
/// An extension's package, loaded: what devpage asks, in JSON, to read its
/// input and show its files.
pub trait Package: Send + Sync {
    /// The package's reply to `request`, a JSON object whose `op` names what is
    /// asked: `create`, `prepare`, `view`, `augment`, `embed`, `targets` or
    /// `route`.
    fn invoke(&self, request: Value) -> Result<Value>;
    /// The program the package keeps running beside devpage — none, unless the
    /// package says otherwise.
    fn process(&self) -> Option<Process> {
        None
    }
}
/// A package's process: the long-running program devpage starts for it, and
/// passes the requests under the extension's `live/` address to.
#[derive(Clone, Debug, PartialEq)]
pub struct Process {
    /// The program and its arguments, as the package's `run` lists them.
    pub command: Vec<String>,
    /// The package's folder, which the program is started in.
    pub folder: PathBuf,
}
/// An address an extension's process answers — under its `live/`, or a page
/// under its `pages/` — and what answers it.
#[derive(Clone, Debug)]
pub struct Live {
    /// What names the bound extension among all of them: `<owner>/<id>`.
    pub key: String,
    /// The extension's name under the document's `extensions:`.
    pub id: String,
    /// The package's process — `None` when it has none.
    pub process: Option<Process>,
    /// What the extension is configured with, as JSON.
    pub config: String,
    /// The folder of the document file that configures the extension — the
    /// served folder, for the live session devpage binds itself.
    pub folder: PathBuf,
    /// What the process is asked for, without a leading `/`: the address
    /// past `live/`, or a page's from `pages/` on.
    pub rest: String,
}
/// What loads an extension's package from its bundle; the executable installs
/// the native one.
pub trait Loader: Send + Sync {
    /// The package bundled at `path`.
    fn load(&self, path: &Path) -> Result<Arc<dyn Package>>;
}
/// The loader the executable installed; until it does, no extension can be
/// configured.
static LOADER: OnceLock<Arc<dyn Loader>> = OnceLock::new();
/// Installs `loader` as the one every package is loaded with; a second call is
/// ignored.
pub fn install_loader(loader: Arc<dyn Loader>) {
    let _ = LOADER.set(loader);
}
/// Every configured document's extensions, by the document's file.
static INSTANCES: OnceLock<Mutex<BTreeMap<PathBuf, Vec<Binding>>>> = OnceLock::new();
/// Every configured document's extensions, by the document's file — empty at
/// first.
fn instances() -> &'static Mutex<BTreeMap<PathBuf, Vec<Binding>>> {
    INSTANCES.get_or_init(Default::default)
}
/// The system extension that is the live session: its name under a
/// document's `extensions:`, and in its address.
pub const LIVE: &str = "live";
/// The package defaults for a local serving process and its shared session.
#[derive(Clone, Default)]
struct Local {
    /// The session package when this local server enables it.
    session: Option<Arc<dyn Package>>,
    /// Extension defaults resolved relative to the global configuration.
    packages: BTreeMap<String, Config>,
    /// The entry document that configures this project’s shared session.
    entry: Option<PathBuf>,
}

/// Local configuration by served folder and mount.
static LOCAL: OnceLock<Mutex<BTreeMap<ServedFolder, Local>>> = OnceLock::new();

/// Local serving configuration, initially empty for offline commands.
fn local() -> &'static Mutex<BTreeMap<ServedFolder, Local>> {
    LOCAL.get_or_init(Default::default)
}

/// Enables globally configured packages during loopback serving. Paths are
/// relative to the global configuration file's folder.
pub fn serve_packages(scope: &ServedFolder, home: &Path, mut packages: BTreeMap<String, Config>) {
    for config in packages.values_mut() {
        if let Some(path) = &mut config.package {
            *path = home.join(&*path).to_string_lossy().into_owned();
        }
    }
    local()
        .lock()
        .unwrap()
        .entry(scope.clone())
        .or_default()
        .packages = packages;
}

/// Enables the shared live-session package for this served project.
pub fn serve_live(scope: &ServedFolder, package: Arc<dyn Package>) {
    local()
        .lock()
        .unwrap()
        .entry(scope.clone())
        .or_default()
        .session = Some(package);
}

/// The shared session reads its project configuration from the entry document.
pub fn serve_entry(scope: &ServedFolder, document: PathBuf) {
    local()
        .lock()
        .unwrap()
        .entry(scope.clone())
        .or_default()
        .entry = Some(document);
}

/// Applies a document's entries over the global defaults, before system
/// extensions and package bindings consume their configuration.
pub(crate) fn configured(
    scope: &ServedFolder,
    document: BTreeMap<String, Config>,
) -> BTreeMap<String, Config> {
    let mut configs = local()
        .lock()
        .unwrap()
        .get(scope)
        .map(|local| local.packages.clone())
        .unwrap_or_default();
    configs.extend(document);
    configs
}
/// Whether `config`, what a document configures the live session with, leaves
/// it on: it does unless it says `enabled: false`.
fn enabled(config: &serde_yaml::Value) -> Result<bool> {
    match config {
        serde_yaml::Value::Null => Ok(true),
        serde_yaml::Value::Mapping(map) => match map.get("enabled") {
            None => Ok(true),
            Some(serde_yaml::Value::Bool(enabled)) => Ok(*enabled),
            Some(_) => bail!("extensions: {LIVE}: config: enabled is not true or false"),
        },
        _ => bail!("extensions: {LIVE}: config is not a mapping"),
    }
}
/// An extension as a document's `extensions:` names it: its package and what it
/// is configured with.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// A document can turn off a package enabled globally.
    pub enabled: Option<bool>,
    /// The package's bundle; left out only for a system extension such as
    /// [`code`](crate::code) or the live session, [`LIVE`].
    pub package: Option<String>,
    /// What the package is created with, as written under `config:`; null when
    /// left out.
    #[serde(default)]
    pub config: serde_yaml::Value,
}
/// An extension bound to a document: its package, the input it reads and what
/// it last prepared from it. Clones share one state; a
/// [`snapshot`](Self::snapshot) does not.
#[derive(Clone)]
pub struct Binding(Arc<Mutex<State>>);
impl std::fmt::Debug for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExtensionBinding")
    }
}
/// What a binding holds: its package, how it was configured and what it last
/// prepared.
#[derive(Clone)]
struct State {
    /// The extension's name under the document's `extensions:`.
    id: String,
    /// The document's whole `extensions:`, as JSON, and whether the live
    /// session is bound to it — what tells a changed configuration from the
    /// bound one.
    configuration: String,
    /// The instance's id: the first 16 hex digits of the SHA-256 of the
    /// document's path, `id` and `configuration` — or, for the live session
    /// devpage binds itself, of the served folder's path and `id`: one for
    /// every document in it.
    owner: String,
    /// Whether devpage bound it itself, as the live session: no document
    /// names its package.
    system: bool,
    /// The package, loaded by the installed [`Loader`]; each request names
    /// this binding by `owner`.
    package: Arc<dyn Package>,
    /// The served folder its input and the files it shows must lie in.
    scope: ServedFolder,
    /// What the extension is configured with, as JSON.
    config: Value,
    /// The folder of the document file that configures it — the served
    /// folder, for the live session devpage binds itself.
    folder: PathBuf,
    /// The file or folder it reads, as the package's `create` reply names it —
    /// `None` when it reads none.
    input: Option<PathBuf>,
    /// The package's reply to `create`: the suffixes it owns, its input and the
    /// suffixes read there, its grammar, widgets and styles.
    descriptor: Value,
    /// Widget placements from this declaration.
    places: crate::document::Places,
    /// The modules, widgets and stylesheets the package ships for the browser,
    /// compiled.
    browser: Arc<Browser>,
    /// What the package named its last preparation — empty before the first.
    revision: String,
    /// Why the last preparation failed — `None` when it didn't, or when the
    /// reply only lacked a revision. While set, every call fails.
    error: Option<String>,
    /// The input last prepared from: each file's contents, by its name in the
    /// served folder.
    sources: BTreeMap<String, String>,
}
impl Binding {
    /// The address the binding of `s` answers under:
    /// `<mount>/__devpage/extensions/<owner>/<id>/`.
    fn base(s: &State) -> String {
        format!(
            "{}/__devpage/extensions/{}/{}/",
            s.scope.mount(),
            s.owner,
            s.id
        )
    }
    /// The extension's name under the document's `extensions:`, and the
    /// address it answers under.
    pub fn address(&self) -> (String, String) {
        let s = self.0.lock().unwrap();
        (s.id.clone(), Self::base(&s))
    }
    /// Whether this is the live session devpage binds itself, not a package
    /// the document names.
    pub fn is_session(&self) -> bool {
        self.0.lock().unwrap().system
    }
    /// The widget placements declared by this package.
    pub fn places(&self) -> crate::document::Places {
        self.0.lock().unwrap().places.clone()
    }
    /// The address of each of the package's widgets' modules, by widget name.
    pub fn widget_urls(&self) -> BTreeMap<String, String> {
        let s = self.0.lock().unwrap();
        s.browser.widgets(&Self::base(&s))
    }
    /// The addresses of the package's stylesheets.
    pub fn style_urls(&self) -> Vec<String> {
        let s = self.0.lock().unwrap();
        s.browser.styles(&Self::base(&s))
    }
    /// The package's view of `file`, a file it owns: its `layout`, `title` and
    /// `content`.
    pub fn view(&self, file: &Path) -> Result<Value> {
        let s = self.0.lock().unwrap();
        let path = s
            .scope
            .name(file)
            .ok_or_else(|| anyhow!("View outside served folder"))?;
        drop(s);
        self.call("view", json!({"path":path}))
    }

    /// Pin this render to its prepared revision while the watcher prepares the next.
    pub fn snapshot(&self) -> Self {
        Self(Arc::new(Mutex::new(self.0.lock().unwrap().clone())))
    }
    /// The package's reply to `op`, asked with `extra` and the instance, its
    /// base address and revision. An error while the last preparation failed.
    fn call(&self, op: &str, extra: Value) -> Result<Value> {
        let s = self.0.lock().unwrap();
        if let Some(error) = &s.error {
            bail!("Extension preparation failed: {error}");
        }
        let mut request = extra;
        request["op"] = op.into();
        request["instance"] = s.owner.clone().into();
        request["base"] = format!(
            "{}/__devpage/extensions/{}/{}/",
            s.scope.mount(),
            s.owner,
            s.id
        )
        .into();
        request["revision"] = s.revision.clone().into();
        s.package.invoke(request)
    }
    /// The file or folder the extension reads — `None` when it reads none.
    pub fn input(&self) -> Option<PathBuf> {
        self.0.lock().unwrap().input.clone()
    }
    /// Whether `file` is one the extension shows: its suffix is one the package
    /// declared and, when `file` is absolute, it lies in the input.
    pub fn owns(&self, file: &Path) -> bool {
        let s = self.0.lock().unwrap();
        if file.is_absolute()
            && !s
                .input
                .as_ref()
                .is_some_and(|input| file.starts_with(input))
        {
            return false;
        }
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        s.descriptor["file_extensions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(ext)))
    }
    /// Whether a new revision was prepared: the input is read again, and the
    /// package prepares from it unless it is unchanged, the last preparation
    /// succeeded and a revision is held. A failure to read or prepare is kept,
    /// and fails every call until a preparation succeeds; a reply without a
    /// revision fails only this preparation.
    pub fn prepare(&self) -> Result<bool> {
        let mut s = self.0.lock().unwrap();
        let mut sources = BTreeMap::new();
        fn read(
            scope: &ServedFolder,
            p: &Path,
            files: &mut BTreeMap<String, String>,
            extensions: &[Value],
        ) -> Result<()> {
            if std::fs::symlink_metadata(p)?.file_type().is_symlink() {
                bail!("Symlinked extension input {}", p.display());
            }
            let canonical = p.canonicalize()?;
            if !canonical.starts_with(scope.path()) {
                bail!("Extension input outside served folder");
            }
            if p.is_dir() {
                for entry in std::fs::read_dir(p)? {
                    let entry = entry?;
                    if !entry.file_name().to_string_lossy().starts_with('.') {
                        read(scope, &entry.path(), files, extensions)?;
                    }
                }
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| extensions.iter().any(|v| v.as_str() == Some(ext)))
            {
                files.insert(
                    scope
                        .name(p)
                        .ok_or_else(|| anyhow!("Input outside served folder"))?,
                    std::fs::read_to_string(p)?,
                );
            }
            Ok(())
        }
        if let Some(input) = &s.input {
            let extensions = s.descriptor["input_extensions"]
                .as_array()
                .ok_or_else(|| anyhow!("Extension must declare input_extensions"))?;
            if let Err(error) = read(&s.scope, input, &mut sources, extensions) {
                s.error = Some(error.to_string());
                return Err(error);
            }
        }
        if s.error.is_none() && s.sources == sources && !s.revision.is_empty() {
            return Ok(false);
        }
        let reply = match s
            .package
            .invoke(json!({"op":"prepare","instance":s.owner,"sources":sources}))
        {
            Ok(reply) => reply,
            Err(error) => {
                s.error = Some(format!("{error:#}"));
                return Err(error);
            }
        };
        s.revision = reply["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Extension preparation omitted revision"))?
            .into();
        s.sources = sources;
        s.error = None;
        Ok(true)
    }
    /// The syntax definition the package highlights its language with — `None`
    /// when it brings none.
    pub fn grammar(&self) -> Option<String> {
        self.0.lock().unwrap().descriptor["grammar"]
            .as_str()
            .map(String::from)
    }
    /// `source`, the page at `file`, as the package augments it — unchanged
    /// when the reply has no `source`.
    pub fn transform(&self, file: &Path, source: &str) -> Result<String> {
        let s = self.0.lock().unwrap();
        let path = s
            .scope
            .name(file)
            .ok_or_else(|| anyhow!("Page outside served folder"))?;
        drop(s);
        let reply = self.call("augment", json!({"path":path,"source":source}))?;
        Ok(reply["source"].as_str().unwrap_or(source).into())
    }
    /// The HTML the package shows `target` as in the page at `file`, with
    /// `label`; `identity` is its place among the page's `src` targets.
    pub fn embed(&self, file: &Path, target: &str, label: &str, identity: usize) -> Result<String> {
        let s = self.0.lock().unwrap();
        let path = s
            .scope
            .name(file)
            .ok_or_else(|| anyhow!("Page outside served folder"))?;
        drop(s);
        Ok(self.call(
            "embed",
            json!({"page":path,"target":target,"label":label,"identity":identity}),
        )?["html"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing embed HTML"))?
            .into())
    }
    /// The ids a link may point at inside `file`, a file the extension owns.
    pub fn ids(&self, file: &Path) -> Result<Vec<String>> {
        let s = self.0.lock().unwrap();
        let path = s
            .scope
            .name(file)
            .ok_or_else(|| anyhow!("Target outside served folder"))?;
        drop(s);
        Ok(self.call("targets", json!({"path":path}))?["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect())
    }
}
/// Binds the extensions `configs` to the document at `doc`, each package loaded
/// from its bundle under `base`, and prepares each; with them the live
/// session, where it is [on](serve_live) in `scope`, unless the document's
/// `live` says `enabled: false` in its `config` or names a package of its
/// own. Nothing to bind unbinds the document. A configuration already bound
/// in `scope` is kept as it is.
pub fn configure(
    doc: &Path,
    base: &Path,
    scope: &ServedFolder,
    mut configs: BTreeMap<String, Config>,
) -> Result<()> {
    // `live` with no package is the system extension: the live session.
    let live_enabled = configs
        .get(LIVE)
        .is_none_or(|config| config.enabled != Some(false));
    configs.retain(|_, config| config.enabled != Some(false));
    let own = match configs.get(LIVE) {
        Some(config) if config.package.is_none() => configs.remove(LIVE).map(|c| c.config),
        _ => None,
    };
    let on = own.as_ref().map(enabled).transpose()?.unwrap_or(true);
    let session = (on && live_enabled && !configs.contains_key(LIVE))
        .then(|| {
            local()
                .lock()
                .unwrap()
                .get(scope)
                .and_then(|local| local.session.clone())
        })
        .flatten();
    let mut guard = instances().lock().unwrap();
    if configs.is_empty() && session.is_none() {
        guard.remove(doc);
        return Ok(());
    }
    let configuration = serde_json::to_string(&(&configs, &own, session.is_some()))?;
    if guard.get(doc).is_some_and(|bindings| {
        bindings.iter().all(|b| {
            let s = b.0.lock().unwrap();
            s.configuration == configuration && s.scope == *scope
        })
    }) {
        return Ok(());
    }
    let mut bindings = vec![];
    let mut parts = BTreeMap::new();
    let mut claims: std::collections::BTreeSet<String> = ["md", "yml", "yaml", "design"]
        .into_iter()
        .map(String::from)
        .collect();
    for (id, config) in configs {
        let Some(path) = &config.package else {
            bail!("extensions: {id}: package is required — {id} is not a system extension");
        };
        let loader = LOADER
            .get()
            .ok_or_else(|| anyhow!("Native extension loader unavailable"))?;
        let package = loader
            .load(&base.join(path))
            .map_err(|e| anyhow!("Extension {id}: {e:#}"))?;
        let owner = format!(
            "{:x}",
            Sha256::digest(format!("{}:{id}:{configuration}", doc.display()))
        )[..16]
            .to_string();
        let binding = bind(
            Bound {
                id: id.clone(),
                owner,
                system: false,
                package,
                config: serde_json::to_value(&config.config)?,
                folder: base,
            },
            &configuration,
            scope,
            &mut claims,
        )?;
        if let Some(path) = binding.0.lock().unwrap().descriptor["world"].as_str() {
            parts.insert(id, path.to_owned());
        }
        bindings.push(binding);
    }
    if let Some(package) = session {
        let mut config = serde_json::to_value(own.unwrap_or_default())?;
        if !parts.is_empty() {
            if config["world"].is_null() {
                config["world"] = json!({});
            }
            let world = config["world"]
                .as_object_mut()
                .ok_or_else(|| anyhow!("live: world must be a mapping"))?;
            for (part, path) in parts {
                world.insert(part, path.into());
            }
        }
        let owner = format!(
            "{:x}",
            Sha256::digest(format!("{}:{LIVE}", scope.path().display()))
        )[..16]
            .to_string();
        bindings.push(bind(
            Bound {
                id: LIVE.to_string(),
                owner,
                system: true,
                package,
                config,
                folder: scope.path(),
            },
            &configuration,
            scope,
            &mut claims,
        )?);
    }
    guard.insert(doc.to_path_buf(), bindings);
    Ok(())
}
/// An extension about to be bound to a document: what [`bind`] makes a
/// [`Binding`] from.
struct Bound<'a> {
    /// The extension's name under the document's `extensions:`.
    id: String,
    /// The instance's id.
    owner: String,
    /// Whether devpage binds it itself, as the live session.
    system: bool,
    /// The package, loaded.
    package: Arc<dyn Package>,
    /// What the extension is configured with, as JSON.
    config: Value,
    /// The folder its input is named from and its program is told as the
    /// root: the document file's, or the served folder for the live session.
    folder: &'a Path,
}
/// Creates and prepares the extension `bound` in `scope`, under the document
/// configuration `configuration`; `claims` holds the file suffixes taken so
/// far, and gains the package's.
fn bind(
    bound: Bound,
    configuration: &str,
    scope: &ServedFolder,
    claims: &mut std::collections::BTreeSet<String>,
) -> Result<Binding> {
    let Bound {
        id,
        owner,
        system,
        package,
        config,
        folder,
    } = bound;
    let descriptor =
        package.invoke(json!({"op":"create","instance":owner,"id":id,"config":config}))?;
    // A package that shows no files and reads no input may leave both out.
    let none = Vec::new();
    let listed = |field: &str| -> Result<&Vec<Value>> {
        match &descriptor[field] {
            Value::Null => Ok(&none),
            Value::Array(listed) => Ok(listed),
            _ => bail!("Extension {id} {field} is not a list"),
        }
    };
    let reads = !listed("input_extensions")?.is_empty();
    for suffix in listed("file_extensions")? {
        let suffix = suffix
            .as_str()
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            .ok_or_else(|| anyhow!("Extension {id} invalid file suffix"))?;
        if !claims.insert(suffix.into()) {
            bail!("Extension {id} file format {suffix} is already claimed");
        }
    }
    if let Some(grammar) = descriptor["grammar"].as_str() {
        let syntax = syntect::parsing::SyntaxDefinition::load_from_str(grammar, true, None)
            .map_err(|e| anyhow!("Extension {id} grammar: {e}"))?;
        if syntax
            .file_extensions
            .iter()
            .any(|s| crate::highlight::builtin_language(s))
        {
            bail!("Extension {id} cannot replace a built-in language");
        }
    }
    let input = match descriptor["input"].as_str() {
        Some(input) => {
            let input = folder.join(input);
            if !input.canonicalize()?.starts_with(scope.path()) {
                bail!("Extension {id} input outside served folder");
            }
            Some(input)
        }
        None if reads || !listed("file_extensions")?.is_empty() => {
            bail!("Extension input missing")
        }
        None => None,
    };
    let browser = Browser::new(
        &descriptor,
        &format!("{}/__devpage/extensions/{owner}/{id}/", scope.mount()),
    )?;
    let places = serde_json::from_value(descriptor.get("places").cloned().unwrap_or(json!({})))?;
    let binding = Binding(Arc::new(Mutex::new(State {
        id,
        configuration: configuration.to_string(),
        owner,
        system,
        package,
        scope: scope.clone(),
        config,
        folder: folder.to_path_buf(),
        input,
        descriptor,
        places,
        browser: Arc::new(browser),
        revision: String::new(),
        error: None,
        sources: BTreeMap::new(),
    })));
    binding.prepare()?;
    Ok(binding)
}
/// The extensions bound to the document at `doc` — none when it has none.
pub fn for_document(doc: &Path) -> Vec<Binding> {
    instances()
        .lock()
        .unwrap()
        .get(doc)
        .cloned()
        .unwrap_or_default()
}
/// The input of every extension bound in `scope`, for the watcher to watch.
pub fn input_roots(scope: &ServedFolder) -> Vec<PathBuf> {
    instances()
        .lock()
        .unwrap()
        .values()
        .flatten()
        .filter(|b| b.0.lock().unwrap().scope == *scope)
        .filter_map(Binding::input)
        .collect()
}
/// Whether an extension in `scope` prepared anew after `files` changed: each
/// whose input holds one of them, or lies in one, prepares again. A failed
/// preparation counts as a change.
pub fn changed(scope: &ServedFolder, files: &[PathBuf]) -> bool {
    let all: Vec<_> = instances()
        .lock()
        .unwrap()
        .values()
        .flatten()
        .filter(|b| b.0.lock().unwrap().scope == *scope)
        .cloned()
        .collect();
    let mut changed = false;
    for b in all {
        let Some(input) = b.input() else {
            continue;
        };
        if files
            .iter()
            .any(|p| p.starts_with(&input) || input.starts_with(p))
        {
            changed |= b.prepare().unwrap_or(true);
        }
    }
    changed
}
/// The extensions of the document that owns `file` in `folder` — the entry
/// document's when none does.
pub fn for_file(file: &Path, folder: &crate::folder::Folder) -> Vec<Binding> {
    let path = folder
        .owner(file)
        .and_then(|o| o.document.ok())
        .map(|d| d.path())
        .unwrap_or_else(|| folder.entry().path());
    for_document(&path)
}
/// What answers `path` when it is under an extension's `live/` address in
/// `scope`: the bound extension and its process. `None` when it is no such
/// address.
pub fn live(path: &str, scope: &ServedFolder) -> Option<Live> {
    let (mut live, _) = bound_at(path, scope, None)?;
    live.rest = live.rest.strip_prefix("live/")?.to_string();
    Some(live)
}
/// The page of an extension `path` names in `scope`, `pages/<page>` under the
/// extension's address: what answers it, asked for `pages/<page>`; the page,
/// as its extension names it; and the file of the document it is drawn in —
/// `entry` when that document binds the extension, else the first that does.
/// `None` when `path` is no extension's page.
pub fn page(path: &str, scope: &ServedFolder, entry: &Path) -> Option<(Live, String, PathBuf)> {
    let (live, document) = bound_at(path, scope, Some(entry))?;
    let page = live.rest.strip_prefix("pages/")?.to_string();
    (!page.is_empty()).then_some((live, page, document))
}
/// The extension bound in `scope` whose address `path` is under, with
/// everything past that address as its [`Live::rest`], and the file of a
/// document that binds it: `preferred` when it does, else the first.
fn bound_at(path: &str, scope: &ServedFolder, preferred: Option<&Path>) -> Option<(Live, PathBuf)> {
    let suffix = path
        .trim_start_matches('/')
        .strip_prefix("__devpage/extensions/")?;
    let (owner, tail) = suffix.split_once('/')?;
    let (id, rest) = tail.split_once('/')?;
    let guard = instances().lock().unwrap();
    let found = |(document, bindings): (&PathBuf, &Vec<Binding>)| {
        bindings.iter().find_map(|b| {
            let s = b.0.lock().unwrap();
            (s.owner == owner && s.id == id && s.scope == *scope).then(|| {
                let live = Live {
                    key: format!("{owner}/{id}"),
                    id: s.id.clone(),
                    process: s.package.process(),
                    config: s.config.to_string(),
                    folder: s.folder.clone(),
                    rest: rest.to_string(),
                };
                (live, document.clone())
            })
        })
    };
    let entry = (id == LIVE)
        .then(|| {
            local()
                .lock()
                .unwrap()
                .get(scope)
                .and_then(|local| local.entry.clone())
        })
        .flatten();
    preferred
        .or(entry.as_deref())
        .and_then(|preferred| guard.get_key_value(preferred))
        .and_then(found)
        .or_else(|| guard.iter().find_map(found))
}
/// Whether `key`, a [`Live::key`], still names a bound extension: one whose
/// document was not loaded again with another configuration.
pub fn bound(key: &str) -> bool {
    let guard = instances().lock().unwrap();
    guard.values().flatten().any(|b| {
        let s = b.0.lock().unwrap();
        key.split_once('/') == Some((&s.owner, &s.id))
    })
}
/// The reply to `path` when it is an extension's address in `scope`: a browser
/// file the package ships, or else what the package routes it to. `None` when
/// it is no extension's.
pub fn route(path: &str, scope: &ServedFolder) -> Option<Result<Value>> {
    let suffix = path
        .trim_start_matches('/')
        .strip_prefix("__devpage/extensions/")?;
    let (owner, tail) = suffix.split_once('/')?;
    let (id, tail) = tail.split_once('/')?;
    let all: Vec<_> = instances()
        .lock()
        .unwrap()
        .values()
        .flatten()
        .cloned()
        .collect();
    all.into_iter()
        .find(|b| {
            let s = b.0.lock().unwrap();
            s.owner == owner && s.id == id && s.scope == *scope
        })
        .map(|b| {
            let response = {
                let s = b.0.lock().unwrap();
                s.browser.response(tail)
            };
            response.map_or_else(|| b.call("route", json!({"path":tail})), Ok)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codespec_rust_macro::spec;
    struct Toy;
    impl Loader for Toy {
        fn load(&self, _: &Path) -> Result<Arc<dyn Package>> {
            Ok(Arc::new(Toy))
        }
    }
    impl Package for Toy {
        fn invoke(&self, r: Value) -> Result<Value> {
            Ok(match r["op"].as_str().unwrap() {
                "create" => {
                    json!({"input":r["config"]["input"],"input_extensions":["toy","md"],"file_extensions":["toy"],
                    "grammar":"%YAML 1.2\n---\nname: Toy\nfile_extensions: [toy]\nscope: source.toy\ncontexts:\n  main:\n    - match: magic\n      scope: keyword.control.toy\n"})
                }
                "prepare" => {
                    json!({"revision":format!("{:x}",Sha256::digest(r["sources"].to_string()))})
                }
                "augment" => {
                    json!({"source":format!("{}\n\n## Extension content\n",r["source"].as_str().unwrap())})
                }
                "embed" => {
                    json!({"html":"<svg role=\"img\"><text>Prepared toy diagram</text></svg>"})
                }
                "view" => {
                    json!({"layout":"workspace","title":"Toy","content":format!("<h1>Toy viewer</h1><p>{}</p>",r["revision"].as_str().unwrap())})
                }
                "targets" => json!({"ids":["main"]}),
                "route" => json!({"body":r["revision"],"mime":"text/plain","status":200}),
                _ => bail!("Unknown toy operation"),
            })
        }
    }

    #[test]
    #[spec("hot-reload-works-for-every-page")]
    #[spec("a-broken-file-shows-its-error")]
    fn another_language_uses_the_same_capture_render_targets_and_watch_contract() {
        install_loader(Arc::new(Toy));
        let dir =
            std::env::temp_dir().join(format!("devpage-extension-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("model/nested")).unwrap();
        std::fs::write(dir.join("model/nested/a.toy"), "first").unwrap();
        std::fs::write(dir.join("model/ignored.rs"), "not an input").unwrap();
        std::fs::write(
            dir.join("page.md"),
            "# Page\n\n```toy\nmagic\n```\n\n![Toy](model/nested/a.toy#main)",
        )
        .unwrap();
        std::fs::write(dir.join("devpage.yml"),"kind: site\ntitle: Toy\nroot: page.md\nwatch: [page.md]\nextensions:\n  toy:\n    package: ./toy-package\n    config:\n      input: model\n").unwrap();
        let dir = dir.canonicalize().unwrap();
        let scope = ServedFolder::new(&dir).unwrap();
        let doc = crate::document::Document::load(&dir.join("devpage.yml"), &scope).unwrap();
        let bindings = for_document(&doc.path());
        assert_eq!(bindings.len(), 1);
        assert!(!bindings[0].prepare().unwrap());
        let first = bindings[0].view(&dir.join("model/nested/a.toy")).unwrap();
        let rendered = crate::page::content(
            &crate::widgets::Widgets::of(&doc, &crate::widgets::Modules::default()),
            &dir.join("page.md"),
            std::fs::read_to_string(dir.join("page.md")).unwrap(),
        );
        assert!(rendered.html().contains("hl-keyword"));
        assert!(rendered.html().contains("Prepared toy diagram"));
        assert!(rendered.ids().contains("extension-content"));
        assert!(
            rendered
                .embeds()
                .iter()
                .any(|t| t == "model/nested/a.toy#main")
        );
        assert_eq!(
            bindings[0].ids(&dir.join("model/nested/a.toy")).unwrap(),
            vec!["main"]
        );
        let folder = crate::folder::Folder::new(Arc::new(doc));
        assert!(folder.matters(&dir.join("model/nested/new.toy"), false));
        std::fs::write(dir.join("model/nested/a.toy"), "second").unwrap();
        assert!(changed(&scope, &[dir.join("model/nested/a.toy")]));
        assert_ne!(
            first,
            bindings[0].view(&dir.join("model/nested/a.toy")).unwrap()
        );
        // Failure is visible; stale model data is not presented as current.
        std::fs::remove_dir_all(dir.join("model")).unwrap();
        assert!(bindings[0].prepare().is_err());
        assert!(bindings[0].view(&dir.join("model/nested/a.toy")).is_err());
        std::fs::create_dir_all(dir.join("model")).unwrap();
        assert!(bindings[0].prepare().unwrap());
        assert!(bindings[0].view(&dir.join("model/a.toy")).is_ok());
        instances().lock().unwrap().remove(&dir.join("devpage.yml"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A package that only runs a program, as the live session's does.
    struct Session;
    impl Package for Session {
        fn invoke(&self, r: Value) -> Result<Value> {
            Ok(match r["op"].as_str().unwrap() {
                "create" => {
                    json!({"places":{"navbar":["DevpageLiveTasksButton"],"panel":["DevpageLiveTasks"],"float":["DevpageLiveCanvas","DevpageLiveOrb"]}})
                }
                "prepare" => json!({"revision": "none"}),
                _ => json!({}),
            })
        }
        fn process(&self) -> Option<Process> {
            Some(Process {
                command: vec!["session".into()],
                folder: PathBuf::from("/program"),
            })
        }
    }

    #[test]
    fn the_live_session_is_one_for_a_served_folder_and_a_document_turns_it_off_or_replaces_it() {
        use crate::document::Document;
        install_loader(Arc::new(Toy));
        let dir = std::env::temp_dir().join(format!("devpage-live-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["widgets", "set", "off", "own/model"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        std::fs::write(dir.join("widgets/Mine.tsx"), "export default () => null;").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nwidgets: widgets\nplaces:\n  float: [Mine]\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# T\n").unwrap();
        std::fs::write(dir.join("lone.md"), "# Lone\n").unwrap();
        std::fs::write(
            dir.join("set/devpage.yml"),
            "kind: doc\ntitle: S\nchapters: [a.md]\nextensions:\n  live:\n    config:\n      voice: calm\n",
        )
        .unwrap();
        std::fs::write(dir.join("set/a.md"), "# A\n").unwrap();
        std::fs::write(
            dir.join("off/devpage.yml"),
            "kind: doc\ntitle: O\nchapters: [a.md]\nextensions:\n  live:\n    config:\n      enabled: false\n",
        )
        .unwrap();
        std::fs::write(dir.join("off/a.md"), "# A\n").unwrap();
        std::fs::write(
            dir.join("own/devpage.yml"),
            "kind: doc\ntitle: W\nchapters: [a.md]\nextensions:\n  live:\n    package: ./package\n    config:\n      input: model\n",
        )
        .unwrap();
        std::fs::write(dir.join("own/a.md"), "# A\n").unwrap();
        let dir = dir.canonicalize().unwrap();
        let scope = ServedFolder::new(&dir).unwrap();
        let load = |path: &str| Document::load(&dir.join(path), &scope).unwrap();

        // Off: no document has it, whatever it says of it.
        for path in [
            "devpage.yml",
            "lone.md",
            "set/devpage.yml",
            "off/devpage.yml",
        ] {
            let document = load(path);
            assert!(document.extensions.is_empty(), "{path}");
            assert!(document.places.panel.is_empty(), "{path}");
        }

        serve_live(&scope, Arc::new(Session));
        let site = load("devpage.yml");
        let (id, address) = site.extensions[0].address();
        assert_eq!(id, LIVE);
        assert!(site.extensions[0].is_session());
        // Its widgets come after the document's own.
        assert_eq!(site.places.navbar, ["DevpageLiveTasksButton"]);
        assert_eq!(site.places.panel, ["DevpageLiveTasks"]);
        assert_eq!(
            site.places.float,
            ["Mine", "DevpageLiveCanvas", "DevpageLiveOrb"]
        );
        // One address for every document: a lone Markdown file's, and one
        // that configures the session.
        for path in ["lone.md", "set/devpage.yml"] {
            let document = load(path);
            assert_eq!(document.extensions.len(), 1, "{path}");
            assert_eq!(document.extensions[0].address().1, address, "{path}");
            assert_eq!(document.places.panel, ["DevpageLiveTasks"], "{path}");
        }
        // Its program is told the served folder, and each document's own
        // configuration is kept with its binding.
        let asked = live(&format!("{address}live/socket"), &scope).unwrap();
        assert_eq!(asked.folder, dir);
        assert_eq!(asked.rest, "socket");
        assert!(asked.process.is_some());
        assert!(bound(&asked.key));
        let configured = for_document(&dir.join("set/devpage.yml"));
        assert_eq!(
            configured[0].0.lock().unwrap().config,
            json!({"voice": "calm"})
        );

        // `enabled: false` turns it off for the document, and a package of
        // the document's own takes its place: devpage places nothing.
        let off = load("off/devpage.yml");
        assert!(off.extensions.is_empty());
        assert_eq!(off.places, Default::default());
        let own = load("own/devpage.yml");
        assert_eq!(own.extensions.len(), 1);
        assert!(!own.extensions[0].is_session());
        assert_ne!(own.extensions[0].address().1, address);
        assert_eq!(own.places, Default::default());

        std::fs::write(
            dir.join("off/devpage.yml"),
            "kind: doc\ntitle: O\nchapters: [a.md]\nextensions:\n  live:\n    config:\n      enabled: sometimes\n",
        )
        .unwrap();
        let error = Document::load(&dir.join("off/devpage.yml"), &scope).unwrap_err();
        assert!(
            format!("{error:#}")
                .ends_with("extensions: live: config: enabled is not true or false"),
            "{error:#}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
