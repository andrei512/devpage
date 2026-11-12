//! A site's requirements, checked against its tests: the `requirements`
//! system extension. A page of type `constraint` is a requirement a test can
//! check; a test points at it by its id — the page's file name without `.md`
//! — which the site's own pattern reads out of its test files. A constraint
//! no test points at is a problem, and so is a test pointing at no
//! constraint. Each suite's `run` command prints its results as JUnit XML,
//! which [`junit`] reads; the command itself is run by the binary.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::GlobSet;
use regex::Regex;
use serde::Deserialize;

use crate::document::Document;
use crate::folder::{ServedFolder, lexical};
use crate::problem::Problem;

/// The page type whose pages are constraints.
const CONSTRAINT: &str = "constraint";

/// `extensions: requirements: config:` as a site writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// One entry per language or test framework.
    tests: Vec<SuiteConfig>,
}

/// One suite of tests as written: where its files are, how a test names a
/// constraint, and the command that runs it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteConfig {
    /// What the suite is called in problems.
    name: String,
    /// A glob of its test files, relative to the document file.
    glob: String,
    /// A regular expression whose first group is a constraint's id.
    pattern: String,
    /// The command that runs it and prints JUnit XML on standard output.
    run: String,
}

/// A site's requirements as configured: its test suites.
#[derive(Debug)]
pub struct Requirements {
    /// The suites, in the order written.
    pub suites: Vec<Suite>,
}

/// One suite of tests, ready to be searched.
#[derive(Debug)]
pub struct Suite {
    /// What the suite is called in problems.
    pub name: String,
    /// The command that runs it and prints JUnit XML on standard output.
    pub run: String,
    /// The folder its glob's files are under.
    root: PathBuf,
    /// Its glob, matched against absolute paths.
    files: GlobSet,
    /// How a test names a constraint: the first group is the id.
    pattern: Regex,
}

/// A test pointing at a constraint: the id it names and where it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pointer {
    /// The constraint id as the test file writes it.
    pub id: String,
    /// The test file.
    pub file: PathBuf,
    /// The line the id is on, counted from 1.
    pub line: usize,
}

/// One test of a JUnit report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Its name, with its class before it when the report gives one.
    pub name: String,
    /// Whether it failed or stopped on an error.
    pub failed: bool,
}

impl Requirements {
    /// Reads `config` — what a site writes under `extensions: requirements:
    /// config:` — for the document file in `base`; an error names the field
    /// that is wrong.
    pub fn read(config: &serde_yaml::Value, base: &Path, served: &ServedFolder) -> Result<Self> {
        let config: Config = serde_yaml::from_value(config.clone())?;
        if config.tests.is_empty() {
            bail!("tests lists no suite");
        }
        let mut suites = Vec::new();
        for suite in config.tests {
            let name = &suite.name;
            let absolute = lexical(&base.join(&suite.glob));
            if !served.contains(&absolute) {
                bail!(
                    "tests {name}: glob {} is outside the served folder",
                    suite.glob
                );
            }
            let root = crate::code::literal_prefix(&absolute);
            let rest = absolute.strip_prefix(&root).unwrap_or(&absolute);
            let text = if rest.as_os_str().is_empty() {
                globset::escape(&crate::code::glob_path(&root))
            } else {
                format!(
                    "{}/{}",
                    globset::escape(crate::code::glob_path(&root).trim_end_matches('/')),
                    crate::code::glob_path(rest)
                )
            };
            let glob = globset::GlobBuilder::new(&text)
                .literal_separator(true)
                .build()
                .with_context(|| format!("tests {name}: glob {}", suite.glob))?;
            let pattern = Regex::new(&suite.pattern)
                .with_context(|| format!("tests {name}: pattern {}", suite.pattern))?;
            if pattern.captures_len() < 2 {
                bail!(
                    "tests {name}: pattern {} has no group — its first group is the constraint's id",
                    suite.pattern
                );
            }
            if suite.run.trim().is_empty() {
                bail!("tests {name}: run is empty");
            }
            suites.push(Suite {
                name: suite.name,
                run: suite.run,
                root,
                files: GlobSet::builder().add(glob).build()?,
                pattern,
            });
        }
        Ok(Self { suites })
    }

