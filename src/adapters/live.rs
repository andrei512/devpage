//! The live session on this machine: its program, which the executable
//! carries and `devpage setup` unpacks into `~/.devpage/live/program`, and
//! the bun that runs it, which `devpage setup` downloads into
//! `~/.devpage/live/bun`. The machine's own bun is never touched.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result, bail};
use devpage_core::extensions::{LIVE, Package};
use sha2::{Digest, Sha256};

/// The variable holding the OpenAI key the session's models run on, unless
/// the Azure endpoint is set.
pub const KEY: &str = "DEVPAGE_OPENAI_API_KEY";
/// The variable naming an Azure endpoint, which has the session's models run
/// on that Azure resource.
pub const AZURE_ENDPOINT: &str = "DEVPAGE_AZURE_OPENAI_ENDPOINT";
/// The variable holding the Azure resource's key, needed when the endpoint is
/// set.
pub const AZURE_KEY: &str = "DEVPAGE_AZURE_OPENAI_API_KEY";

/// The version of bun `devpage setup` downloads. Pinned, with the checksum
/// of each of its builds in [`BUN_BUILDS`], so every machine runs the program
/// with the same bun.
pub(crate) const BUN_VERSION: (u32, u32, u32) = (1, 4, 4);

/// The builds of bun at [`BUN_VERSION`] devpage downloads: each one's name in
/// the release, and the SHA-256 of its zip.
const BUN_BUILDS: &[(&str, &str)] = &[
    (
        "bun-darwin-aarch64",
        "c240d7f0ccb90c263a849fcdb09b7ee62630d921348ed6aafda86b9d577bc510",
    ),
    (
        "bun-darwin-x64",
        "50a3491f73abc2fe81673171a85762d3bc2cc1680ccecadd983de99adf77977e",
    ),
    (
        "bun-darwin-x64-baseline",
        "d4a155adfbfd09155eb3ba05c9e3a544f39297d7a68616d3c7759f67ab5c47dc",
    ),
    (
        "bun-linux-aarch64",
        "baa6bb36f51d6e58561e02bbea825038a9df454f673d6d776e8bbf86f8c7850e",
    ),
    (
        "bun-linux-x64",
        "bf8ccbfb2b0b80159e0f9a6d15c55317add8077534dbc6130a3edb52fccb395c",
    ),
    (
        "bun-linux-x64-baseline",
        "e0446eb55995772eb9894bebae9a6be368d985a8b9271ee124486f9946eedd76",
    ),
];

/// Where bun's releases are downloaded from: `<this>/bun-v<version>/<build>.zip`.
const BUN_RELEASES: &str = "https://github.com/oven-sh/bun/releases/download";

/// The file in the program's folder that says which program is installed:
/// the [`version`] `devpage setup` unpacked, written once its packages are in.
const STAMP: &str = ".version";

include!(concat!(env!("OUT_DIR"), "/live_program.rs"));

/// The version of the program this executable carries: the SHA-256 of its
/// files. Another executable's program has another.
fn version() -> &'static str {
    static VERSION: LazyLock<String> = LazyLock::new(|| {
        let mut hash = Sha256::new();
        for (name, bytes) in PROGRAM {
            hash.update(name.as_bytes());
            hash.update([0]);
            hash.update(bytes.len().to_le_bytes());
            hash.update(bytes);
        }
        format!("{:x}", hash.finalize())
    });
    &VERSION
}

/// [`BUN_VERSION`] as bun writes it: `1.4.4`.
fn bun_version() -> String {
    let (major, minor, patch) = BUN_VERSION;
    format!("{major}.{minor}.{patch}")
}

