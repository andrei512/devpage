//! Extension contracts and captured state; the executable installs the native loader.
mod browser;
use crate::folder::ServedFolder;
use anyhow::{Result, anyhow, bail};
use browser::Browser;
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
/// An extension as a document's `extensions:` names it: its package and what it
/// is configured with.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The package's bundle; left out only for a system extension such as
    /// [`code`](crate::code).
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
    /// The document's whole `extensions:`, as JSON — what tells a changed
    /// configuration from the bound one.
    configuration: String,
    /// The instance's id: the first 16 hex digits of the SHA-256 of the
    /// document's path, `id` and `configuration`.
    owner: String,
    /// The package, loaded by the installed [`Loader`]; each request names
    /// this binding by `owner`.
    package: Arc<dyn Package>,
    /// The served folder its input and the files it shows must lie in.
    scope: ServedFolder,
    /// The file or folder it reads, as the package's `create` reply names it.
    input: PathBuf,
    /// The package's reply to `create`: the suffixes it owns, its input and the
    /// suffixes read there, its grammar, widgets and styles.
    descriptor: Value,
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
    /// The document-local name of this native extension.
    pub fn id(&self) -> String {
        self.0.lock().unwrap().id.clone()
    }
    /// The semantic reference declarations of a namespace at this revision.
    pub fn references(&self, namespace: &str) -> Result<Value> {
        self.call("references", json!({"namespace":namespace}))
    }
    /// The served folder of this binding, used to validate reference sources.
    pub fn scope(&self) -> ServedFolder {
        self.0.lock().unwrap().scope.clone()
    }

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
    /// The file or folder the extension reads.
    pub fn input(&self) -> PathBuf {
        self.0.lock().unwrap().input.clone()
    }
    /// Whether `file` is one the extension shows: its suffix is one the package
    /// declared and, when `file` is absolute, it lies in the input.
    pub fn owns(&self, file: &Path) -> bool {
        let s = self.0.lock().unwrap();
        if file.is_absolute() && !file.starts_with(&s.input) {
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
        let extensions = s.descriptor["input_extensions"]
            .as_array()
            .ok_or_else(|| anyhow!("Extension must declare input_extensions"))?;
        if let Err(error) = read(&s.scope, &s.input, &mut sources, extensions) {
            s.error = Some(error.to_string());
            return Err(error);
        }
        if s.error.is_none() && s.sources == sources && !s.revision.is_empty() {
            return Ok(false);
        }
        let mut declarations = BTreeMap::new();
        if let Some(languages) = s.descriptor["code_languages"].as_object() {
            for (file, source) in &sources {
                let suffix = Path::new(file)
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("");
                if let Some(language) = languages.get(suffix).and_then(Value::as_str) {
                    let grammar = crate::code::grammar(language, None)?;
                    declarations.insert(file.clone(), grammar.extract(source));
                }
            }
        }
        let reply = match s
            .package
            .invoke(json!({"op":"prepare","instance":s.owner,"sources":sources,"declarations":declarations}))
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
/// from its bundle under `base`, and prepares each; no `configs` unbinds it. A
/// configuration already bound in `scope` is kept as it is.
pub fn configure(
    doc: &Path,
    base: &Path,
    scope: &ServedFolder,
    configs: BTreeMap<String, Config>,
) -> Result<()> {
    let mut guard = instances().lock().unwrap();
    if configs.is_empty() {
        guard.remove(doc);
        return Ok(());
    }
    let configuration = serde_json::to_string(&configs)?;
    if guard.get(doc).is_some_and(|bindings| {
        bindings.iter().all(|b| {
            let s = b.0.lock().unwrap();
            s.configuration == configuration && s.scope == *scope
        })
    }) {
        return Ok(());
    }
    let loader = LOADER
        .get()
        .ok_or_else(|| anyhow!("Native extension loader unavailable"))?;
    let mut bindings = vec![];
    let mut claims: std::collections::BTreeSet<String> = ["md", "yml", "yaml", "design"]
        .into_iter()
        .map(String::from)
        .collect();
    for (id, config) in configs {
        let Some(path) = &config.package else {
            bail!("extensions: {id}: package is required — {id} is not a system extension");
        };
        let package = loader
            .load(&base.join(path))
            .map_err(|e| anyhow!("Extension {id}: {e:#}"))?;
        let owner = format!(
            "{:x}",
            Sha256::digest(format!("{}:{id}:{configuration}", doc.display()))
        )[..16]
            .to_string();
        let descriptor = package
            .invoke(json!({"op":"create","instance":owner,"id":id,"config":config.config}))?;
        for suffix in descriptor["file_extensions"]
            .as_array()
            .ok_or_else(|| anyhow!("Extension {id} file_extensions missing"))?
        {
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
        let input = base.join(
            descriptor["input"]
                .as_str()
                .ok_or_else(|| anyhow!("Extension input missing"))?,
        );
        if !input.canonicalize()?.starts_with(scope.path()) {
            bail!("Extension {id} input outside served folder");
        }
        let browser = Browser::new(
            &descriptor,
            &format!("{}/__devpage/extensions/{owner}/{id}/", scope.mount()),
        )?;
        let binding = Binding(Arc::new(Mutex::new(State {
            id,
            configuration: configuration.clone(),
            owner,
            package,
            scope: scope.clone(),
            input,
            descriptor,
            browser: Arc::new(browser),
            revision: String::new(),
            error: None,
            sources: BTreeMap::new(),
        })));
        binding.prepare()?;
        bindings.push(binding);
    }
    guard.insert(doc.to_path_buf(), bindings);
    Ok(())
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
        .map(Binding::input)
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
        let input = b.input();
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
                "references" => {
                    json!({"items":[{"path":["Signal"],"kind":"node","signature":"Signal(input: Price)","doc":"A signal from a price.","file":"model/nested/a.toy","line":1,"public":true}]})
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

    /// Semantic declarations use ordinary pages, search, prose and checking.
    #[test]
    fn a_native_reference_provider_uses_the_shared_catalog() {
        install_loader(Arc::new(Toy));
        let dir =
            std::env::temp_dir().join(format!("devpage-native-reference-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("model/nested")).unwrap();
        std::fs::write(dir.join("model/nested/a.toy"), "signal\n").unwrap();
        std::fs::write(dir.join("README.md"), "# Reference\n").unwrap();
        std::fs::write(
            dir.join("Signal.md"),
            "# ``nodes:Signal``\n\nThe domain explanation.\n",
        )
        .unwrap();
        std::fs::write(dir.join("devpage.yml"),"kind: site\ntitle: Native\nroot: README.md\nextensions:\n  toy:\n    package: ./package\n    config: {input: model}\n  code:\n    config:\n      nodes: {provider: toy, strict: true}\n").unwrap();
        let dir = dir.canonicalize().unwrap();
        let served = ServedFolder::new(&dir).unwrap();
        let doc = crate::document::Document::load(&dir.join("devpage.yml"), &served).unwrap();
        let index = crate::code::for_document(&doc.path()).unwrap();
        assert!(index.problems().is_empty(), "{:?}", index.problems());
        let page = index.page(&dir.join("reference/nodes/Signal")).unwrap();
        let markdown = index.markdown(&page);
        assert!(markdown.contains("A signal from a price."));
        assert!(markdown.contains("The domain explanation."));
        assert!(markdown.contains("model/nested/a.toy"));
        assert!(
            index.searched().iter().any(
                |item| item.title == "Signal" && item.prose.contains("The domain explanation.")
            )
        );
        assert_eq!(index.inventory()["nodes"].len(), 1);
        std::fs::remove_file(dir.join("model/nested/a.toy")).unwrap();
        crate::code::changed(&served, &[dir.join("model/nested/a.toy")]);
        assert!(index.page(&dir.join("reference/nodes/Signal")).is_none());
        assert!(
            index
                .problems()
                .iter()
                .any(|p| p.message.contains("source does not exist"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
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
}