    /// Every place a test file of any suite names a constraint, in path order.
    pub fn pointers(&self) -> Vec<Pointer> {
        let mut found = Vec::new();
        for suite in &self.suites {
            let mut files = Vec::new();
            let walk = ignore::WalkBuilder::new(&suite.root)
                .hidden(true)
                .require_git(false)
                .build();
            for entry in walk.flatten() {
                let path = entry.path();
                if entry.file_type().is_some_and(|t| t.is_file())
                    && suite.files.is_match(crate::code::glob_path(path))
                {
                    files.push(path.to_path_buf());
                }
            }
            files.sort();
            for file in files {
                let Ok(source) = std::fs::read_to_string(&file) else {
                    continue;
                };
                for captures in suite.pattern.captures_iter(&source) {
                    let Some(id) = captures.get(1) else { continue };
                    found.push(Pointer {
                        id: id.as_str().to_string(),
                        file: file.clone(),
                        line: source[..id.start()].matches('\n').count() + 1,
                    });
                }
            }
        }
        found
    }
}

/// A constraint page's id: its file name without `.md`, or its folder's name
/// for a `README.md`.
fn id(page: &Path) -> String {
    let stem = page.file_stem().unwrap_or_default().to_string_lossy();
    if stem.eq_ignore_ascii_case("README") {
        page.parent()
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        stem.into_owned()
    }
}

/// The site's constraint pages by id: every Markdown file in its folder whose
/// `page-type` is `constraint`. An id two pages share keeps both.
fn constraints(document: &Document) -> BTreeMap<String, Vec<PathBuf>> {
    let mut found: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let walk = ignore::WalkBuilder::new(&document.base)
        .hidden(true)
        .require_git(false)
        .build();
    for entry in walk.flatten() {
        let path = entry.path();
        if !entry.file_type().is_some_and(|t| t.is_file())
            || path.extension().is_none_or(|ext| ext != "md")
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if crate::structure::page_type(&source).is_some_and(|(name, _)| name == CONSTRAINT) {
            found.entry(id(path)).or_default().push(path.to_path_buf());
        }
    }
    for pages in found.values_mut() {
        pages.sort();
    }
    found
}

/// What is wrong between a site's constraints and its tests, without running
/// one: a constraint no test points at, a test pointing at no constraint, and
/// two constraints with one id. Nothing for a document with no requirements.
pub fn problems(document: &Document) -> Vec<Problem> {
    let Some(requirements) = &document.requirements else {
        return Vec::new();
    };
    let problem = |file: &Path, line: Option<usize>, message: String| {
        document.name(file).map(|file| Problem {
            file,
            line,
            column: None,
            message,
            target: None,
        })
    };
    let constraints = constraints(document);
    let pointers = requirements.pointers();
    let mut problems = Vec::new();
    for (id, pages) in &constraints {
        for page in &pages[1..] {
            problems.extend(problem(
                page,
                None,
                format!(
                    "constraint id {id} is taken by {}",
                    document.name(&pages[0]).unwrap_or_default()
                ),
            ));
        }
        if !pointers.iter().any(|pointer| pointer.id == *id) {
            problems.extend(problem(
                &pages[0],
                None,
                format!("no test points at this constraint — a test names it {id}"),
            ));
        }
    }
    for pointer in &pointers {
        if !constraints.contains_key(&pointer.id) {
            problems.extend(problem(
                &pointer.file,
                Some(pointer.line),
                format!("{} — no constraint has this id", pointer.id),
            ));
        }
    }
    problems
}

