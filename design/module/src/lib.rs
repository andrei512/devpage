//! The vendored WebAssembly module: what it is built from, and the fingerprint
//! that says whether `core/assets/vendor/` still holds a module built from the
//! sources as they are now.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The command that rebuilds the vendored module; the staleness test names it.
pub const COMMAND: &str = "cargo run -p devpage-design-module";

/// The vendored files, from the top of the repository.
pub const MODULE: &str = "core/assets/vendor/design-engine.wasm";
pub const BINDINGS: &str = "core/assets/vendor/design-engine.js";
pub const FINGERPRINT: &str = "core/assets/vendor/design-engine.fingerprint";

/// Folders whose every file goes into the module, or decides how it is built.
const FOLDERS: &[&str] = &[
    "design/src",
    "design/fonts",
    "design/proto",
    "design/wasm/src",
    "design/module/src",
];

/// Single files that do.
const FILES: &[&str] = &[
    "design/build.rs",
    "design/Cargo.toml",
    "design/wasm/Cargo.toml",
    "design/module/Cargo.toml",
];

/// The locked packages whose versions change the module's bytes: the
/// module's whole dependency tree, and the bindings generator's.
const ROOTS: &[&str] = &["devpage-design-wasm", "wasm-bindgen-cli-support"];

/// The top of the repository: two folders above this crate.
pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository's top folder")
}

/// A hash of everything the vendored module is built from: the engine's and
/// the module's sources, the fonts, the proto, the manifests, this builder,
/// the `wasm` profile and the locked version of every package in the module's
/// dependency tree. The toolchain is not in it — it is recorded beside it —
/// so the same sources give the same fingerprint on every machine.
pub fn fingerprint(root: &Path) -> String {
    let mut hash = Sha256::new();
    let mut add = |label: &str, bytes: &[u8]| {
        hash.update(label.as_bytes());
        hash.update([0]);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    let mut paths = BTreeSet::new();
    for folder in FOLDERS {
        files_under(&root.join(folder), &mut paths);
    }
    for file in FILES {
        paths.insert(root.join(file));
    }
    for path in &paths {
        let name = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("reading {name}: {e}"));
        add(&name.replace('\\', "/"), &bytes);
    }
    add("profile.wasm", wasm_profile(root).as_bytes());
    add("Cargo.lock", locked_tree(root).as_bytes());
    let digest = hash.finalize();
    digest.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

/// The fingerprint `core/assets/vendor/` records, if it records one.
pub fn recorded(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join(FINGERPRINT)).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("sources "))
        .map(|hex| hex.trim().to_string())
}

/// The fingerprint file's contents.
pub fn record(sources: &str, rustc: &str) -> String {
    format!(
        "# The design engine's WebAssembly module and bindings beside this file were\n\
         # built from the sources with this fingerprint. Rebuild them with\n\
         # `{COMMAND}`; a test fails when the sources no longer match.\n\
         sources {sources}\n\
         rustc {rustc}\n"
    )
}

fn files_under(folder: &Path, out: &mut BTreeSet<PathBuf>) {
    let entries =
        fs::read_dir(folder).unwrap_or_else(|e| panic!("reading {}: {e}", folder.display()));
    for entry in entries {
        let path = entry.expect("a folder entry").path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.insert(path);
        }
    }
}

/// The root manifest's `[profile.wasm]`, as TOML.
fn wasm_profile(root: &Path) -> String {
    let manifest: toml::Table = fs::read_to_string(root.join("Cargo.toml"))
        .expect("reading Cargo.toml")
        .parse()
        .expect("parsing Cargo.toml");
    let profile = manifest
        .get("profile")
        .and_then(|p| p.get("wasm"))
        .expect("Cargo.toml has a [profile.wasm]");
    toml::to_string(profile).expect("writing the wasm profile")
}

