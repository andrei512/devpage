//! The code reference: a page for every module, type and function of the
//! code a site names, in the style of DocC — made from the code itself,
//! live, and never written to disk.
//!
//! A site enables it as the system extension `code`, one namespace per
//! language:
//!
//! ```yaml
//! extensions:
//!   code:
//!     config:
//!       rust:
//!         files: ["../core/src/**/*.rs"]
//! ```
//!
//! Each file is parsed by its namespace's grammar and query ([`extract`]),
//! every file's declarations are linked into items ([`rust`]), and a
//! item's page is rendered on request ([`render`]) at
//! `<site folder>/reference/<namespace>/<path>`. A save re-parses that one
//! file and links again in memory; nothing is cached on disk.

mod extract;
mod languages;
mod macros;
mod provider;
mod pyo3;
mod render;
mod rust;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use anyhow::{Context, Result, bail};
use globset::{GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::folder::{ServedFolder, lexical};

pub use extract::{Declaration, Extracted, Grammar, Visibility};

/// One thing of the code, after every file is linked: a module, a type, a
/// function — what a reference page is about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Item {
    /// Its path as the language names it: crate, modules, item.
    pub path: Vec<String>,
    /// What it is, as the query names it: `module`, `struct`, `function`….
    pub kind: String,
    /// Its declaration as written, without its body.
    pub signature: String,
    /// Its doc comment, as Markdown.
    pub doc: String,
    /// Where it is declared.
    pub file: PathBuf,
    /// The line it is declared on, from 1.
    pub line: usize,
    /// Whether a `pub` path reaches it from its crate's root.
    pub public: bool,
    /// Its fields, variants and methods, shown as sections of its page.
    pub members: Vec<Member>,
    /// The traits it implements, by name.
    pub implements: Vec<String>,
    /// A generated path container, not a source API requiring an abstract.
    pub container: bool,
}

impl Item {
    /// A member's route, qualified by kind only when its name is ambiguous.
    pub fn member_id(&self, member: &Member) -> String {
        if self
            .members
            .iter()
            .filter(|m| m.name == member.name)
            .count()
            > 1
        {
            format!("{}@{}", member.kind, member.name)
        } else {
            member.name.clone()
        }
    }
}

/// A part of an item shown on its page: a field, a variant, a method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Its name as declared: the last part of its id,
    /// `namespace:path::name`.
    pub name: String,
    /// What it is: `field`, `variant`, `method`….
    pub kind: String,
    /// Its declaration as written, without its body.
    pub signature: String,
    /// Its doc comment, as Markdown.
    pub doc: String,
    /// Where it is declared.
    pub file: PathBuf,
    /// The line it is declared on, from 1.
    pub line: usize,
    /// Whether it is reachable: `pub`, or a trait's item or an enum's variant.
    pub public: bool,
}

/// Every item of a namespace, by path, and the paths `pub use` makes
/// another name for.
#[derive(Debug, Default)]
pub struct Items {
    /// Every item, by its path.
    pub items: BTreeMap<Vec<String>, Item>,
    /// Each other name a `pub use` gives an item, to the path it stands for.
    pub aliases: BTreeMap<Vec<String>, Vec<String>>,
}

impl Items {
    /// Resolve aliases on any ancestor so an aliased type retains nested contracts.
    fn canonical(&self, path: &[String]) -> Vec<String> {
        let mut resolved = path.to_vec();
        let mut visited = std::collections::BTreeSet::new();
        while visited.insert(resolved.clone()) {
            let Some((length, target)) = (1..=resolved.len()).rev().find_map(|length| {
                self.aliases
                    .get(&resolved[..length])
                    .map(|target| (length, target))
            }) else {
                break;
            };
            let mut next = target.clone();
            next.extend_from_slice(&resolved[length..]);
            resolved = next;
        }
        resolved
    }
}

/// The `code` extension's `config`: its namespaces.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Config(
    /** Configurations keyed by reference namespace. */ BTreeMap<String, NamespaceConfig>,
);

/// One namespace as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamespaceConfig {
    /// The language its files are written in; the namespace's name when
    /// left out.
    language: Option<String>,
    /// Glob patterns of its files, relative to the document file.
    #[serde(default)]
    files: Vec<String>,
    /// Patterns to leave out of the namespace, relative to the site.
    #[serde(default)]
    exclude: Vec<String>,
    /// A query file replacing the language's built-in query.
    query: Option<String>,
    /// Whether items no `pub` path reaches get pages too.
    #[serde(default)]
    private: bool,
    /// A native extension contributing this namespace's semantic declarations.
    provider: Option<String>,
    /// Fail checking for empty inputs, parse failures and missing abstracts.
    #[serde(default)]
    strict: bool,
    /// The base for Python and TypeScript module paths, relative to the site.
    root: Option<String>,
    /// An optional namespace-wide module prefix, separated with `::`.
    module: Option<String>,
    /// `flat` uses captured names directly; `modules` uses source module paths.
    layout: Option<String>,
    /// Explicit local single-rule macro templates to expand.
    #[serde(default)]
    macros: Vec<String>,
    /// Item-position macros deliberately outside this reference surface.
    #[serde(default)]
    ignore_macros: Vec<String>,
}

/// One namespace: its files, its parser and the items they make now.
struct Namespace {
    /// Its name in the configuration, and in its pages' addresses.
    name: String,
    /// The language its files are written in.
    language: String,
    /// The parser of its files, replaced when its query file changes.
    grammar: RwLock<Arc<Grammar>>,
    /// The JSX grammar for .tsx files in a TypeScript namespace.
    tsx: RwLock<Option<Arc<Grammar>>>,
    /// Its `files` patterns, made absolute.
    patterns: GlobSet,
    /// Files intentionally outside this reference surface.
    exclusions: GlobSet,
    /// The folder each pattern's files are under: its literal prefix.
    roots: Vec<PathBuf>,
    /// The query file, when the project wrote its own; a change to it
    /// re-parses every file.
    query: Option<PathBuf>,
    /// Whether items no `pub` path reaches get pages too.
    private: bool,
    /// The extension that owns this namespace, when it is not grammar-linked.
    provider: Option<crate::extensions::Binding>,
    /// Diagnostics produced by the last provider refresh or file read.
    failures: RwLock<Vec<(PathBuf, usize, String)>>,
    /// Whether checking requires complete documented inputs.
    strict: bool,
    /// Base folder for non-Rust module paths.
    root: PathBuf,
    /// Prefix for module paths.
    module: Vec<String>,
    /// Whether names are absolute within the namespace.
    flat: bool,

