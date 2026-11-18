//! Tells the build which platform it is for, and embeds the live session's
//! program: the files of `live/` that `devpage setup` unpacks.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!(
        "cargo:rustc-env=NATIVE_EXTENSION_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    let live = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("live");
    println!("cargo:rerun-if-changed=live");
    let mut files = Vec::new();
    collect(&live, "", &mut files);
    files.sort();
    let mut code = String::from(
        "/// Each file of the live session's program: its path in the program's\n\
         /// folder, with `/` between names, and its bytes.\n\
         const PROGRAM: &[(&str, &[u8])] = &[\n",
    );
    for (name, file) in &files {
        writeln!(code, "    ({name:?}, include_bytes!({file:?})),").unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("live_program.rs"), code).unwrap();
}

/// Adds every file under `folder` the running session needs to `files`, as
/// its path under the program's folder — `folder`'s own is `under` — and
/// where it lies.
fn collect(folder: &Path, under: &str, files: &mut Vec<(String, PathBuf)>) {
    let entries = std::fs::read_dir(folder)
        .unwrap_or_else(|error| panic!("reading {}: {error}", folder.display()));
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !needed(&name) {
            continue;
        }
        let path = format!("{under}{name}");
        if entry.file_type().unwrap().is_dir() {
            collect(&entry.path(), &format!("{path}/"), files);
        } else {
            files.push((path, entry.path()));
        }
    }
}

/// Whether the running session needs the file or folder `name`: not the
/// installed packages, the evals and their runner, a test, the smoke test or
/// a hidden file.
fn needed(name: &str) -> bool {
    !(name.starts_with('.')
        || name == "node_modules"
        || name == "evals"
        || name == "eval.ts"
        || name == "smoke.ts"
        || name.contains(".test."))
}
