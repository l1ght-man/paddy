//! The folder of vaults: listing them, creating new ones, and searching across all.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::Entry;
use crate::vault::Vault;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultInfo {
    pub path: PathBuf,
    pub name: String,
    pub last_opened: i64,
}

/// One search result from [`search_all`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub vault: VaultInfo,
    pub entry: Entry,
}

/// Every readable `*.db` vault in `dir`, most recently opened first.
/// Files that are not vaults are skipped. A missing dir is an empty library.
pub fn list_vaults(dir: &Path) -> Vec<VaultInfo> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<VaultInfo> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "db"))
        .filter_map(|p| {
            let m = Vault::peek_meta(&p).ok()?;
            Some(VaultInfo { name: m.name, last_opened: m.last_opened, path: p })
        })
        .collect();
    out.sort_by(|a, b| b.last_opened.cmp(&a.last_opened).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Filesystem-safe file stem for a vault name: lowercase alphanumerics and `-`.
fn slug(name: &str) -> String {
    let s: String =
        name.trim().chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "vault".into()
    } else {
        s
    }
}

/// Create a new vault called `name` in `dir` (file name derived from the name,
/// numbered if taken). Returns the open vault.
pub fn create_vault(dir: &Path, name: &str) -> Result<Vault> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::EmptyName);
    }
    let stem = slug(name);
    let mut path = dir.join(format!("{stem}.db"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.db"));
        n += 1;
    }
    Vault::create(path, name)
}

/// Search every vault in `dir`; hits grouped by vault (most recent vault first).
/// `live` is the vault currently open in memory: if it is one of the files, its
/// unsaved state is searched instead of what is on disk.
pub fn search_all(dir: &Path, query: &str, live: Option<&Vault>) -> Vec<Hit> {
    let mut hits = Vec::new();
    for info in list_vaults(dir) {
        let peeked;
        let v = match live.filter(|l| l.path() == Some(info.path.as_path())) {
            Some(l) => l,
            None => match Vault::peek(&info.path) {
                Ok(p) => {
                    peeked = p;
                    &peeked
                }
                Err(_) => continue,
            },
        };
        let Ok(found) = v.search(query) else { continue };
        hits.extend(found.into_iter().map(|entry| Hit { vault: info.clone(), entry }));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Field;

    #[test]
    fn create_list_and_number_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let a = create_vault(dir.path(), "HTB Lab #1").unwrap();
        assert_eq!(a.path().unwrap().file_name().unwrap(), "htb-lab-1.db");
        let b = create_vault(dir.path(), "htb lab 1").unwrap();
        assert_eq!(b.path().unwrap().file_name().unwrap(), "htb-lab-1-2.db");
        assert!(matches!(create_vault(dir.path(), "  "), Err(Error::EmptyName)));
        assert_eq!(create_vault(dir.path(), "???").unwrap().path().unwrap().file_name().unwrap(), "vault.db");

        fs::write(dir.path().join("junk.db"), "not sqlite").unwrap();
        fs::write(dir.path().join("notes.txt"), "x").unwrap();
        let names: Vec<_> = list_vaults(dir.path()).into_iter().map(|v| v.name).collect();
        assert_eq!(names.len(), 3, "junk.db and notes.txt are skipped");
        assert!(names.contains(&"HTB Lab #1".to_string()));
        assert!(list_vaults(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn peek_does_not_stamp_or_write() {
        let dir = tempfile::tempdir().unwrap();
        let v = create_vault(dir.path(), "a").unwrap();
        let path = v.path().unwrap().to_path_buf();
        let before = fs::read(&path).unwrap();
        let p = Vault::peek(&path).unwrap();
        assert!(!p.is_dirty());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn search_across_vaults() {
        let dir = tempfile::tempdir().unwrap();
        for (vault, label, ip) in [("one", "dc01", "10.0.0.1"), ("two", "dc02", "10.0.0.2")] {
            let mut v = create_vault(dir.path(), vault).unwrap();
            let mut e = Entry::new(label);
            e.fields = vec![Field::new("host", ip)];
            v.add_entry(&e).unwrap();
            v.add_entry(&Entry::new("other")).unwrap();
            v.save().unwrap();
        }
        let hits = search_all(dir.path(), "10.0.0.2", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].vault.name, "two");
        assert_eq!(hits[0].entry.label, "dc02");
        assert_eq!(search_all(dir.path(), "dc", None).len(), 2);
        assert_eq!(search_all(dir.path(), "", None).len(), 4);

        // unsaved edits in the live vault are searched, not the stale file
        let mut live = Vault::open(dir.path().join("one.db")).unwrap();
        live.add_entry(&Entry::new("fresh-unsaved")).unwrap();
        assert!(search_all(dir.path(), "fresh-unsaved", None).is_empty());
        let hits = search_all(dir.path(), "fresh-unsaved", Some(&live));
        assert_eq!((hits.len(), hits[0].vault.name.as_str()), (1, "one"));
    }
}
