//! What server mode's cache keeps, and the port that chooses what it lets go.
//! The cache itself — files on the PVC — is the binary's; deciding which
//! entries to delete when it is past its limit is a pure function of them.

use std::time::SystemTime;

/// One version's copy in the cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The document's id.
    pub document: String,
    /// The version's number.
    pub version: u64,
    /// The bytes it takes on disk.
    pub bytes: u64,
    /// When it was last read.
    pub read: SystemTime,
}

impl Entry {
    /// The entry's key: its document and version.
    pub fn key(&self) -> (String, u64) {
        (self.document.clone(), self.version)
    }
}

/// The eviction policy: given every entry, the cache's limit and its size,
/// which entries to delete. It decides and never acts — the cache deletes
/// them, skipping any being served.
pub trait Eviction: Send + Sync {
    fn choose(&self, entries: &[Entry], limit: u64) -> Vec<(String, u64)>;
}

/// The least recently read first, until the cache is at 90% of its limit, so
/// a full cache doesn't evict on every read.
pub struct Lru;

impl Eviction for Lru {
    fn choose(&self, entries: &[Entry], limit: u64) -> Vec<(String, u64)> {
        let mut size: u64 = entries.iter().map(|e| e.bytes).sum();
        if size <= limit {
            return Vec::new();
        }
        let target = limit / 10 * 9;
        let mut oldest: Vec<&Entry> = entries.iter().collect();
        oldest.sort_by_key(|e| e.read);
        let mut chosen = Vec::new();
        for entry in oldest {
            if size <= target {
                break;
            }
            size -= entry.bytes;
            chosen.push(entry.key());
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entry(document: &str, bytes: u64, age: u64) -> Entry {
        Entry {
            document: document.into(),
            version: 1,
            bytes,
            read: SystemTime::UNIX_EPOCH + Duration::from_secs(1000 - age),
        }
    }

    #[test]
    fn under_the_limit_nothing_goes() {
        let entries = [entry("a", 40, 1), entry("b", 50, 2)];
        assert!(Lru.choose(&entries, 100).is_empty());
    }

    #[test]
    fn past_the_limit_the_least_recently_read_go_until_it_is_at_ninety_percent() {
        let entries = [
            entry("new", 30, 1),
            entry("old", 30, 9),
            entry("mid", 30, 5),
            entry("older", 30, 7),
        ];
        // 120 bytes against a limit of 100: down to 90 takes the oldest only.
        assert_eq!(Lru.choose(&entries, 100), [("old".to_string(), 1)]);
        // Against 60, down to 54: three go, oldest first.
        assert_eq!(
            Lru.choose(&entries, 60),
            [
                ("old".to_string(), 1),
                ("older".to_string(), 1),
                ("mid".to_string(), 1)
            ]
        );
    }
}
