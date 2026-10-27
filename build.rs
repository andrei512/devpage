//! Builds the asset manifest and devpage's build id, into `OUT_DIR`.
//!
//! Every built-in asset gets a content address — `devpage.<hash>.js` — from
//! the SHA-256 of its bytes. An asset that names another (`design.js` imports
//! `design-engine.js`) is hashed after that name is rewritten to the other's
//! content address, so the assets are listed leaves first. Each is also
//! compressed once, as brotli and as gzip, for the server to send whichever
//! the browser accepts. `src/addresses.rs` includes the result. Files left in
//! `OUT_DIR` from an asset's earlier versions are removed.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The built-in assets — name, source, content type — leaves first: an
/// asset's references to the assets before it are rewritten before it is
/// hashed, and a reference to one after it fails the build.
const ASSETS: &[(&str, &str, &str)] = &[
    (
        "design-engine.wasm",
        "assets/vendor/design-engine.wasm",
        "application/wasm",
    ),
    (
        "design-engine.js",
        "assets/vendor/design-engine.js",
        "text/javascript",
    ),
    ("design.js", "assets/design.js", "text/javascript"),
    ("react.js", "assets/vendor/react.js", "text/javascript"),
    ("style.css", "assets/style.css", "text/css"),
    ("devpage.js", "assets/devpage.js", "text/javascript"),
];

/// What devpage's build is made from: a change to any of it is a new build id.
/// Paths that don't exist (the design crate's, in a published package) are
/// skipped.
const BUILD_INPUTS: &[&str] = &[
    "build.rs",
    "Cargo.toml",
    "Cargo.lock",
    "src",
    "assets",
    "design/Cargo.toml",
    "design/src",
    "design/proto",
    "design/fonts",
];

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for input in BUILD_INPUTS {
        println!("cargo::rerun-if-changed={input}");
    }

    // Each asset, rewritten and hashed: name, content type, hash, content
    // address and bytes.
    let mut hashed_assets: Vec<(&str, &str, String, String, Vec<u8>)> = Vec::new();
    let mut table = String::new();
    // Every file this build writes; any other in `OUT_DIR` is left from an
    // earlier version of an asset, and is removed.
    let mut written: Vec<PathBuf> = vec![out.join("manifest.rs")];
    for (name, source, mime) in ASSETS {
        let mut bytes = std::fs::read(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        if !name.ends_with(".wasm") {
            for (earlier, _, _, hashed, _) in &hashed_assets {
                bytes = replace(
                    &bytes,
                    format!("\"/__devpage/{earlier}\"").as_bytes(),
                    format!("\"/__devpage/{hashed}\"").as_bytes(),
                );
            }
            for (other, _, _) in ASSETS {
                let bare = format!("/__devpage/{other}");
                assert!(
                    find(&bytes, bare.as_bytes()).is_none(),
                    "{source} names {bare} where build.rs can't give it its content address: \
                     write it in double quotes, and list {other} before {name} in ASSETS"
                );
            }
        }
        let hash = hex(&Sha256::digest(&bytes))[..16].to_string();
        let (stem, ext) = name
            .split_once('.')
            .expect("an asset name has an extension");
        let hashed = format!("{stem}.{hash}.{ext}");
        hashed_assets.push((name, mime, hash, hashed, bytes));
    }

    // Compressing is most of the build script's time, brotli at its best
    // most of all: every asset's two variants at once, each on a thread.
    std::thread::scope(|scope| {
        for (_, _, _, hashed, bytes) in &hashed_assets {
            write_if_new(&out.join(hashed), bytes, |b| b.to_vec());
            let brotli = out.join(format!("{hashed}.br"));
            scope.spawn(move || write_if_new(&brotli, bytes, compress_brotli));
            let gzip = out.join(format!("{hashed}.gz"));
            scope.spawn(move || write_if_new(&gzip, bytes, compress_gzip));
        }
    });

    for (name, mime, hash, hashed, bytes) in &hashed_assets {
        let identity = out.join(hashed);
        let brotli = out.join(format!("{hashed}.br"));
        let gzip = out.join(format!("{hashed}.gz"));
        written.extend([identity.clone(), brotli.clone(), gzip.clone()]);
        // A variant no smaller than the original isn't worth sending.
        let variant = |file: &Path| {
            let len = std::fs::metadata(file).map(|m| m.len()).unwrap_or(u64::MAX);
            if len < bytes.len() as u64 {
                format!("Some(include_bytes!({:?}))", file.display().to_string())
            } else {
                "None".to_string()
            }
        };
        writeln!(
            table,
            "    Asset {{ name: {name:?}, hashed: {hashed:?}, hash: {hash:?}, mime: {mime:?}, \
             identity: include_bytes!({:?}), brotli: {}, gzip: {} }},",
            identity.display().to_string(),
            variant(&brotli),
            variant(&gzip),
        )
        .unwrap();
    }

    let mut build = Sha256::new();
    build.update(std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    for input in BUILD_INPUTS {
        let mut files = Vec::new();
        walk(Path::new(input), &mut files);
        files.sort();
        for file in files {
            build.update(file.to_string_lossy().as_bytes());
            build.update(std::fs::read(&file).unwrap_or_default());
        }
    }
    let build = hex(&build.finalize())[..16].to_string();

    let manifest = format!(
        "/// devpage's build id: a hash of everything the binary is built from.\n\
         pub const BUILD: &str = {build:?};\n\n\
         /// The built-in assets, each under its content address.\n\
         pub static ASSETS: &[Asset] = &[\n{table}];\n"
    );
    std::fs::write(out.join("manifest.rs"), manifest).expect("writing the manifest");
    prune(&out, &written);
}

/// Removes every file in `out` that isn't in `written`: the versions of
/// assets this build no longer names, which would otherwise pile up in
/// `OUT_DIR` with every edit.
fn prune(out: &Path, written: &[PathBuf]) {
    for entry in std::fs::read_dir(out).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_file() && !written.contains(&path) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Writes `make(bytes)` to `file` unless it is there already: a file is named
/// by its content's hash, so it is made once, however often devpage builds.
fn write_if_new(file: &Path, bytes: &[u8], make: impl Fn(&[u8]) -> Vec<u8>) {
    if file.exists() {
        return;
    }
    // Beside the file under its whole name: the brotli and gzip variants of
    // one asset are written at once.
    let mut partial = file.as_os_str().to_owned();
    partial.push(".partial");
    std::fs::write(&partial, make(bytes)).expect("writing an asset");
    std::fs::rename(&partial, file).expect("writing an asset");
}

fn compress_brotli(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: 11,
        lgwin: 24,
        size_hint: bytes.len(),
        ..Default::default()
    };
    brotli::BrotliCompress(&mut &bytes[..], &mut out, &params).expect("brotli");
    out
}

fn compress_gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("gzip");
    encoder.finish().expect("gzip")
}

fn walk(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_path_buf());
    } else if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            walk(&entry.path(), files);
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = find(rest, from) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(to);
        rest = &rest[at + from.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