/// The build of bun this machine runs, by its name in [`BUN_BUILDS`]; `None`
/// where devpage downloads none. An x64 processor without AVX2 runs the
/// baseline build.
fn bun_build() -> Option<&'static str> {
    #[cfg(target_arch = "x86_64")]
    let modern = std::arch::is_x86_feature_detected!("avx2");
    #[cfg(not(target_arch = "x86_64"))]
    let modern = true;
    match (std::env::consts::OS, std::env::consts::ARCH, modern) {
        ("macos", "aarch64", _) => Some("bun-darwin-aarch64"),
        ("macos", "x86_64", true) => Some("bun-darwin-x64"),
        ("macos", "x86_64", false) => Some("bun-darwin-x64-baseline"),
        ("linux", "aarch64", _) => Some("bun-linux-aarch64"),
        ("linux", "x86_64", true) => Some("bun-linux-x64"),
        ("linux", "x86_64", false) => Some("bun-linux-x64-baseline"),
        _ => None,
    }
}

/// Whether the environment names where the session's models run: the Azure
/// endpoint with its key, or without that endpoint the OpenAI key. devpage
/// serves a session only then.
pub fn has_provider() -> bool {
    provided(|name| std::env::var(name).ok())
}

/// [`has_provider`], with each variable looked up by `var`; a blank one
/// counts as unset. The program picks the same way.
fn provided(var: impl Fn(&str) -> Option<String>) -> bool {
    let set = |name: &str| var(name).is_some_and(|value| !value.trim().is_empty());
    if set(AZURE_ENDPOINT) {
        set(AZURE_KEY)
    } else {
        set(KEY)
    }
}

/// The folder the live session is installed in: `~/.devpage/live`.
pub struct Installed(PathBuf);

impl Installed {
    /// The session's folder in `home`, devpage's own: `~/.devpage`.
    pub fn new(home: &Path) -> Self {
        Self(home.join("live"))
    }

    /// The folder `devpage setup` unpacks the program into.
    fn program(&self) -> PathBuf {
        self.0.join("program")
    }

    /// devpage's own bun, where `devpage setup` puts it.
    pub fn bun(&self) -> PathBuf {
        self.0.join("bun").join(bun_version()).join("bun")
    }

    /// The session's package: the program in `configured` — `live.program`
    /// of `~/.devpage/config.yml` — when there is one, else the installed
    /// program. An error says why there is no session: no program is
    /// installed, or the one installed isn't this executable's.
    pub fn package(&self, configured: Option<&Path>) -> Result<Arc<dyn Package>> {
        let folder = match configured {
            Some(folder) => folder.to_path_buf(),
            None => {
                let folder = self.program();
                match std::fs::read_to_string(folder.join(STAMP)) {
                    Ok(installed) if installed.trim() == version() => {}
                    Ok(_) => bail!(
                        "the program in {} is another devpage's: run `devpage setup`",
                        folder.display()
                    ),
                    Err(_) => bail!("its program is not installed: run `devpage setup`"),
                }
                folder
            }
        };
        super::extensions::load(&folder, LIVE)
            .with_context(|| format!("loading its program in {}", folder.display()))
    }