/// Every locked package reachable from [`ROOTS`], one `name version source
/// checksum` line each, sorted.
fn locked_tree(root: &Path) -> String {
    let lock: toml::Table = fs::read_to_string(root.join("Cargo.lock"))
        .expect("reading Cargo.lock")
        .parse()
        .expect("parsing Cargo.lock");
    let packages = lock
        .get("package")
        .and_then(|p| p.as_array())
        .expect("Cargo.lock lists packages");
    let field = |p: &toml::Value, key: &str| {
        p.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut by_name: HashMap<String, Vec<&toml::Value>> = HashMap::new();
    for package in packages {
        by_name
            .entry(field(package, "name"))
            .or_default()
            .push(package);
    }
    let mut seen = BTreeSet::new();
    let mut lines = BTreeSet::new();
    let mut queue: Vec<String> = ROOTS.iter().map(|r| r.to_string()).collect();
    while let Some(wanted) = queue.pop() {
        if !seen.insert(wanted.clone()) {
            continue;
        }
        // A dependency is `name`, `name version` or `name version (source)`.
        let mut parts = wanted.split(' ');
        let name = parts.next().unwrap_or_default();
        let version = parts.next();
        let candidates = by_name
            .get(name)
            .unwrap_or_else(|| panic!("Cargo.lock has no package {name}"));
        let package = candidates
            .iter()
            .find(|p| version.is_none_or(|v| field(p, "version") == v))
            .unwrap_or_else(|| panic!("Cargo.lock has no package {wanted}"));
        lines.insert(format!(
            "{} {} {} {}",
            name,
            field(package, "version"),
            field(package, "source"),
            field(package, "checksum")
        ));
        if let Some(deps) = package.get("dependencies").and_then(|d| d.as_array()) {
            queue.extend(deps.iter().filter_map(|d| d.as_str()).map(String::from));
        }
    }
    lines.into_iter().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_module_is_built_from_the_sources_as_they_are() {
        let root = repository();
        let now = fingerprint(&root);
        assert_eq!(
            recorded(&root).as_deref(),
            Some(now.as_str()),
            "the vendored design engine in core/assets/vendor/ was built from other sources \
             than these — rebuild it with `{COMMAND}`"
        );
        for file in [MODULE, BINDINGS] {
            assert!(
                root.join(file).is_file(),
                "{file} is missing — run `{COMMAND}`"
            );
        }
    }

    #[test]
    fn a_change_to_what_the_module_is_built_from_changes_the_fingerprint() {
        let root = repository();
        let copy = std::env::temp_dir().join(format!("devpage-module-{}", std::process::id()));
        let _ = fs::remove_dir_all(&copy);
        for file in ["Cargo.toml", "Cargo.lock"].iter().chain(FILES) {
            let to = copy.join(file);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::copy(root.join(file), &to).unwrap();
        }
        for folder in FOLDERS {
            let mut paths = BTreeSet::new();
            files_under(&root.join(folder), &mut paths);
            for path in paths {
                let to = copy.join(path.strip_prefix(&root).unwrap());
                fs::create_dir_all(to.parent().unwrap()).unwrap();
                fs::copy(&path, &to).unwrap();
            }
        }
        let before = fingerprint(&copy);
        assert_eq!(before, fingerprint(&root), "a copy fingerprints the same");

        let source = copy.join("design/src/render.rs");
        let mut text = fs::read_to_string(&source).unwrap();
        text.push_str("\n// one more line\n");
        fs::write(&source, &text).unwrap();
        assert_ne!(fingerprint(&copy), before, "an engine source");
        text.truncate(text.len() - "\n// one more line\n".len());
        fs::write(&source, &text).unwrap();
        assert_eq!(fingerprint(&copy), before);

        let manifest = copy.join("Cargo.toml");
        let original = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            original.replace("opt-level = \"s\"", "opt-level = \"z\""),
        )
        .unwrap();
        assert_ne!(fingerprint(&copy), before, "the wasm profile");
        fs::write(&manifest, &original).unwrap();

        let lock = copy.join("Cargo.lock");
        let original = fs::read_to_string(&lock).unwrap();
        let bumped = original.replacen(
            "name = \"vello\"\nversion = \"",
            "name = \"vello\"\nversion = \"9",
            1,
        );
        assert_ne!(bumped, original, "Cargo.lock locks vello");
        fs::write(&lock, bumped).unwrap();
        assert_ne!(
            fingerprint(&copy),
            before,
            "a locked version in the module's tree"
        );
        fs::write(&lock, &original).unwrap();

        // Something the module isn't built from changes nothing.
        let unrelated = original.replacen(
            "name = \"comrak\"\nversion = \"",
            "name = \"comrak\"\nversion = \"9",
            1,
        );
        assert_ne!(unrelated, original, "Cargo.lock locks comrak");
        fs::write(&lock, unrelated).unwrap();
        assert_eq!(
            fingerprint(&copy),
            before,
            "a package outside the module's tree"
        );
        fs::remove_dir_all(&copy).ok();
    }
}