    /// Each of its files' declarations, by path.
    files: Mutex<BTreeMap<PathBuf, Arc<Extracted>>>,
    /// Its items, as its files were last linked.
    items: RwLock<Arc<Items>>,
    /// Why its query file, saved while devpage runs, doesn't compile: its
    /// line and the error. `None` while the query in use is the file's.
    query_error: RwLock<Option<(usize, String)>>,
}

/// The code reference of one site.
pub struct Index {
    /// The site's folder: pages are under its `reference/`.
    base: PathBuf,
    /// The served folder the site is in, which page addresses are made from.
    served: ServedFolder,
    /// The configuration it was made from, to tell a reload from a change.
    configuration: String,
    /// Its namespaces, in the order of their names.
    namespaces: Vec<Namespace>,
    /// Markdown that adds to an item's page, by `namespace:path`.
    prose: RwLock<BTreeMap<String, Prose>>,
    /// Prose files for an id another prose file already adds to: each with
    /// its id and the file that adds to it first.
    twice: RwLock<Vec<(Prose, String, PathBuf)>>,
}

/// A Markdown file headed ``# ``rust:path`` ``: what it adds to that
/// item's page.
#[derive(Debug, Clone)]
pub struct Prose {
    /// The Markdown file it is read from.
    pub file: PathBuf,
    /// The line of its heading, from 1.
    pub line: usize,
    /// What follows its heading.
    pub body: String,
}

/// Every site's code reference, by its document file.
static INDEXES: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Index>>>> = OnceLock::new();

/// Every site's code reference, by its document file, made empty on first use.
fn indexes() -> &'static Mutex<BTreeMap<PathBuf, Arc<Index>>> {
    INDEXES.get_or_init(Default::default)
}

/// Enables the code reference of the site whose document file is `doc`,
/// in the folder `base`, from its `config` — or disables it when `config`
/// is `None`. Loading the same configuration again keeps the index.
pub fn configure(
    doc: &Path,
    base: &Path,
    served: &ServedFolder,
    config: Option<&serde_yaml::Value>,
) -> Result<()> {
    let Some(config) = config else {
        indexes().lock().unwrap().remove(doc);
        return Ok(());
    };
    let configuration = format!(
        "{}:{:?}",
        serde_yaml::to_string(config)?,
        crate::extensions::for_document(doc)
            .iter()
            .map(crate::extensions::Binding::identity)
            .collect::<Vec<_>>()
    );
    if let Some(index) = indexes().lock().unwrap().get(doc)
        && index.configuration == configuration
        && index.served == *served
    {
        return Ok(());
    }
    let Config(namespaces) = serde_yaml::from_value(config.clone()).context("extensions: code")?;
    if namespaces.is_empty() {
        bail!("extensions: code: config lists no namespaces");
    }
    let mut built = Vec::new();
    for (name, namespace) in namespaces {
        built.push(
            Namespace::new(&name, namespace, base, served, doc)
                .with_context(|| format!("extensions: code: {name}"))?,
        );
    }
    let index = Index {
        base: base.to_path_buf(),
        served: served.clone(),
        configuration,
        namespaces: built,
        prose: RwLock::default(),
        twice: RwLock::default(),
    };
    for namespace in &index.namespaces {
        namespace.parse_all();
    }
    index.read_prose();
    indexes()
        .lock()
        .unwrap()
        .insert(doc.to_path_buf(), Arc::new(index));
    Ok(())
}

/// Forgets every code reference of a document under `folder`: for server
/// mode, a version it no longer holds, whose items would otherwise stay in
/// memory for good.
pub fn forget(folder: &Path) {
    let canonical = folder.canonicalize().ok();
    indexes().lock().unwrap().retain(|doc, _| {
        !doc.starts_with(folder) && canonical.as_ref().is_none_or(|c| !doc.starts_with(c))
    });
}

/// The code reference of the site whose document file is `doc`, if it has
/// one.
pub fn for_document(doc: &Path) -> Option<Arc<Index>> {
    indexes().lock().unwrap().get(doc).cloned()
}

/// The code reference whose pages lie where `path` — a lexical path in the
/// served folder — is: its nearest site's, when that site has one and
/// `path` is under its `reference/`.
pub fn at(folder: &crate::folder::Folder, path: &Path) -> Option<Arc<Index>> {
    if !folder.served().contains(path) {
        return None;
    }
    let site = folder.site(path)?;
    // The site's document loaded, so its code reference is configured.
    folder.owner(path)?.document.ok()?;
    for_document(&site).filter(|index| index.owns(path))
}

