//! What the real `devpage` exits with and prints, as an agent reads it: 0
//! done, 1 the document has problems, 2 devpage couldn't run; with `--json`,
//! the problems as JSON on standard output and an error as JSON on standard
//! error.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A folder holding `files`.
fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("devpage-exit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    dir
}

/// `devpage <args>` run in `dir`.
fn devpage(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_devpage"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(bytes)))
}

#[test]
fn a_clean_document_exits_0_and_a_broken_one_1() {
    let dir = folder(
        "check",
        &[
            ("good.md", "# Good\n\n[Bad](bad.md)\n"),
            ("bad.md", "# Bad\n\n[Gone](gone.md)\n"),
        ],
    );
    let reaching = devpage(&dir, &["check", "good.md"]);
    assert_eq!(reaching.status.code(), Some(1), "good.md reaches bad.md");
    let alone = folder("check-alone", &[("ok.md", "# Ok\n")]);
    let ok = devpage(&alone, &["check", "ok.md", "--json"]);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(json(&ok.stdout), serde_json::json!({ "problems": [] }));

    let broken = devpage(&dir, &["check", "bad.md", "--json"]);
    assert_eq!(broken.status.code(), Some(1));
    assert_eq!(
        json(&broken.stdout),
        serde_json::json!({ "problems": [{
            "file": "bad.md",
            "line": 3,
            "message": "broken link gone.md — no such file",
            "target": "gone.md",
        }] })
    );
}

#[test]
fn a_design_names_its_line_and_column() {
    let dir = folder(
        "design",
        &[(
            "bad.design",
            "<Frame width={40} height={30}>\n  <Nope />\n</Frame>\n",
        )],
    );
    let output = devpage(&dir, &["png", "bad.design", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let problems = json(&output.stdout);
    let problem = &problems["problems"][0];
    assert_eq!(
        (&problem["file"], &problem["line"], &problem["column"]),
        (&"bad.design".into(), &2.into(), &3.into())
    );
    assert!(!dir.join("bad.png").exists());
}

#[test]
fn what_cannot_run_exits_2_and_says_so_in_json_when_asked() {
    let dir = folder("fail", &[]);
    let output = devpage(&dir, &["check", "missing.md", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = json(&output.stderr);
    assert_eq!(error["error"], "reading missing.md");
    // The system's own words for a missing file, which differ by system.
    assert_eq!(error["causes"].as_array().unwrap().len(), 1);
    let plain = devpage(&dir, &["check", "missing.md"]);
    assert_eq!(plain.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&plain.stderr).starts_with("Error: reading missing.md"));
    let unreadable = devpage(&dir, &["check", "--bogus"]);
    assert_eq!(unreadable.status.code(), Some(2));
}
