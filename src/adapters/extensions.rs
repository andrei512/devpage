//! Trusted native extension packages loaded by the executable, never by core.
use anyhow::{Context, Result, bail};
use devpage_core::extensions::{Loader, Package, Process};
use devpage_extension_api::{API, VTable};
use libloading::Library;
use serde::Deserialize;
use serde_json::Value;
use std::{path::Path, sync::Arc};

/// A package's `extension.yml`: what it is, where its library lies and the
/// program it runs. It has a library, a program or both.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    /// The package's id, of ASCII letters, digits and `-`; a `create` request
    /// under another configuration key is refused.
    id: String,
    /// The package's version; must not be empty.
    version: String,
    /// The extension ABI the package was built for; must be [`API`].
    api: u32,
    /// The platform triple the library was built for; must be the executable's
    /// own. Needed only with `library`.
    target: Option<String>,
    /// The dynamic library's path inside the package.
    library: Option<String>,
    /// The program the package keeps running beside devpage, and its
    /// arguments; started in the package's folder.
    run: Option<Vec<String>>,
    /// The package's asset folder inside it, if it has one.
    assets: Option<String>,
}
/// A loaded native package: its library and the two entry points taken from
/// its table.
struct Native {
    /// The open library, kept so `invoke` and `release` stay valid.
    _library: Library,
    /// The package's canonical folder; a grammar it names must lie inside.
    root: std::path::PathBuf,
    /// The package's id from its manifest.
    id: String,
    /// The library's request entry point.
    invoke: unsafe extern "C" fn(*const u8, usize) -> devpage_extension_api::Buffer,
    /// Frees a buffer `invoke` returned.
    release: unsafe extern "C" fn(devpage_extension_api::Buffer),
    /// The program the package runs, if its manifest names one.
    process: Option<Process>,
}
/// A package with no library: it shows no files and adds nothing to a page,
/// and only runs its program.
struct Plain {
    /// The package's id from its manifest.
    id: String,
    /// The program the package runs.
    process: Process,
}
impl Package for Plain {
    fn invoke(&self, request: Value) -> Result<Value> {
        match request["op"].as_str() {
            Some("create") if request["id"].as_str() != Some(&self.id) => {
                bail!("Package id must match configuration key {}", self.id)
            }
            Some("create") => Ok(serde_json::json!({})),
            Some("prepare") => Ok(serde_json::json!({"revision": "none"})),
            // A page's source passes through unchanged.
            Some("augment") => Ok(serde_json::json!({})),
            Some("route") => Ok(serde_json::json!({
                "body": "Not found", "mime": "text/plain", "status": 404
            })),
            _ => bail!("Package {} has no library", self.id),
        }
    }
    fn process(&self) -> Option<Process> {
        Some(self.process.clone())
    }
}
impl Package for Native {
    fn invoke(&self, request: Value) -> Result<Value> {
        if request["op"] == "create" && request["id"].as_str() != Some(&self.id) {
            bail!("Package id must match configuration key {}", self.id);
        }
        let bytes = serde_json::to_vec(&request)?;
        let buffer = unsafe { (self.invoke)(bytes.as_ptr(), bytes.len()) };
        let reply = if buffer.ptr.is_null() {
            Err(anyhow::anyhow!("Extension returned a null buffer"))
        } else {
            serde_json::from_slice::<Value>(unsafe {
                std::slice::from_raw_parts(buffer.ptr, buffer.len)
            })
            .map_err(Into::into)
        };
        unsafe { (self.release)(buffer) };
        let reply = reply?;
        if reply["ok"] != true {
            bail!("{}", reply["error"].as_str().unwrap_or("Extension failed"));
        }
        let mut value = reply["value"].clone();
        if request["op"] == "create"
            && let Some(grammar) = value["grammar"].as_str()
        {
            let path = self.root.join(grammar).canonicalize()?;
            if !path.starts_with(&self.root) {
                bail!("Grammar outside package");
            }
            value["grammar"] = std::fs::read_to_string(path)?.into();
        }
        Ok(value)
    }
    fn process(&self) -> Option<Process> {
        self.process.clone()
    }
}
/// Loads a package folder as a native extension, after checking its manifest
/// and ABI.
struct NativeLoader;
impl Loader for NativeLoader {
    fn load(&self, path: &Path) -> Result<Arc<dyn Package>> {
        Ok(package(path)?.1)
    }
}
/// The package in the folder `path`, which must be the extension `id`: how
/// devpage loads a package no document names, the live session's.
pub fn load(path: &Path, id: &str) -> Result<Arc<dyn Package>> {
    let (named, package) = package(path)?;
    if named != id {
        bail!("Package {named} is not the extension {id}");
    }
    Ok(package)
}
/// The id and the package of the folder `path`, after checking its manifest
/// and ABI.
fn package(path: &Path) -> Result<(String, Arc<dyn Package>)> {
    let root = path.canonicalize()?;
    let manifest: Manifest =
        serde_yaml::from_str(&std::fs::read_to_string(root.join("extension.yml"))?)?;
    if manifest.api != API {
        bail!("Unsupported extension API {}", manifest.api);
    }
    if manifest.id.is_empty()
        || !manifest
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("Invalid extension id");
    }
    if manifest.version.is_empty() {
        bail!("Extension version is empty");
    }
    let process = match manifest.run {
        Some(command) if command.is_empty() => bail!("Extension run names no program"),
        Some(command) => Some(Process {
            command,
            folder: root.clone(),
        }),
        None => None,
    };
    if let Some(assets) = manifest.assets
        && !root.join(assets).canonicalize()?.starts_with(&root)
    {
        bail!("Assets outside package");
    }
    let Some(library) = manifest.library else {
        let Some(process) = process else {
            bail!("Extension has neither a library nor a program to run");
        };
        let package = Plain {
            id: manifest.id.clone(),
            process,
        };
        return Ok((manifest.id, Arc::new(package)));
    };
    let target = manifest.target.unwrap_or_default();
    if target != env!("NATIVE_EXTENSION_TARGET") {
        bail!("Extension version/target mismatch: {target}");
    }
    let file = root.join(library).canonicalize()?;
    if !file.starts_with(&root) {
        bail!("Library outside package");
    }
    unsafe {
        let library = Library::new(&file).with_context(|| format!("Loading {}", file.display()))?;
        let entry: libloading::Symbol<unsafe extern "C" fn() -> *const VTable> =
            library.get(b"devpage_extension_v1")?;
        let table = entry().as_ref().context("Null extension table")?;
        if table.version != API || table.size != std::mem::size_of::<VTable>() {
            bail!("Extension ABI mismatch");
        }
        let invoke = table.invoke;
        let release = table.release;
        let package = Native {
            _library: library,
            root,
            id: manifest.id.clone(),
            invoke,
            release,
            process,
        };
        Ok((manifest.id, Arc::new(package)))
    }
}
/// Makes core load extension packages as native libraries.
pub fn install() {
    devpage_core::extensions::install_loader(Arc::new(NativeLoader));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_with_only_a_program_leaves_pages_as_they_are() {
        let dir = std::env::temp_dir().join(format!("devpage-program-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("extension.yml"),
            "id: clock\nversion: \"1\"\napi: 1\nrun: [clock]\n",
        )
        .unwrap();
        let package = load(&dir, "clock").unwrap();
        assert!(load(&dir, "live").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(package.process().is_some());
        package
            .invoke(serde_json::json!({"op":"create","id":"clock"}))
            .unwrap();
        let reply = package
            .invoke(serde_json::json!({"op":"augment","path":"README.md","source":"# T\n"}))
            .unwrap();
        assert!(reply["source"].is_null());
    }
}