/// The tests of a JUnit report: every `testcase`, failed when it holds a
/// `failure` or an `error`. `text` may hold several reports one after another,
/// as a command running several test binaries prints them; what is not XML
/// ends the reading.
pub fn junit(text: &str) -> Vec<Case> {
    use quick_xml::events::{BytesStart, Event};
    fn case(element: &BytesStart) -> Case {
        let mut name = String::new();
        let mut class = String::new();
        for attribute in element.attributes().flatten() {
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|value| value.into_owned())
                .unwrap_or_default();
            match attribute.key.as_ref() {
                "name" => name = value,
                "classname" => class = value,
                _ => {}
            }
        }
        Case {
            name: if class.is_empty() {
                name
            } else {
                format!("{class}::{name}")
            },
            failed: false,
        }
    }
    let mut reader = quick_xml::Reader::from_str(text);
    let mut cases = Vec::new();
    let mut open: Option<Case> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Empty(element)) => match element.name().as_ref() {
                "testcase" => cases.push(case(&element)),
                "failure" | "error" => {
                    if let Some(open) = &mut open {
                        open.failed = true;
                    }
                }
                _ => {}
            },
            Ok(Event::Start(element)) => match element.name().as_ref() {
                "testcase" => open = Some(case(&element)),
                "failure" | "error" => {
                    if let Some(open) = &mut open {
                        open.failed = true;
                    }
                }
                _ => {}
            },
            Ok(Event::End(element)) if element.name().as_ref() == "testcase" => {
                cases.extend(open.take());
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    cases
}

