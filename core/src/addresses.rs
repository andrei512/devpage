//! Content addresses: the one place an address devpage serves for a page to
//! load is made.
//!
//! Everything a page loads from devpage itself — a built-in asset, a system
//! widget, a project widget or a helper it imports — is named by
//! [`address`], and by nothing else. The page's tags, the import map, the
//! widget list, the import inside a compiled widget, and the server's answer
//! to "is this address current?" all come from it, so two of them never
//! differ by a byte: a browser that preloads one address and imports another
//! downloads twice.
//!
//! Every address but a built-in asset's is the document's: it starts with the
//! served folder's mount, so a version in server mode, under `/d/<id>/v/<n>`,
//! asks its own router. A built-in asset is the same for every document.
//!
//! An address changes exactly when the bytes it serves would: a built-in
//! asset's carries the hash of its bytes (`/__devpage/devpage.<hash>.js`); a
//! widget's carries `?v=<hash>` over what its compiled module is made from.
//! What answers a current address never changes, so it is served
//! `immutable`; any other — a bare name, a stale hash — still answers, with
//! the current bytes and `no-cache`.

use sha2::{Digest, Sha256};

/// A built-in asset, as `build.rs` made it.
pub struct Asset {
    /// The name it is written by in the source: `devpage.js`.
    pub name: &'static str,
    /// Its name under its content address: `devpage.<hash>.js`.
    pub hashed: &'static str,
    /// The first 16 hex digits of the SHA-256 of its bytes.
    pub hash: &'static str,
    pub mime: &'static str,
    /// Its bytes, its references to other assets already content-addressed.
    pub identity: &'static [u8],
    /// The same bytes, compressed — `None` when that is no smaller.
    pub brotli: Option<&'static [u8]>,
    pub gzip: Option<&'static [u8]>,
}

// `BUILD` and `ASSETS`.
include!(concat!(env!("OUT_DIR"), "/manifest.rs"));

/// Something a page loads from devpage itself.
#[derive(Clone, Copy, Debug)]
pub enum Owned<'a> {
    /// A built-in asset, by the name it is written by: `"devpage.js"`.
    Asset(&'a str),
    /// A system widget, by name — `"Callout"` — for the document served under
    /// `mount`, at its version `hash`.
    SystemWidget {
        name: &'a str,
        mount: &'a str,
        hash: &'a str,
    },
    /// A project widget, or a helper a widget imports, by its `name` in the
    /// served folder under `mount`, at its version `hash` — what the
    /// [widgets](crate::widgets) say its compiled bytes are made from.
    Module {
        mount: &'a str,
        name: &'a str,
        hash: &'a str,
    },
}

/// The address `owned` is served at, for a page to load: the only function
/// that makes one.
///
/// Panics on an asset name that isn't built in — a mistake in devpage, not in
/// a document.
pub fn address(owned: Owned) -> String {
    match owned {
        Owned::Asset(name) => {
            let asset = ASSETS
                .iter()
                .find(|asset| asset.name == name)
                .unwrap_or_else(|| panic!("{name} is not a built-in asset"));
            format!("/__devpage/{}", asset.hashed)
        }
        Owned::SystemWidget { name, mount, hash } => {
            format!("{mount}/__devpage/system-widgets/{name}.tsx?v={hash}")
        }
        Owned::Module { mount, name, hash } => {
            format!("{mount}/__devpage/widgets/{name}?v={hash}")
        }
    }
}

/// The built-in asset a name under `/__devpage/` asks for, and whether it is
/// its current content address. A bare name (`devpage.js`) and a stale hash
/// (`devpage.0123456789abcdef.js`) find the asset, but not as current.
pub fn asset(requested: &str) -> Option<(&'static Asset, bool)> {
    if let Some(asset) = ASSETS.iter().find(|asset| asset.hashed == requested) {
        return Some((asset, true));
    }
    let (stem, rest) = requested.split_once('.')?;
    let ext = rest.rsplit_once('.').map_or(rest, |(_, ext)| ext);
    ASSETS
        .iter()
        .find(|asset| asset.name == format!("{stem}.{ext}"))
        .map(|asset| (asset, false))
}

/// The first 16 hex digits of a digest: what every address and validator
/// carries.
pub fn short(digest: &[u8]) -> String {
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The first 16 hex digits of the SHA-256 of `bytes`.
pub fn hash(bytes: &[u8]) -> String {
    short(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_has_a_content_address_and_old_names_still_find_it() {
        for asset in ASSETS {
            let address = address(Owned::Asset(asset.name));
            assert_eq!(address, format!("/__devpage/{}", asset.hashed));
            assert_eq!(asset.hash, hash(asset.identity), "{}", asset.name);
            let (found, current) = super::asset(asset.hashed).unwrap();
            assert!(current && found.name == asset.name);
            let (found, current) = super::asset(asset.name).unwrap();
            assert!(!current && found.name == asset.name);
            let (stem, ext) = asset.name.split_once('.').unwrap();
            let (found, current) = super::asset(&format!("{stem}.0123456789abcdef.{ext}")).unwrap();
            assert!(!current && found.name == asset.name);
        }
        assert!(super::asset("nope.js").is_none());
    }

    #[test]
    fn an_asset_names_the_others_by_their_content_addresses() {
        let text = |name: &str| {
            let asset = ASSETS.iter().find(|a| a.name == name).unwrap();
            String::from_utf8(asset.identity.to_vec()).unwrap()
        };
        assert!(text("devpage.js").contains(&address(Owned::Asset("design.js"))));
        let design = text("design.js");
        assert!(design.contains(&address(Owned::Asset("design-engine.js"))));
        assert!(design.contains(&address(Owned::Asset("design-engine.wasm"))));
        assert!(design.contains(&address(Owned::Asset("design-stream.js"))));
        assert!(design.contains(&address(Owned::Asset("design-canvas.js"))));
        assert!(text("design-canvas.js").contains(&address(Owned::Asset("design-tiles.js"))));
        assert!(text("design-tiles.js").contains(&address(Owned::Asset("design-worker.js"))));
    }

    #[test]
    fn a_widget_address_carries_its_mount_and_version() {
        assert_eq!(
            address(Owned::SystemWidget {
                name: "Callout",
                mount: "/d/x/v/1",
                hash: "0123456789abcdef",
            }),
            "/d/x/v/1/__devpage/system-widgets/Callout.tsx?v=0123456789abcdef"
        );
        assert_eq!(
            address(Owned::Module {
                mount: "",
                name: "docs/widgets/Card.tsx",
                hash: "fedcba9876543210",
            }),
            "/__devpage/widgets/docs/widgets/Card.tsx?v=fedcba9876543210"
        );
    }
}
