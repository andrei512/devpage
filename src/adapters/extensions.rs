//! Trusted native extension packages loaded by the executable, never by core.
use anyhow::{Context, Result, bail};
use devpage_core::extensions::{Loader, Package};
use devpage_extension_api::{API, VTable};
use libloading::Library;
use serde::Deserialize;
use serde_json::Value;
use std::{path::Path, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String,
    version: String,
    api: u32,
    target: String,
    library: String,
    assets: Option<String>,
}
struct Native {
    _library: Library,
    root: std::path::PathBuf,
    id: String,
    invoke: unsafe extern "C" fn(*const u8, usize) -> devpage_extension_api::Buffer,
    release: unsafe extern "C" fn(devpage_extension_api::Buffer),
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
}
struct NativeLoader;
impl Loader for NativeLoader {
    fn load(&self, path: &Path) -> Result<Arc<dyn Package>> {
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
        if manifest.version.is_empty() || manifest.target != env!("NATIVE_EXTENSION_TARGET") {
            bail!("Extension version/target mismatch: {}", manifest.target);
        }
        if let Some(assets) = manifest.assets
            && !root.join(assets).canonicalize()?.starts_with(&root)
        {
            bail!("Assets outside package");
        }
        let file = root.join(manifest.library).canonicalize()?;
        if !file.starts_with(&root) {
            bail!("Library outside package");
        }
        unsafe {
            let library =
                Library::new(&file).with_context(|| format!("Loading {}", file.display()))?;
            let entry: libloading::Symbol<unsafe extern "C" fn() -> *const VTable> =
                library.get(b"devpage_extension_v1")?;
            let table = entry().as_ref().context("Null extension table")?;
            if table.version != API || table.size != std::mem::size_of::<VTable>() {
                bail!("Extension ABI mismatch");
            }
            let invoke = table.invoke;
            let release = table.release;
            Ok(Arc::new(Native {
                _library: library,
                root,
                id: manifest.id,
                invoke,
                release,
            }))
        }
    }
}
pub fn install() {
    devpage_core::extensions::install_loader(Arc::new(NativeLoader));
}