/// What a suite's run shows is wrong, from what its command printed and how
/// it exited: each failed test, or that it printed no report at all. The
/// problems are the document file's.
pub fn run_problems(
    document: &Document,
    suite: &Suite,
    printed: &str,
    exit: Option<i32>,
) -> Vec<Problem> {
    let problem = |message: String| Problem {
        file: document.part_name(&document.path()),
        line: None,
        column: None,
        message,
        target: None,
    };
    let cases = junit(printed);
    if cases.is_empty() {
        let exit = exit.map_or("no exit code".to_string(), |code| {
            format!("exit code {code}")
        });
        return vec![problem(format!(
            "tests {}: `{}` printed no JUnit report ({exit})",
            suite.name, suite.run
        ))];
    }
    cases
        .into_iter()
        .filter(|case| case.failed)
        .map(|case| problem(format!("tests {}: {} failed", suite.name, case.name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A site named `name` whose requirements read `tests/*.sh` for
    /// `# spec: <id>`, holding `files` — each a path in the site and its text.
    fn site(name: &str, files: &[(&str, &str)]) -> Document {
        let dir = std::env::temp_dir().join(format!("devpage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\nextensions:\n  requirements:\n    config:\n      tests:\n        - name: shell\n          glob: \"tests/*.sh\"\n          pattern: '# spec: ([a-z0-9-]+)'\n          run: \"sh tests/run.sh\"\n",
        )
        .unwrap();
        for (file, text) in files {
            let file = dir.join(file);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap()
    }

    fn found(document: &Document) -> Vec<String> {
        let mut found: Vec<String> = problems(document).iter().map(ToString::to_string).collect();
        found.sort();
        found
    }

    const CONSTRAINT: &str = "---\npage-type: constraint\n---\n# C\n";

    #[test]
    fn a_constraint_every_test_of_which_is_found_has_no_problem() {
        let doc = site(
            "requirements-ok",
            &[
                ("README.md", "# Home\n"),
                ("rules/a-save-is-kept.md", CONSTRAINT),
                (
                    "rules/no-rework.md",
                    "---\npage-type: review-rule\n---\n# R\n",
                ),
                (
                    "tests/save.sh",
                    "# spec: a-save-is-kept\none() { :; }\n# spec: a-save-is-kept\ntwo() { :; }\n",
                ),
            ],
        );
        assert_eq!(found(&doc), Vec::<String>::new());
    }

    #[test]
    fn a_constraint_no_test_points_at_is_a_problem_on_its_page() {
        let doc = site(
            "requirements-untested",
            &[
                ("README.md", "# Home\n"),
                ("rules/a-save-is-kept.md", CONSTRAINT),
                ("tests/save.sh", "one() { :; }\n"),
            ],
        );
        assert_eq!(
            found(&doc),
            [
                "rules/a-save-is-kept.md: no test points at this constraint — a test names it a-save-is-kept"
            ]
        );
    }

    #[test]
    fn a_test_pointing_at_no_constraint_is_a_problem_at_its_line() {
        let doc = site(
            "requirements-orphan",
            &[
                ("README.md", "# Home\n"),
                (
                    "tests/save.sh",
                    "one() { :; }\n# spec: gone-rule\ntwo() { :; }\n",
                ),
            ],
        );
        assert_eq!(
            found(&doc),
            ["tests/save.sh:2: gone-rule — no constraint has this id"]
        );
    }

    #[test]
    fn two_constraints_with_one_id_are_a_problem_on_the_second() {
        let doc = site(
            "requirements-twice",
            &[
                ("README.md", "# Home\n"),
                ("a/kept.md", CONSTRAINT),
                ("b/kept.md", CONSTRAINT),
                ("tests/save.sh", "# spec: kept\none() { :; }\n"),
            ],
        );
        assert_eq!(
            found(&doc),
            ["b/kept.md: constraint id kept is taken by a/kept.md"]
        );
    }

    #[test]
    fn a_site_with_no_requirements_has_none_of_these_problems() {
        let dir =
            std::env::temp_dir().join(format!("devpage-requirements-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("devpage.yml"),
            "kind: site\ntitle: T\nroot: README.md\n",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
        std::fs::write(dir.join("kept.md"), CONSTRAINT).unwrap();
        let doc = Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap();
        assert!(doc.requirements.is_none());
        assert_eq!(found(&doc), Vec::<String>::new());
    }

    #[test]
    fn a_junit_report_gives_each_test_and_whether_it_failed() {
        let cases = junit(
            "<?xml version=\"1.0\"?>\n<testsuites><testsuite name=\"a\">\
             <testcase classname=\"save\" name=\"one\"/>\
             <testcase classname=\"save\" name=\"two\"><failure message=\"x &lt; y\">at line 3</failure></testcase>\
             <testcase name=\"three\"><error/></testcase>\
             </testsuite></testsuites>\n\
             <?xml version=\"1.0\"?>\n<testsuites><testsuite name=\"b\"><testcase name=\"four\"/></testsuite></testsuites>",
        );
        let read: Vec<(&str, bool)> = cases.iter().map(|c| (c.name.as_str(), c.failed)).collect();
        assert_eq!(
            read,
            [
                ("save::one", false),
                ("save::two", true),
                ("three", true),
                ("four", false)
            ]
        );
    }

    #[test]
    fn a_run_reports_each_failed_test_or_that_nothing_was_printed() {
        let doc = site("requirements-run", &[("README.md", "# Home\n")]);
        let suite = &doc.requirements.as_ref().unwrap().suites[0];
        let report = "<testsuite><testcase name=\"one\"/><testcase name=\"two\"><failure/></testcase></testsuite>";
        let shown = |printed: &str, exit| -> Vec<String> {
            run_problems(&doc, suite, printed, exit)
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(
            shown(report, Some(101)),
            ["devpage.yml: tests shell: two failed"]
        );
        assert_eq!(
            shown("error: could not compile", Some(101)),
            ["devpage.yml: tests shell: `sh tests/run.sh` printed no JUnit report (exit code 101)"]
        );
    }

    #[test]
    fn a_wrong_configuration_stops_the_site_loading_and_names_the_field() {
        let dir =
            std::env::temp_dir().join(format!("devpage-requirements-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Home\n").unwrap();
        let load = |tests: &str| {
            std::fs::write(
                dir.join("devpage.yml"),
                format!("kind: site\ntitle: T\nroot: README.md\nextensions:\n  requirements:\n    config:\n      tests:\n{tests}"),
            )
            .unwrap();
            format!(
                "{:#}",
                Document::load(&dir, &ServedFolder::new(&dir).unwrap()).unwrap_err()
            )
        };
        assert!(
            load("        - {name: sh, glob: \"*.sh\", pattern: 'spec', run: \"x\"}\n")
                .contains("tests sh: pattern spec has no group"),
        );
        assert!(
            load("        - {name: sh, glob: \"*.sh\", pattern: '(', run: \"x\"}\n")
                .contains("extensions: requirements: tests sh: pattern ("),
        );
    }
}
