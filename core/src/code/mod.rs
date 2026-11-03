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
//! every file's declarations are linked into targets ([`rust`]), and a
//! target's page is rendered on request ([`render`]) at
//! `<site folder>/reference/<namespace>/<path>`. A save re-parses that one
//! file and links again in memory; nothing is cached on disk.

mod extract;
mod render;
mod rust;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use anyhow::{Context, Result, bail};
use globset::{GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::folder::{ServedFolder, lexical, slashed};

pub use extract::{Declaration, Extracted, Grammar, Visibility};

/// One thing of the code, after every file is linked: a module, a type, a
/// function — what a reference page is about.
#[derive(Debug, Clone, Default)]
pub struct Target {
    /// Its path as the language names it: crate, modules, item.
    pub path: Vec<String>,
    pub kind: String,
    pub signature: String,
    pub doc: String,
    /// Where it is declared.
    pub file: PathBuf,
    pub line: usize,
    /// Whether a `pub` path reaches it from its crate's root.
    pub public: bool,
    /// Its fields, variants and methods, shown as sections of its page.
    pub members: Vec<Member>,
    /// The traits it implements, by name.
    pub implements: Vec<String>,
}

/// A part of a target shown on its page: a field, a variant, a method.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub doc: String,
    pub file: PathBuf,
    pub line: usize,
    pub public: bool,
}

/// Every target of a namespace, by path, and the paths `pub use` makes
/// another name for.
#[derive(Debug, Default)]
pub struct Targets {
    pub targets: BTreeMap<Vec<String>, Target>,
    pub aliases: BTreeMap<Vec<String>, Vec<String>>,
}

/// The `code` extension's `config`: its namespaces.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Config(BTreeMap<String, NamespaceConfig>);

/// One namespace as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamespaceConfig {
    /// The language its files are written in; the namespace's name when
    /// left out.
    language: Option<String>,
    /// Glob patterns of its files, relative to the document file.
    files: Vec<String>,
    /// A query file replacing the language's built-in query.
    query: Option<String>,
    /// Whether items no `pub` path reaches get pages too.
    #[serde(default)]
    private: bool,
}

/// One namespace: its files, its parser and the targets they make now.
struct Namespace {
    name: String,
    language: String,
    grammar: RwLock<Arc<Grammar>>,
    patterns: GlobSet,
    roots: Vec<PathBuf>,
    /// The query file, when the project wrote its own; a change to it
    /// re-parses every file.
    query: Option<PathBuf>,
    private: bool,
    files: Mutex<BTreeMap<PathBuf, Arc<Extracted>>>,
    targets: RwLock<Arc<Targets>>,
}

/// The code reference of one site.
pub struct Index {
    /// The site's folder: pages are under its `reference/`.
    base: PathBuf,
    served: ServedFolder,
    /// The configuration it was made from, to tell a reload from a change.
    configuration: String,
    namespaces: Vec<Namespace>,
    /// Markdown that adds to a target's page, by `namespace:path`.
    prose: RwLock<BTreeMap<String, Prose>>,
}

/// A Markdown file headed ``# ``rust:path`` ``: what it adds to that
/// target's page.
#[derive(Debug, Clone)]
pub struct Prose {
    pub file: PathBuf,
    pub body: String,
}