    /// Installs the session: downloads devpage's bun unless it is there, and
    /// unpacks the program and installs its packages unless this
    /// executable's program is installed already. Prints what it did.
    pub async fn setup(&self) -> Result<()> {
        let Some(build) = bun_build() else {
            if cfg!(windows) {
                bail!("`devpage setup` does not support Windows yet");
            }
            bail!(
                "`devpage setup` has no bun for {} on {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
        };
        let bun = self.bun();
        if bun.is_file() {
            println!("bun {} is in {}", bun_version(), bun.display());
        } else {
            let sum = BUN_BUILDS
                .iter()
                .find(|(name, _)| *name == build)
                .map(|(_, sum)| *sum)
                .with_context(|| format!("no checksum for {build}"))?;
            println!("Downloading bun {}", bun_version());
            self.download_bun(BUN_RELEASES, build, sum).await?;
            println!("bun {} is in {}", bun_version(), bun.display());
        }
        let program = self.program();
        let installed = std::fs::read_to_string(program.join(STAMP));
        if installed.is_ok_and(|installed| installed.trim() == version()) {
            println!("The live session's program is in {}", program.display());
            return Ok(());
        }
        self.unpack()?;
        println!("Installing the program's packages");
        self.install_packages()?;
        std::fs::write(program.join(STAMP), version())
            .with_context(|| format!("writing {}", program.join(STAMP).display()))?;
        println!("The live session's program is in {}", program.display());
        Ok(())
    }

    /// Downloads the zip of `build` from `releases`, checks that its SHA-256
    /// is `sum`, and puts the bun in it where [`bun`](Self::bun) says.
    async fn download_bun(&self, releases: &str, build: &str, sum: &str) -> Result<()> {
        let url = format!("{releases}/bun-v{}/{build}.zip", bun_version());
        let zip = async {
            let client = reqwest::Client::builder()
                .user_agent(concat!("devpage/", env!("CARGO_PKG_VERSION")))
                .build()?;
            let answer = client.get(&url).send().await?.error_for_status()?;
            answer.bytes().await
        }
        .await
        .with_context(|| format!("downloading {url}"))?;
        let found = format!("{:x}", Sha256::digest(&zip));
        if found != sum {
            bail!("{url} is not the bun devpage pins: its SHA-256 is {found}, not {sum}");
        }
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))
            .with_context(|| format!("opening {url}"))?;
        let mut packed = archive
            .by_name(&format!("{build}/bun"))
            .with_context(|| format!("{url} holds no bun"))?;
        let bun = self.bun();
        let folder = bun.parent().context("bun has no folder")?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("creating {}", folder.display()))?;
        // Written beside, then renamed: a bun that is there is whole.
        let partial = bun.with_extension("partial");
        let written = (|| {
            std::io::copy(&mut packed, &mut std::fs::File::create(&partial)?)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
            }
            std::fs::rename(&partial, &bun)
        })();
        written.with_context(|| format!("writing {}", bun.display()))
    }

    /// Writes the program this executable carries into its folder, in place
    /// of whatever was there.
    fn unpack(&self) -> Result<()> {
        let program = self.program();
        if program.exists() {
            std::fs::remove_dir_all(&program)
                .with_context(|| format!("removing {}", program.display()))?;
        }
        for (name, bytes) in PROGRAM {
            let file = program.join(name);
            let written = (|| {
                std::fs::create_dir_all(file.parent().unwrap_or(&program))?;
                std::fs::write(&file, bytes)
            })();
            written.with_context(|| format!("writing {}", file.display()))?;
        }
        Ok(())
    }

    /// Installs the packages the program's lockfile names, with devpage's
    /// bun, keeping what it downloads in devpage's folder.
    fn install_packages(&self) -> Result<()> {
        let bun = self.bun();
        let status = std::process::Command::new(&bun)
            .args(["install", "--frozen-lockfile", "--production"])
            .current_dir(self.program())
            .env("BUN_INSTALL_CACHE_DIR", self.0.join("bun").join("cache"))
            .stdin(std::process::Stdio::null())
            .status()
            .with_context(|| format!("running {}", bun.display()))?;
        if !status.success() {
            bail!("`bun install` failed ({status})");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty `~/.devpage` of its own, for a test.
    fn home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("devpage-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_azure_endpoint_with_its_key_or_else_the_openai_key_is_a_provider() {
        let with = |vars: &'static [(&'static str, &'static str)]| {
            provided(|name| {
                let found = vars.iter().find(|(known, _)| *known == name);
                found.map(|(_, value)| value.to_string())
            })
        };
        assert!(with(&[(KEY, "k")]));
        assert!(with(&[
            (AZURE_ENDPOINT, "talks.openai.azure.com"),
            (AZURE_KEY, "az")
        ]));
        assert!(!with(&[
            (KEY, "k"),
            (AZURE_ENDPOINT, "talks.openai.azure.com")
        ]));
        assert!(!with(&[(KEY, " "), (AZURE_KEY, "az")]));
        assert!(!with(&[]));
    }

    #[test]
    fn the_executable_carries_the_program_without_its_packages_evals_and_tests() {
        let names: Vec<&str> = PROGRAM.iter().map(|(name, _)| *name).collect();
        for needed in ["extension.yml", "package.json", "bun.lock"] {
            assert!(names.contains(&needed), "{needed} in {names:?}");
        }
        for name in names {
            assert!(
                !name.starts_with("node_modules/")
                    && !name.starts_with("evals/")
                    && !name.contains(".test.")
                    && name != "smoke.ts",
                "{name}"
            );
        }
    }

    #[test]
    fn the_session_is_the_installed_program_only_when_this_executable_unpacked_it() {
        let home = home("package");
        let installed = Installed::new(&home);
        let error = installed.package(None).err().unwrap().to_string();
        assert_eq!(error, "its program is not installed: run `devpage setup`");
        // Unpacked, but its packages are not in yet: still not installed.
        std::fs::create_dir_all(installed.program()).unwrap();
        std::fs::write(installed.program().join("stale.ts"), "").unwrap();
        installed.unpack().unwrap();
        assert!(!installed.program().join("stale.ts").exists());
        assert!(installed.package(None).is_err());
        std::fs::write(installed.program().join(STAMP), version()).unwrap();
        assert!(installed.package(None).unwrap().process().is_some());
        std::fs::write(installed.program().join(STAMP), "another").unwrap();
        let error = installed.package(None).err().unwrap().to_string();
        assert!(error.contains("another devpage's"), "{error}");
        assert!(error.contains("run `devpage setup`"), "{error}");
        // A configured program is run whatever is installed, and must be the
        // live session's.
        let own = home.join("own");
        std::fs::create_dir_all(&own).unwrap();
        let manifest = |id: &str| format!("id: {id}\nversion: \"1\"\napi: 1\nrun: [bun, s.ts]\n");
        std::fs::write(own.join("extension.yml"), manifest("live")).unwrap();
        let process = installed.package(Some(&own)).unwrap().process().unwrap();
        assert_eq!(process.folder, own.canonicalize().unwrap());
        std::fs::write(own.join("extension.yml"), manifest("clock")).unwrap();
        let error = format!("{:#}", installed.package(Some(&own)).err().unwrap());
        assert!(
            error.contains("Package clock is not the extension live"),
            "{error}"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[tokio::test]
    async fn bun_is_kept_only_when_its_zip_has_the_pinned_checksum() {
        use std::io::Write as _;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("bun-toy/bun", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"#!/bin/sh\n").unwrap();
        let zip = writer.finish().unwrap().into_inner();
        let sum = format!("{:x}", Sha256::digest(&zip));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let releases = format!("http://{}", listener.local_addr().unwrap());
        let path = format!("/bun-v{}/bun-toy.zip", bun_version());
        let router = axum::Router::new().route(&path, axum::routing::get(move || async { zip }));
        tokio::spawn(async move { axum::serve(listener, router).await });

        let home = home("bun");
        let installed = Installed::new(&home);
        let error = installed
            .download_bun(&releases, "bun-toy", "0000")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not the bun devpage pins"), "{error}");
        assert!(!installed.bun().exists());
        installed
            .download_bun(&releases, "bun-toy", &sum)
            .await
            .unwrap();
        assert_eq!(std::fs::read(installed.bun()).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(installed.bun()).unwrap().permissions();
            assert_eq!(mode.mode() & 0o777, 0o755);
        }
        let error = installed
            .download_bun(&releases, "bun-missing", &sum)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("downloading http://"), "{error}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn every_build_of_bun_this_devpage_picks_has_a_checksum() {
        if let Some(build) = bun_build() {
            assert!(BUN_BUILDS.iter().any(|(name, _)| *name == build), "{build}");
        }
        for (_, sum) in BUN_BUILDS {
            assert_eq!(sum.len(), 64);
        }
    }
}