/// Every folder a code reference of `served` reads from: watched whatever
/// the site's `watch` list or `.gitignore` says.
pub fn input_roots(served: &ServedFolder) -> Vec<PathBuf> {
    indexes()
        .lock()
        .unwrap()
        .values()
        .filter(|index| index.served == *served)
        .flat_map(|index| {
            index
                .namespaces
                .iter()
                .flat_map(|namespace| namespace.roots.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Brings every code reference of `served` up to date after `files`
/// changed: each changed source file parsed again, each namespace linked
/// again. Whether any page may now read differently.
pub fn changed(served: &ServedFolder, files: &[PathBuf]) -> bool {
    let all: Vec<Arc<Index>> = indexes()
        .lock()
        .unwrap()
        .values()
        .filter(|index| index.served == *served)
        .cloned()
        .collect();
    let mut any = false;
    for index in all {
        any |= index.changed(files);
    }
    any
}

impl Namespace {
    /// The namespace `name` as `config` writes it, its patterns relative to
    /// `base`, with no file parsed yet; an error when its language, query or a
    /// pattern is wrong.
    fn new(
        name: &str,
        config: NamespaceConfig,
        base: &Path,
        served: &ServedFolder,
        doc: &Path,
    ) -> Result<Self> {
        let language = config
            .language
            .as_deref()
            .unwrap_or(if config.provider.is_some() {
                "rust"
            } else {
                name
            });
        let provider = config
            .provider
            .as_deref()
            .map(|id| {
                crate::extensions::for_document(doc)
                    .into_iter()
                    .find(|binding| binding.id() == id)
                    .ok_or_else(|| anyhow::anyhow!("reference provider {id} is not configured"))
            })
            .transpose()?;
        if provider.is_some() && (!config.files.is_empty() || config.query.is_some()) {
            bail!("provider cannot be combined with files or query");
        }
        if config
            .layout
            .as_deref()
            .is_some_and(|layout| !matches!(layout, "flat" | "modules"))
        {
            bail!("layout must be flat or modules");
        }
        let root = lexical(&base.join(config.root.as_deref().unwrap_or(".")));
        if !served.contains(&root) {
            bail!("root is outside the served folder");
        }
        let query_file = match &config.query {
            Some(query) => match served.file(&base.join(query)) {
                Some(file) => Some(file),
                None => bail!("query {query} does not exist"),
            },
            None => None,
        };
        let mut grammar = grammar(language, query_file.as_deref())?;
        grammar.macros = config.macros.clone();
        grammar.ignore_macros = config.ignore_macros.clone();
        grammar.check_macros = config.strict && language == "rust";
        if config.files.is_empty() && provider.is_none() {
            bail!("files lists no patterns");
        }
        let mut patterns = GlobSetBuilder::new();
        let mut exclusions = GlobSetBuilder::new();
        let mut roots = Vec::new();
        for (position, pattern) in config.files.iter().chain(&config.exclude).enumerate() {
            let absolute = lexical(&base.join(pattern));
            if !served.contains(&absolute) {
                bail!("files {pattern} is outside the served folder");
            }
            // The folder before the first glob character is matched as
            // written — on Windows it starts `\\?\` — and the rest as the
            // pattern.
            let root = literal_prefix(&absolute);
            let rest = absolute.strip_prefix(&root).unwrap_or(&absolute);
            let text = if rest.as_os_str().is_empty() {
                // A pattern naming one file.
                globset::escape(&glob_path(&root))
            } else {
                format!(
                    "{}/{}",
                    globset::escape(glob_path(&root).trim_end_matches('/')),
                    glob_path(rest)
                )
            };
            let target = if position < config.files.len() {
                &mut patterns
            } else {
                &mut exclusions
            };
            target.add(
                globset::GlobBuilder::new(&text)
                    .literal_separator(true)
                    .build()
                    .with_context(|| format!("files {pattern}"))?,
            );
            if position < config.files.len() {
                roots.push(root);
            }
        }
        Ok(Self {
            name: name.to_string(),
            language: language.to_string(),
            grammar: RwLock::new(Arc::new(grammar)),
            tsx: RwLock::new(if language == "typescript" {
                Some(Arc::new(self::grammar("tsx", query_file.as_deref())?))
            } else {
                None
            }),
            patterns: patterns.build()?,
            exclusions: exclusions.build()?,
            roots,
            query: query_file,
            private: config.private,
            provider,
            failures: RwLock::default(),
            strict: config.strict,
            root,
            module: config
                .module
                .as_deref()
                .map(|m| m.split("::").map(str::to_string).collect())
                .unwrap_or_default(),
            flat: config.layout.as_deref() == Some("flat"),
            files: Mutex::default(),
            items: RwLock::default(),
            query_error: RwLock::default(),
        })
    }

    /// Whether `file` is one of the namespace's files.
    fn includes(&self, file: &Path) -> bool {
        self.patterns.is_match(glob_path(file)) && !self.exclusions.is_match(glob_path(file))
    }

    /// Every file of the namespace parsed, across the machine's cores, then
    /// linked.
    fn parse_all(&self) {
        if self.provider.is_some() {
            self.refresh_provider();
            return;
        }
        let mut found = Vec::new();
        for root in &self.roots {
            let walk = ignore::WalkBuilder::new(root)
                .hidden(true)
                .require_git(false)
                .build();
            for entry in walk.flatten() {
                let path = entry.path();
                if entry.file_type().is_some_and(|t| t.is_file()) && self.includes(path) {
                    found.push(path.to_path_buf());
                }
            }
        }
        found.sort();
        found.dedup();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = found.len().div_ceil(threads).max(1);
        let grammar = self.grammar.read().unwrap().clone();
        let grammar = &*grammar;
        let tsx = self.tsx.read().unwrap().clone();
        let tsx = &tsx;
        let parsed: Vec<(PathBuf, Arc<Extracted>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = found
                .chunks(chunk)
                .map(|files| {
                    scope.spawn(move || {
                        files
                            .iter()
                            .map(|file| {
                                let selected = if file.extension().is_some_and(|ext| ext == "tsx") {
                                    tsx.as_deref().unwrap_or(grammar)
                                } else {
                                    grammar
                                };
                                let extracted = match std::fs::read_to_string(file) {
                                    Ok(source) => selected.extract(&source),
                                    Err(error) => Extracted {
                                        failure: Some(format!("cannot read source: {error}")),
                                        ..Extracted::default()
                                    },
                                };
                                (file.clone(), Arc::new(extracted))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap_or_default())
                .collect()
        });
        *self.files.lock().unwrap() = parsed.into_iter().collect();
        self.link();
    }

    /// Every file's declarations linked into items.
    fn link(&self) {
        let files = self.files.lock().unwrap().clone();
        if self.language != "rust" || self.flat {
            let (items, failures) = languages::link(
                &files,
                &self.root,
                &self.module,
                self.flat,
                self.private,
                &self.language,
            );
            *self.items.write().unwrap() = Arc::new(items);
            *self.failures.write().unwrap() = failures;
            return;
        }
        let mut crates = HashMap::new();
        let units: Vec<(Vec<String>, rust::Unit)> = files
            .iter()
            .filter_map(|(file, extracted)| {
                Some((
                    rust::module_of(file, &mut crates)?,
                    rust::Unit { file, extracted },
                ))
            })
            .collect();
        let items = rust::link(&units, self.private);
        *self.items.write().unwrap() = Arc::new(items);
    }

    /// The items now.
    fn items(&self) -> Arc<Items> {
        self.items.read().unwrap().clone()
    }
}

/// Read one source unit with a built-in language and explicitly selected local
/// macro templates. Native extensions and coverage tests share this reader.
pub fn read_source(language: &str, source: &str, macros: &[String]) -> Result<Extracted> {
    let mut grammar = grammar(language, None)?;
    grammar.macros = macros.to_vec();
    Ok(grammar.extract(source))
}

/// The parser for `language`: its built-in grammar with its built-in query,
/// or the query in `query`.
pub(crate) fn grammar(language: &str, query: Option<&Path>) -> Result<Grammar> {
    let (grammar, built_in, docs) = match language {
        "rust" => (
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("rust.scm"),
            extract::Docs::Rust,
        ),
        "python" => (
            tree_sitter_python::LANGUAGE.into(),
            include_str!("python.scm"),
            extract::Docs::Python,
        ),
        "typescript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            include_str!("typescript.scm"),
            extract::Docs::TypeScript,
        ),
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            include_str!("typescript.scm"),
            extract::Docs::TypeScript,
        ),
        "pyo3" => (
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("rust.scm"),
            extract::Docs::PyO3,
        ),
        other => {
            bail!("language {other} is not one devpage knows: rust, python, typescript, tsx, pyo3")
        }
    };
    let source = match query {
        Some(file) => {
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?
        }
        None if language == "pyo3" => format!("{built_in}\n{}", include_str!("pyo3.scm")),
        None => built_in.to_string(),
    };
    Grammar::new(grammar, &source, docs).map_err(|error| match query {
        Some(file) => error.context(format!("query {}", file.display())),
        None => error,
    })
}

/// An absolute path as a glob pattern matches it: `/` between its parts on
/// every platform.
fn glob_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// The part of an absolute pattern before its first glob character: the
/// folder its files are under.
fn literal_prefix(pattern: &Path) -> PathBuf {
    let mut prefix = PathBuf::new();
    for part in pattern.components() {
        // A drive or a `\\?\` prefix is never part of the glob.
        let text = part.as_os_str().to_string_lossy();
        if matches!(part, std::path::Component::Normal(_)) && text.contains(['*', '?', '[', '{']) {
            break;
        }
        prefix.push(part);
    }
    prefix
}

/// One page of a code reference, as search reads it.
pub struct Searched {
    /// The address of its page, or of its section on its type's page.
    pub url: String,
    /// The name of what it is about.
    pub title: String,
    /// Where it is: the reference, then each module above it.
    pub trail: Vec<String>,
    /// Its doc comment and the prose that adds to it.
    pub prose: String,
    /// Its declaration.
    pub code: String,
}

/// The kinds the sidebar's tree lists; the rest are left to their pages.
const NAVIGATED: [&str; 13] = [
    "module",
    "struct",
    "enum",
    "union",
    "trait",
    "type",
    "macro",
    "class",
    "interface",
    "node",
    "schema",
    "rpc",
    "variant",
];

/// What a page address of the code reference names.
pub enum Page {
    /// A namespace's root: every crate it holds.
    Namespace(usize),
    /// An item, by its path.
    Item(usize, Vec<String>),
    /// A member of the item at the path, by its index.
    Member(usize, Vec<String>, usize),
}

impl Index {
    /// A source-owned inventory for coverage tools, keyed by namespace.
    /// It is the same catalog pages, search and prose validation use.
    pub fn inventory(&self) -> BTreeMap<String, Vec<Item>> {
        self.namespaces
            .iter()
            .map(|namespace| {
                (
                    namespace.name.clone(),
                    namespace.items().items.values().cloned().collect(),
                )
            })
            .collect()
    }
    /// The folder the reference's pages are under.
    fn reference(&self) -> PathBuf {
        self.base.join("reference")
    }

    /// The page at `path` — a lexical path in the served folder — if it is
    /// one of the reference's. An alias names the item it stands for.
    pub fn page(&self, path: &Path) -> Option<Page> {
        let relative = path.strip_prefix(self.reference()).ok()?;
        let mut parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned());
        let name = parts.next()?;
        let at = self.namespaces.iter().position(|n| n.name == name)?;
        let segments: Vec<String> = parts
            .map(|part| {
                percent_encoding::percent_decode_str(&part)
                    .decode_utf8()
                    .map(|s| s.into_owned())
                    .unwrap_or(part)
            })
            .collect();
        if segments.is_empty() {
            return Some(Page::Namespace(at));
        }
        let items = self.namespaces[at].items();
        let segments = items.canonical(&segments);
        if items.items.contains_key(&segments) {
            return Some(Page::Item(at, segments));
        }
        // A member's page is under its type's: `Store/read`.
        let (name, owner) = segments.split_last()?;
        let owner = items.canonical(owner);
        let members = &items.items.get(&owner)?.members;
        let matches: Vec<_> = members
            .iter()
            .enumerate()
            .filter(|(_, member)| {
                if let Some((kind, bare)) = name.split_once('@') {
                    member.kind == kind && member.name == bare
                } else {
                    member.name == *name
                }
            })
            .collect();
        if matches.len() != 1 {
            return None;
        }
        let member = matches[0].0;
        Some(Page::Member(at, owner, member))
    }

    /// The Markdown of the page `page`.
    pub fn markdown(&self, page: &Page) -> String {
        let prose = self.prose.read().unwrap();
        match page {
            Page::Namespace(at) => {
                let namespace = &self.namespaces[*at];
                render::namespace(self, &namespace.name, &namespace.items())
            }
            Page::Item(at, path) => {
                let namespace = &self.namespaces[*at];
                let items = namespace.items();
                let id = format!("{}:{}", namespace.name, path.join("::"));
                render::item(
                    self,
                    &namespace.name,
                    &items,
                    &items.items[path],
                    prose.get(&id),
                )
            }
            Page::Member(at, path, member) => {
                let namespace = &self.namespaces[*at];
                let items = namespace.items();
                let owner = &items.items[path];
                let member = &owner.members[*member];
                let id = format!(
                    "{}:{}::{}",
                    namespace.name,
                    path.join("::"),
                    owner.member_id(member)
                );
                render::member(self, &namespace.name, owner, member, prose.get(&id))
            }
        }
    }

    /// The address of a page: a namespace's with no `path`, an item's, or a
    /// member's under its type's.
    pub(crate) fn address(&self, namespace: &str, path: &[String]) -> String {
        let mut file = self.reference().join(namespace);
        for segment in path {
            file.push(segment);
        }
        self.served.url(&file).unwrap_or_default()
    }

    /// The page the sidebar lists for `page`: its own, or — for a member,
    /// which the sidebar leaves to its type's page — its type's.
    pub fn listed(&self, page: &Page) -> PathBuf {
        let (at, mut path) = match page {
            Page::Namespace(at) => (*at, &[][..]),
            Page::Item(at, path) | Page::Member(at, path, _) => (*at, &path[..]),
        };
        // A function, a constant: the sidebar lists its module.
        let items = self.namespaces[at].items();
        if let Some(item) = items.items.get(path)
            && !NAVIGATED.contains(&item.kind.as_str())
        {
            path = &path[..path.len() - 1];
        }
        let mut file = self.reference().join(&self.namespaces[at].name);
        for segment in path {
            file.push(segment);
        }
        file
    }

    /// Every page of the reference as search reads it: each item's and
    /// each member's, in path order.
    pub fn searched(&self) -> Vec<Searched> {
        let prose = self.prose.read().unwrap();
        let mut out = Vec::new();
        for namespace in &self.namespaces {
            let items = namespace.items();
            let reference = format!("{} reference", render::title_case(&namespace.name));
            let added = |path: &[String]| {
                let id = format!("{}:{}", namespace.name, path.join("::"));
                prose.get(&id).map(|p| p.body.clone()).unwrap_or_default()
            };
            for (path, item) in &items.items {
                let mut trail = vec![reference.clone()];
                trail.extend(path[..path.len() - 1].iter().cloned());
                out.push(Searched {
                    url: self.address(&namespace.name, path),
                    title: path.last().cloned().unwrap_or_default(),
                    trail: trail.clone(),
                    prose: format!("{}\n\n{}", render::doc_markdown(&item.doc), added(path)),
                    code: item.signature.clone(),
                });
                trail.push(path.last().cloned().unwrap_or_default());
                for member in &item.members {
                    let mut member_path = path.clone();
                    member_path.push(item.member_id(member));
                    out.push(Searched {
                        url: self.address(&namespace.name, &member_path),
                        title: member.name.clone(),
                        trail: trail.clone(),
                        prose: format!(
                            "{}\n\n{}",
                            render::doc_markdown(&member.doc),
                            added(&member_path)
                        ),
                        code: member.signature.clone(),
                    });
                }
            }
        }
        out
    }

    /// The sidebar's tree under a Topics entry for the page at `path`: for a
    /// namespace, its crates; under each module, its modules and types by
    /// kind. Functions and members are left to the pages, so the tree every
    /// page carries stays small.
    pub fn navigator(&self, path: &Path) -> Vec<crate::nav::Group> {
        let Some(page) = self.page(path) else {
            return Vec::new();
        };
        let (at, under) = match page {
            Page::Namespace(at) => (at, Vec::new()),
            Page::Item(at, path) => (at, path),
            Page::Member(..) => return Vec::new(),
        };
        let namespace = &self.namespaces[at];
        let items = namespace.items();
        self.nav_children(&namespace.name, &items, &under)
    }

    /// The sidebar's groups for the items directly under `under`: its
    /// modules, each with its own, and its types, by kind.
    fn nav_children(
        &self,
        namespace: &str,
        items: &Items,
        under: &[String],
    ) -> Vec<crate::nav::Group> {
        const SHOWN: [(&str, &str); 13] = [
            ("module", "Modules"),
            ("struct", "Structures"),
            ("enum", "Enumerations"),
            ("union", "Unions"),
            ("trait", "Traits"),
            ("type", "Type Aliases"),
            ("macro", "Macros"),
            ("class", "Classes"),
            ("interface", "Interfaces"),
            ("node", "Nodes"),
            ("schema", "Schemas"),
            ("rpc", "RPC Methods"),
            ("variant", "Variants"),
        ];
        let children: Vec<&Item> = items
            .items
            .values()
            .filter(|item| item.path.len() == under.len() + 1 && item.path.starts_with(under))
            .collect();
        let mut groups = Vec::new();
        for (kind, title) in SHOWN {
            let entries: Vec<crate::nav::Entry> = children
                .iter()
                .filter(|item| item.kind == kind)
                .map(|item| crate::nav::Entry {
                    label: item.path.last().cloned().unwrap_or_default(),
                    file: {
                        let mut file = self.reference().join(namespace);
                        for segment in &item.path {
                            file.push(segment);
                        }
                        file
                    },
                    children: if NAVIGATED.contains(&kind) {
                        self.nav_children(namespace, items, &item.path)
                    } else {
                        Vec::new()
                    },
                })
                .collect();
            if !entries.is_empty() {
                // A namespace's crates are one untitled group.
                let title = (!under.is_empty()).then(|| title.to_string());
                groups.push(crate::nav::Group { title, entries });
            }
        }
        groups
    }

    /// A source file's address and its name, for an item's "declared in".
    pub(crate) fn source(&self, file: &Path) -> Option<(String, String)> {
        Some((self.served.url(file)?, self.served.name(file)?))
    }

    /// The ids `path`'s page holds, for a link's `#fragment`; `None` when
    /// `path` is no page of the reference.
    pub fn ids(&self, path: &Path) -> Option<Vec<String>> {
        let page = self.page(path)?;
        let rendered = crate::page::content(
            &crate::widgets::Widgets::default(),
            path,
            self.markdown(&page),
        );
        Some(rendered.ids().into_iter().collect())
    }

    /// Whether `path` lies where the reference's pages are.
    pub fn owns(&self, path: &Path) -> bool {
        path.starts_with(self.reference())
    }

    /// The page nearest to `path`, which names none: the item of the same
    /// last name, else the closest path by edit distance.
    pub fn closest(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(self.reference()).ok()?;
        let mut parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned());
        let name = parts.next()?;
        let namespace = self.namespaces.iter().find(|n| n.name == name)?;
        let wanted: Vec<String> = parts.collect();
        let last = wanted.last()?;
        let items = namespace.items();
        let joined = wanted.join("::");
        // Every page: each item's, and each of its members'.
        let pages = items.items.iter().flat_map(|(path, item)| {
            std::iter::once(path.clone()).chain(item.members.iter().map(move |member| {
                let mut path = path.clone();
                path.push(member.name.clone());
                path
            }))
        });
        let best = pages.min_by_key(|path| {
            (
                path.last() != Some(last),
                distance(&path.join("::"), &joined),
            )
        })?;
        Some(self.address(&namespace.name, &best))
    }

    /// Every changed file of a namespace parsed again and the namespace
    /// linked again; prose read again when a Markdown file changed.
    fn changed(&self, files: &[PathBuf]) -> bool {
        let mut any = false;
        for namespace in &self.namespaces {
            if namespace.provider.is_some() {
                namespace.refresh_provider();
                any = true;
                continue;
            }
            // A new query reads every file anew; one that doesn't compile
            // is a problem, and the last that did keeps reading.
            if namespace
                .query
                .as_ref()
                .is_some_and(|query| files.iter().any(|f| f == query))
            {
                match grammar(&namespace.language, namespace.query.as_deref()) {
                    Ok(mut grammar) => {
                        if namespace.language == "typescript" {
                            match self::grammar("tsx", namespace.query.as_deref()) {
                                Ok(tsx) => *namespace.tsx.write().unwrap() = Some(Arc::new(tsx)),
                                Err(error) => {
                                    *namespace.query_error.write().unwrap() =
                                        Some((1, error.to_string()));
                                    any = true;
                                    continue;
                                }
                            }
                        }
                        {
                            let old = namespace.grammar.read().unwrap();
                            grammar.macros = old.macros.clone();
                            grammar.ignore_macros = old.ignore_macros.clone();
                            grammar.check_macros = old.check_macros;
                        }
                        *namespace.grammar.write().unwrap() = Arc::new(grammar);
                        *namespace.query_error.write().unwrap() = None;
                        namespace.parse_all();
                    }
                    Err(error) => {
                        let message = error.root_cause().to_string();
                        let line = message
                            .strip_prefix("line ")
                            .and_then(|rest| rest.split(':').next())
                            .and_then(|line| line.parse().ok())
                            .unwrap_or(1);
                        let message = message
                            .split_once(": ")
                            .map_or(message.clone(), |(_, rest)| rest.to_string());
                        *namespace.query_error.write().unwrap() = Some((line, message));
                    }
                }
                any = true;
                continue;
            }
            // A file new to the namespace walks its folders again, so a
            // `.gitignore`d one is still left out.
            if files.iter().any(|file| {
                namespace.includes(file)
                    && file.is_file()
                    && !namespace.files.lock().unwrap().contains_key(file)
            }) {
                namespace.parse_all();
                any = true;
                continue;
            }
            let mut touched = false;
            for file in files {
                if !namespace.includes(file) {
                    continue;
                }
                let mut parsed = namespace.files.lock().unwrap();
                if !parsed.contains_key(file) {
                    continue;
                }
                match std::fs::read_to_string(file) {
                    Ok(source) => {
                        let grammar = if file.extension().is_some_and(|ext| ext == "tsx") {
                            namespace
                                .tsx
                                .read()
                                .unwrap()
                                .clone()
                                .unwrap_or_else(|| namespace.grammar.read().unwrap().clone())
                        } else {
                            namespace.grammar.read().unwrap().clone()
                        };
                        parsed.insert(file.clone(), Arc::new(grammar.extract(&source)));
                    }
                    Err(_) => {
                        parsed.remove(file);
                    }
                }
                touched = true;
            }
            if touched {
                namespace.link();
                any = true;
            }
        }
        if files.iter().any(|file| {
            file.extension().is_some_and(|ext| ext == "md") && file.starts_with(&self.base)
        }) {
            any |= self.read_prose();
        }
        any
    }

    /// Every Markdown file of the site headed ``# ``<namespace>:<path>`` ``:
    /// whether what they add changed.
    fn read_prose(&self) -> bool {
        let mut found: BTreeMap<String, Prose> = BTreeMap::new();
        let mut twice = Vec::new();
        let walk = ignore::WalkBuilder::new(&self.base)
            .hidden(true)
            .require_git(false)
            .build();
        let mut files: Vec<PathBuf> = walk
            .flatten()
            .map(|entry| entry.into_path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        // In path order, so the first of two for one id is always the same.
        files.sort();
        for path in files {
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((id, line, body)) = prose_of(&source) else {
                continue;
            };
            let added = Prose {
                file: path,
                line,
                body,
            };
            match found.get(&id) {
                Some(first) => twice.push((added, id, first.file.clone())),
                None => {
                    found.insert(id, added);
                }
            }
        }
        *self.twice.write().unwrap() = twice;
        let mut prose = self.prose.write().unwrap();
        let same = prose.len() == found.len()
            && prose
                .iter()
                .zip(&found)
                .all(|((a, x), (b, y))| a == b && x.body == y.body && x.file == y.file);
        *prose = found;
        !same
    }

    /// Every file the reference is made from, for a publish to carry: each
    /// namespace's source files, the `Cargo.toml` that names each one's
    /// crate, its query file, and every prose file.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files = std::collections::BTreeSet::new();
        for namespace in &self.namespaces {
            for file in namespace.files.lock().unwrap().keys() {
                files.insert(file.clone());
                let mut at = file.parent();
                while let Some(folder) = at {
                    let manifest = folder.join("Cargo.toml");
                    if manifest.is_file() {
                        files.insert(manifest);
                        break;
                    }
                    at = folder.parent();
                }
            }
            files.extend(namespace.query.clone());
        }
        let prose = self.prose.read().unwrap();
        files.extend(prose.values().map(|prose| prose.file.clone()));
        files.extend(
            self.twice
                .read()
                .unwrap()
                .iter()
                .map(|(prose, ..)| prose.file.clone()),
        );
        files
            .into_iter()
            .filter(|file| self.served.contains(file))
            .collect()
    }

    /// What is wrong with the reference, for its pages to list and
    /// `devpage check` to report: a prose file whose id names nothing, and
    /// a query saved while devpage runs that doesn't compile.
    pub fn problems(&self) -> Vec<crate::problem::Problem> {
        let mut problems: Vec<crate::problem::Problem> = self
            .orphans()
            .into_iter()
            .map(|(prose, id)| crate::problem::Problem {
                file: self.served.name(&prose.file).unwrap_or_default(),
                line: Some(prose.line),
                column: None,
                message: format!("``{id}`` names no code"),
                target: None,
            })
            .collect();
        for (prose, id, first) in self.twice.read().unwrap().iter() {
            problems.push(crate::problem::Problem {
                file: self.served.name(&prose.file).unwrap_or_default(),
                line: Some(prose.line),
                column: None,
                message: format!(
                    "``{id}`` is added to by {} already",
                    self.served.name(first).unwrap_or_default()
                ),
                target: None,
            });
        }
        for namespace in &self.namespaces {
            let mut failures = namespace.failures.read().unwrap().clone();
            for (file, extracted) in namespace.files.lock().unwrap().iter() {
                if let Some(failure) = &extracted.failure {
                    failures.push((file.clone(), 1, failure.clone()));
                }
                failures.extend(
                    extracted
                        .errors
                        .iter()
                        .map(|line| (file.clone(), *line, "source does not parse".into())),
                );
            }
            if namespace.strict {
                let items = namespace.items();
                if items.items.is_empty() {
                    failures.push((
                        self.base.clone(),
                        1,
                        format!("{}: no reference items", namespace.name),
                    ));
                }
                for item in items.items.values() {
                    if !item.container && render::split_doc(&item.doc).0.is_empty() {
                        failures.push((
                            item.file.clone(),
                            item.line,
                            format!(
                                "{}:{} has no abstract",
                                namespace.name,
                                item.path.join("::")
                            ),
                        ));
                    }
                    let mut member_ids = std::collections::BTreeSet::new();
                    for member in &item.members {
                        if !member_ids.insert((&member.kind, &member.name)) {
                            failures.push((
                                member.file.clone(),
                                member.line,
                                format!(
                                    "duplicate reference {}:{}::{}@{}",
                                    namespace.name,
                                    item.path.join("::"),
                                    member.kind,
                                    member.name
                                ),
                            ));
                        }
                        if render::split_doc(&member.doc).0.is_empty() {
                            failures.push((
                                member.file.clone(),
                                member.line,
                                format!(
                                    "{}:{}::{} has no abstract",
                                    namespace.name,
                                    item.path.join("::"),
                                    member.name
                                ),
                            ));
                        }
                    }
                }
            }
            problems.extend(failures.into_iter().map(|(file, line, message)| {
                crate::problem::Problem {
                    file: self.served.name(&file).unwrap_or_default(),
                    line: Some(line),
                    column: None,
                    message,
                    target: None,
                }
            }));
            if let Some((line, message)) = namespace.query_error.read().unwrap().clone()
                && let Some(query) = &namespace.query
            {
                problems.push(crate::problem::Problem {
                    file: self.served.name(query).unwrap_or_default(),
                    line: Some(line),
                    column: None,
                    message: format!("query doesn't compile: {message}"),
                    target: None,
                });
            }
        }
        problems
    }

    /// Every prose file whose id names no item: `(file, id)`.
    pub fn orphans(&self) -> Vec<(Prose, String)> {
        let prose = self.prose.read().unwrap();
        prose
            .iter()
            .filter(|(id, _)| {
                let Some((name, path)) = id.split_once(':') else {
                    return true;
                };
                // An item's id, or a member's under its type's.
                let mut file = self.reference().join(name);
                for segment in path.split("::") {
                    file.push(segment);
                }
                self.page(&file).is_none()
            })
            .map(|(id, prose)| (prose.clone(), id.clone()))
            .collect()
    }
}

/// A prose file's id and what it adds: the file's first line is
/// ``# ``<id>`` ``; the rest is the body.
fn prose_of(source: &str) -> Option<(String, usize, String)> {
    let mut lines = source.lines().enumerate();
    let (at, first) = lines.find(|(_, line)| !line.trim().is_empty())?;
    let id = first
        .trim()
        .strip_prefix("# ``")?
        .strip_suffix("``")?
        .trim()
        .to_string();
    id.contains(':').then(|| {
        let body = lines.map(|(_, line)| line).collect::<Vec<_>>().join("\n");
        (id, at + 1, body)
    })
}

/// The edit distance between two names.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(previous + usize::from(ca != *cb));
            previous = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::{Visibility, grammar};
    /// Other language declarations enter the same native item catalog.
    #[test]
    fn python_and_typescript_have_native_grammars() {
        let python = grammar("python", None).unwrap().extract("class Graph:\n    \"\"\"A graph.\"\"\"\n    def run(self):\n        \"\"\"Run it.\"\"\"\n        pass\n");
        assert_eq!(python.declarations[0].name, "Graph");
        assert_eq!(python.declarations[0].doc, "A graph.");
        assert_eq!(python.declarations[1].parent, Some(0));
        let ts = grammar("typescript", None)
            .unwrap()
            .extract("/** A graph. */\nexport interface Graph { /** Its id. */ id: string; }\n");
        assert_eq!(ts.declarations[0].name, "Graph");
        assert_eq!(ts.declarations[0].doc, "A graph.");
        assert_eq!(ts.declarations[0].visibility, Visibility::Public);
        assert_eq!(ts.declarations[1].parent, Some(0));
    }

    /// Broken syntax, empty inputs and undocumented API are concrete check failures.
    #[test]
    fn strict_grammar_checks_cannot_pass_an_empty_or_degraded_catalog() {
        let dir =
            std::env::temp_dir().join(format!("devpage-strict-reference-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let served = crate::folder::ServedFolder::new(&dir).unwrap();
        let config:serde_yaml::Value=serde_yaml::from_str("ts: {language: typescript, files: ['*.ts', '*.tsx'], exclude: ['ignored.ts'], strict: true}").unwrap();
        let doc = dir.join("devpage.yml");
        super::configure(&doc, &dir, &served, Some(&config)).unwrap();
        let index = super::for_document(&doc).unwrap();
        assert!(
            index
                .problems()
                .iter()
                .any(|p| p.message.contains("no reference items"))
        );
        let file = dir.join("widget.tsx");
        std::fs::write(
            &file,
            "export function Widget() { return <span>Ready</span>; }\n",
        )
        .unwrap();
        index.changed(std::slice::from_ref(&file));
        assert!(
            index
                .problems()
                .iter()
                .any(|p| p.message.contains("has no abstract"))
        );
        assert!(
            !index
                .problems()
                .iter()
                .any(|p| p.message.contains("does not parse"))
        );
        std::fs::write(
            &file,
            "/** A widget. */\nexport function Widget() { return <span>Ready</span>; }\n",
        )
        .unwrap();
        std::fs::write(dir.join("ignored.ts"), "not valid typescript !!!").unwrap();
        index.changed(&[file.clone(), dir.join("ignored.ts")]);
        assert!(index.problems().is_empty(), "{:?}", index.problems());
        std::fs::write(&file, "/** A widget. */\nexport function Widget( {\n").unwrap();
        index.changed(std::slice::from_ref(&file));
        assert!(
            index
                .problems()
                .iter()
                .any(|p| p.message.contains("does not parse"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Local exports and imported aliases are resolved from the same TypeScript files.
    #[test]
    fn typescript_exports_resolve_named_imports_without_copying_docs() {
        let root = std::path::Path::new("/project");
        let sources = [
            (
                "tree.ts",
                "/** Flatten a tree. */\nexport function flatten() {}\n",
            ),
            (
                "View.tsx",
                "import {flatten} from './tree';\nexport {flatten};\n/** A view. */\nexport function View(){return <span/>;}\n",
            ),
            (
                "local.ts",
                "/** A local export. */\nfunction kept(){}\nexport {kept as publicName, kept as second};\nfunction hidden(){}\n/** A visible value. */\nexport const value=1;\nexport {value as first, value as third};\n",
            ),
        ];
        let files = sources
            .into_iter()
            .map(|(file, source)| {
                (
                    root.join(file),
                    std::sync::Arc::new(
                        grammar(
                            if file.ends_with("tsx") {
                                "tsx"
                            } else {
                                "typescript"
                            },
                            None,
                        )
                        .unwrap()
                        .extract(source),
                    ),
                )
            })
            .collect();
        let (items, failures) =
            super::languages::link(&files, root, &[], false, false, "typescript");
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(
            items.items[&vec!["View".into(), "flatten".into()]].doc,
            "Flatten a tree."
        );
        assert!(
            items
                .items
                .contains_key(&vec!["local".into(), "publicName".into()])
        );
        for name in ["second", "value", "first", "third"] {
            assert!(
                items.items.contains_key(&vec!["local".into(), name.into()]),
                "{name}"
            );
        }
        assert!(
            !items
                .items
                .contains_key(&vec!["local".into(), "hidden".into()])
        );
    }

    /// Navigation and route identity use every native catalog kind consistently.
    #[test]
    fn catalog_kinds_are_navigable_and_member_collisions_have_unambiguous_routes() {
        let dir =
            std::env::temp_dir().join(format!("devpage-catalog-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let served = crate::folder::ServedFolder::new(&dir).unwrap();
        let config: serde_yaml::Value =
            serde_yaml::from_str("ts: {language: typescript, files: ['*.ts'], strict: true}")
                .unwrap();
        let doc = dir.join("devpage.yml");
        super::configure(&doc, &dir, &served, Some(&config)).unwrap();
        let index = super::for_document(&doc).unwrap();
        let mut items = super::Items::default();
        for kind in ["class", "interface", "node", "schema", "rpc", "variant"] {
            let path = vec![kind.to_string()];
            items.items.insert(
                path.clone(),
                super::Item {
                    path,
                    kind: kind.into(),
                    doc: "A documented contract.".into(),
                    public: true,
                    ..super::Item::default()
                },
            );
        }
        let mut owner = super::Item {
            path: vec!["Cell".into()],
            kind: "struct".into(),
            doc: "A value.".into(),
            public: true,
            ..super::Item::default()
        };
        for kind in ["field", "method"] {
            owner.members.push(super::Member {
                name: "present".into(),
                kind: kind.into(),
                signature: format!("{kind} present"),
                doc: format!("The {kind}."),
                file: dir.join("cell.rs"),
                line: 1,
                public: true,
            });
        }
        items.items.insert(owner.path.clone(), owner);
        items.aliases.insert(vec!["old".into()], Vec::new());
        let path = vec!["payload_<i>".into()];
        items.items.insert(
            path.clone(),
            super::Item {
                path,
                kind: "schema".into(),
                doc: "A payload.".into(),
                ..super::Item::default()
            },
        );
        *index.namespaces[0].items.write().unwrap() = std::sync::Arc::new(items);
        let navigation = index.navigator(&dir.join("reference/ts"));
        let names: Vec<_> = navigation
            .iter()
            .flat_map(|g| &g.entries)
            .map(|e| e.label.as_str())
            .collect();
        for kind in ["class", "interface", "node", "schema", "rpc", "variant"] {
            assert!(names.contains(&kind), "{kind}");
        }
        assert!(index.page(&dir.join("reference/ts/Cell/present")).is_none());
        for kind in ["field", "method"] {
            let route = format!("reference/ts/Cell/{kind}@present");
            let page = index.page(&dir.join(&route)).unwrap();
            assert!(
                index
                    .page(&dir.join(format!("reference/ts/old/Cell/{kind}@present")))
                    .is_some()
            );
            assert!(index.markdown(&page).contains(&format!("The {kind}.")));
            assert!(
                index
                    .searched()
                    .iter()
                    .any(|s| s.url.ends_with(&format!("/{kind}@present")))
            );
        }
        let page = index.page(&dir.join("reference/ts/Cell")).unwrap();
        let markdown = index.markdown(&page);
        assert!(markdown.contains("/field@present") && markdown.contains("/method@present"));
        assert!(
            index
                .page(&dir.join("reference/ts/payload_%3Ci%3E"))
                .is_some()
        );
        assert!(index.problems().is_empty(), "{:?}", index.problems());
        let held = index.namespaces[0].items();
        let mut duplicate = super::Items {
            items: held.items.clone(),
            aliases: held.aliases.clone(),
        };
        let cell = duplicate.items.get_mut(&vec!["Cell".into()]).unwrap();
        cell.members.push(cell.members[0].clone());
        *index.namespaces[0].items.write().unwrap() = std::sync::Arc::new(duplicate);
        assert!(
            index
                .problems()
                .iter()
                .any(|p| p.message.contains("duplicate reference"))
        );
        assert!(
            index
                .page(&dir.join("reference/ts/Cell/field@present"))
                .is_none()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A version server mode drops takes its code reference with it.
    #[test]
    fn a_forgotten_folder_holds_no_code_reference() {
        let dir = std::env::temp_dir().join(format!("devpage-code-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lib/src")).unwrap();
        std::fs::write(dir.join("lib/Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        std::fs::write(dir.join("lib/src/lib.rs"), "pub struct A;\n").unwrap();
        std::fs::write(dir.join("README.md"), "# A\n").unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: A\nroot: README.md\nextensions:\n  code:\n    config:\n      rust:\n        files: [\"lib/src/**/*.rs\"]\n",
        )
        .unwrap();
        let served = crate::folder::ServedFolder::new(&dir).unwrap();
        let doc = crate::document::Document::load(&dir.join("devpage.yml"), &served).unwrap();
        assert!(super::for_document(&doc.path()).is_some());
        super::forget(&dir);
        assert!(super::for_document(&doc.path()).is_none());
    }

    /// devpage's own code, as its docs' code reference reads it: every
    /// item, field, variant and method has a doc comment, so every page of
    /// the reference says what it is.
    #[test]
    fn every_item_of_devpages_code_is_documented() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let served = crate::folder::ServedFolder::new(&root).unwrap();
        let doc = crate::document::Document::load(&root.join("docs/devpage.yml"), &served).unwrap();
        let index = super::for_document(&doc.path()).unwrap();
        let mut undocumented = Vec::new();
        for namespace in &index.namespaces {
            for item in namespace.items().items.values() {
                let file = served.name(&item.file).unwrap_or_default();
                if !item.container && item.doc.trim().is_empty() {
                    undocumented.push(format!("{file}:{}: {}", item.line, item.path.join("::")));
                }
                for member in &item.members {
                    if member.doc.trim().is_empty() {
                        let file = served.name(&member.file).unwrap_or_default();
                        undocumented.push(format!(
                            "{file}:{}: {}::{}",
                            member.line,
                            item.path.join("::"),
                            member.name
                        ));
                    }
                }
            }
        }
        assert!(
            undocumented.is_empty(),
            "undocumented:\n{}",
            undocumented.join("\n")
        );
    }
}