static INDEXES: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Index>>>> = OnceLock::new();

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
    let configuration = serde_yaml::to_string(config)?;
    if let Some(index) = indexes().lock().unwrap().get(doc)
        && index.configuration == configuration
        && index.served == *served
    {
        return Ok(());
    }
    let Config(namespaces) = serde_yaml::from_value(config.clone()).context("extensions: code")?;
    let mut built = Vec::new();
    for (name, namespace) in namespaces {
        built.push(
            Namespace::new(&name, namespace, base, served)
                .with_context(|| format!("extensions: code: {name}"))?,
        );
    }
    let index = Index {
        base: base.to_path_buf(),
        served: served.clone(),
        configuration,
        namespaces: built,
        prose: RwLock::default(),
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
    fn new(
        name: &str,
        config: NamespaceConfig,
        base: &Path,
        served: &ServedFolder,
    ) -> Result<Self> {
        let language = config.language.as_deref().unwrap_or(name);
        let query_file = match &config.query {
            Some(query) => match served.file(&base.join(query)) {
                Some(file) => Some(file),
                None => bail!("query {query} does not exist"),
            },
            None => None,
        };
        let grammar = grammar(language, query_file.as_deref())?;
        if config.files.is_empty() {
            bail!("files lists no patterns");
        }
        let mut patterns = GlobSetBuilder::new();
        let mut roots = Vec::new();
        for pattern in &config.files {
            let absolute = lexical(&base.join(pattern));
            if !served.contains(&absolute) {
                bail!("files {pattern} is outside the served folder");
            }
            let text = slashed(&absolute);
            patterns.add(
                globset::GlobBuilder::new(&text)
                    .literal_separator(true)
                    .build()
                    .with_context(|| format!("files {pattern}"))?,
            );
            roots.push(literal_prefix(&absolute));
        }
        Ok(Self {
            name: name.to_string(),
            language: language.to_string(),
            grammar: RwLock::new(Arc::new(grammar)),
            patterns: patterns.build()?,
            roots,
            query: query_file,
            private: config.private,
            files: Mutex::default(),
            targets: RwLock::default(),
        })
    }

    /// Whether `file` is one of the namespace's files.
    fn includes(&self, file: &Path) -> bool {
        self.patterns.is_match(slashed(file))
    }

    /// Every file of the namespace parsed, across the machine's cores, then
    /// linked.
    fn parse_all(&self) {
        let mut found = Vec::new();
        for root in &self.roots {
            let walk = ignore::WalkBuilder::new(root).hidden(true).build();
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
        let parsed: Vec<(PathBuf, Arc<Extracted>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = found
                .chunks(chunk)
                .map(|files| {
                    scope.spawn(move || {
                        files
                            .iter()
                            .filter_map(|file| {
                                let source = std::fs::read_to_string(file).ok()?;
                                Some((file.clone(), Arc::new(grammar.extract(&source))))
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

    /// Every file's declarations linked into targets.
    fn link(&self) {
        let files = self.files.lock().unwrap().clone();
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
        let targets = rust::link(&units, self.private);
        *self.targets.write().unwrap() = Arc::new(targets);
    }

    /// The targets now.
    fn targets(&self) -> Arc<Targets> {
        self.targets.read().unwrap().clone()
    }
}

/// The parser for `language`: its built-in grammar with its built-in query,
/// or the query in `query`.
fn grammar(language: &str, query: Option<&Path>) -> Result<Grammar> {
    let (grammar, built_in, docs) = match language {
        "rust" => (
            tree_sitter_rust::LANGUAGE.into(),
            include_str!("rust.scm"),
            extract::Docs::Rust,
        ),
        other => bail!("language {other} is not one devpage knows: rust"),
    };
    let source = match query {
        Some(file) => {
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?
        }
        None => built_in.to_string(),
    };
    Grammar::new(grammar, &source, docs).map_err(|error| match query {
        Some(file) => error.context(format!("query {}", file.display())),
        None => error,
    })
}

/// The part of an absolute pattern before its first glob character: the
/// folder its files are under.
fn literal_prefix(pattern: &Path) -> PathBuf {
    let mut prefix = PathBuf::new();
    for part in pattern.components() {
        let text = part.as_os_str().to_string_lossy();
        if text.contains(['*', '?', '[', '{']) {
            break;
        }
        prefix.push(part);
    }
    prefix
}

/// What a page address of the code reference names.
pub enum Page {
    /// A namespace's root: every crate it holds.
    Namespace(usize),
    /// A target, by its path.
    Target(usize, Vec<String>),
}

impl Index {
    /// The folder the reference's pages are under.
    fn reference(&self) -> PathBuf {
        self.base.join("reference")
    }

    /// The page at `path` — a lexical path in the served folder — if it is
    /// one of the reference's. An alias names the target it stands for.
    pub fn page(&self, path: &Path) -> Option<Page> {
        let relative = path.strip_prefix(self.reference()).ok()?;
        let mut parts = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned());
        let name = parts.next()?;
        let at = self.namespaces.iter().position(|n| n.name == name)?;
        let segments: Vec<String> = parts.collect();
        if segments.is_empty() {
            return Some(Page::Namespace(at));
        }
        let targets = self.namespaces[at].targets();
        let segments = targets.aliases.get(&segments).cloned().unwrap_or(segments);
        targets
            .targets
            .contains_key(&segments)
            .then_some(Page::Target(at, segments))
    }

    /// The Markdown of the page `page`.
    pub fn markdown(&self, page: &Page) -> String {
        let prose = self.prose.read().unwrap();
        match page {
            Page::Namespace(at) => {
                let namespace = &self.namespaces[*at];
                render::namespace(self, &namespace.name, &namespace.targets())
            }
            Page::Target(at, path) => {
                let namespace = &self.namespaces[*at];
                let targets = namespace.targets();
                let id = format!("{}:{}", namespace.name, path.join("::"));
                render::target(
                    self,
                    &namespace.name,
                    &targets,
                    &targets.targets[path],
                    prose.get(&id),
                )
            }
        }
    }

    /// The address of a target's page, or a member's section on it.
    pub(crate) fn address(&self, namespace: &str, path: &[String], member: Option<&str>) -> String {
        let mut file = self.reference().join(namespace);
        for segment in path {
            file.push(segment);
        }
        let mut address = self.served.url(&file).unwrap_or_default();
        if let Some(member) = member {
            address.push('#');
            address.push_str(&render::member_id(member));
        }
        address
    }

    /// A source file's address and its name, for a target's "declared in".
    pub(crate) fn source(&self, file: &Path) -> Option<(String, String)> {
        Some((self.served.url(file)?, self.served.name(file)?))
    }

    /// The ids `path`'s page holds, for a link's `#fragment`; `None` when
    /// `path` is no page of the reference.
    pub fn ids(&self, path: &Path) -> Option<Vec<String>> {
        Some(match self.page(path)? {
            Page::Namespace(_) => Vec::new(),
            Page::Target(at, segments) => self.namespaces[at].targets().targets[&segments]
                .members
                .iter()
                .map(|member| render::member_id(&member.name))
                .collect(),
        })
    }

    /// Whether `path` lies where the reference's pages are.
    pub fn owns(&self, path: &Path) -> bool {
        path.starts_with(self.reference())
    }

    /// The page nearest to `path`, which names none: the target of the same
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
        let targets = namespace.targets();
        let joined = wanted.join("::");
        let best = targets.targets.keys().min_by_key(|path| {
            (
                path.last() != Some(last),
                distance(&path.join("::"), &joined),
            )
        })?;
        Some(self.address(&namespace.name, best, None))
    }

    /// Every changed file of a namespace parsed again and the namespace
    /// linked again; prose read again when a Markdown file changed.
    fn changed(&self, files: &[PathBuf]) -> bool {
        let mut any = false;
        for namespace in &self.namespaces {
            // A new query reads every file anew; one that doesn't compile
            // keeps the last that did.
            if namespace
                .query
                .as_ref()
                .is_some_and(|query| files.iter().any(|f| f == query))
                && let Ok(grammar) = grammar(&namespace.language, namespace.query.as_deref())
            {
                *namespace.grammar.write().unwrap() = Arc::new(grammar);
                namespace.parse_all();
                any = true;
                continue;
            }
            let mut touched = false;
            for file in files {
                let rs = file.extension().is_some_and(|ext| ext == "rs");
                if !rs || !namespace.includes(file) {
                    continue;
                }
                let mut parsed = namespace.files.lock().unwrap();
                match std::fs::read_to_string(file) {
                    Ok(source) => {
                        let grammar = namespace.grammar.read().unwrap().clone();
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
        let mut found = BTreeMap::new();
        let walk = ignore::WalkBuilder::new(&self.base).hidden(true).build();
        for entry in walk.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "md") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            let Some((id, body)) = prose_of(&source) else {
                continue;
            };
            found.insert(
                id,
                Prose {
                    file: path.to_path_buf(),
                    body,
                },
            );
        }
        let mut prose = self.prose.write().unwrap();
        let same = prose.len() == found.len()
            && prose
                .iter()
                .zip(&found)
                .all(|((a, x), (b, y))| a == b && x.body == y.body && x.file == y.file);
        *prose = found;
        !same
    }

    /// Every prose file whose id names no target: `(file, id)`.
    pub fn orphans(&self) -> Vec<(PathBuf, String)> {
        let prose = self.prose.read().unwrap();
        prose
            .iter()
            .filter(|(id, _)| {
                let Some((name, path)) = id.split_once(':') else {
                    return true;
                };
                let path: Vec<String> = path.split("::").map(str::to_string).collect();
                !self
                    .namespaces
                    .iter()
                    .any(|n| n.name == name && n.targets().targets.contains_key(&path))
            })
            .map(|(id, prose)| (prose.file.clone(), id.clone()))
            .collect()
    }
}

/// A prose file's id and what it adds: the file's first line is
/// ``# ``<id>`` ``; the rest is the body.
fn prose_of(source: &str) -> Option<(String, String)> {
    let mut lines = source.lines();
    let first = lines.find(|line| !line.trim().is_empty())?;
    let id = first
        .trim()
        .strip_prefix("# ``")?
        .strip_suffix("``")?
        .trim()
        .to_string();
    id.contains(':')
        .then(|| (id, lines.collect::<Vec<_>>().join("\n")))
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
