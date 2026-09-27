use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::backup::Backup;
use rusqlite::config::DbConfig;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::error::{Error, Result};
use crate::model::{Entry, Field, Meta};
use crate::template::{default_templates, Template};

const SCHEMA_VERSION: i64 = 1;

/// Refuse to load vault files bigger than this (a vault is text; this is generous).
pub const MAX_VAULT_MB: u64 = 512;

/// Upper bound on templates per vault, so an import can't balloon a vault.
pub const MAX_TEMPLATES: usize = 2000;

/// Result of adding a template pack to a vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    pub added: usize,
    /// Already present (same name and same pattern).
    pub skipped: usize,
}

const SCHEMA: &str = "
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE entries (
    id      INTEGER PRIMARY KEY,
    label   TEXT NOT NULL,
    notes   TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL
);
CREATE TABLE fields (
    id        INTEGER PRIMARY KEY,
    entry_id  INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    is_secret INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX fields_entry ON fields(entry_id, position);
CREATE TABLE tags (
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    tag      TEXT NOT NULL,
    PRIMARY KEY (entry_id, tag)
);
CREATE TABLE templates (
    id      INTEGER PRIMARY KEY,
    name    TEXT NOT NULL,
    pattern TEXT NOT NULL
);
";

fn read_rev(conn: &Connection) -> i64 {
    conn.query_row("SELECT value FROM meta WHERE key = 'rev'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Revision stored in the vault file on disk.
fn disk_rev(path: &Path) -> Result<i64> {
    let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    harden(&src)?;
    Ok(read_rev(&src))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Lock down a connection: SQLite's defensive mode blocks features that let a
/// crafted database file corrupt itself or run unexpected schema code.
fn harden(conn: &Connection) -> Result<()> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
    conn.pragma_update(None, "cell_size_check", true)?;
    Ok(())
}

fn new_conn() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    harden(&conn)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(conn)
}

/// Copy a whole database from `src` into `dst` (SQLite online backup).
fn copy_db(src: &Connection, dst: &mut Connection) -> Result<()> {
    Backup::new(src, dst)?.run_to_completion(256, Duration::ZERO, None)?;
    Ok(())
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
    Ok(())
}

fn read_meta(conn: &Connection) -> Result<Meta> {
    let get = |key: &str| -> Result<String> {
        Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))?)
    };
    Ok(Meta {
        name: get("name")?,
        created: get("created")?.parse().unwrap_or(0),
        last_opened: get("last_opened")?.parse().unwrap_or(0),
    })
}

/// Trim, drop empties, dedupe (keeping first-seen order).
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Replace an entry's fields and tags with the given ones.
fn write_children(conn: &Connection, id: i64, fields: &[Field], tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM fields WHERE entry_id = ?1", [id])?;
    conn.execute("DELETE FROM tags WHERE entry_id = ?1", [id])?;
    let mut ins_field = conn
        .prepare_cached("INSERT INTO fields (entry_id, position, key, value, is_secret) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for (pos, f) in fields.iter().enumerate() {
        ins_field.execute(params![id, pos as i64, f.key, f.value, f.is_secret])?;
    }
    let mut ins_tag = conn.prepare_cached("INSERT INTO tags (entry_id, tag) VALUES (?1, ?2)")?;
    for t in normalize_tags(tags) {
        ins_tag.execute(params![id, t])?;
    }
    Ok(())
}

/// One project. Works on an in-memory database; `save()` checkpoints it to disk.
pub struct Vault {
    conn: Connection,
    path: Option<PathBuf>,
    meta: Meta,
    dirty: bool,
    /// Revision of the file as of our last read/write. Every real save bumps it, so a
    /// different value on disk means another process saved in between.
    rev: i64,
}

impl Vault {
    /// Fresh vault with default templates, not tied to any file (yet).
    pub fn in_memory(name: &str) -> Result<Self> {
        let conn = new_conn()?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let t = now();
        set_meta(&conn, "name", name)?;
        set_meta(&conn, "created", &t.to_string())?;
        set_meta(&conn, "last_opened", &t.to_string())?;
        for tpl in default_templates() {
            conn.execute("INSERT INTO templates (name, pattern) VALUES (?1, ?2)", params![tpl.name, tpl.pattern])?;
        }
        let meta = read_meta(&conn)?;
        Ok(Self { conn, path: None, meta, dirty: true, rev: 0 })
    }

    /// Create a new vault file. Fails if the path already exists.
    pub fn create(path: impl AsRef<Path>, name: &str) -> Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            return Err(Error::AlreadyExists(path.to_path_buf()));
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            crate::fsutil::ensure_private_dir(parent)?;
        }
        let mut vault = Self::in_memory(name)?;
        vault.path = Some(path.to_path_buf());
        vault.save()?;
        Ok(vault)
    }

    /// Copy a vault file into memory without touching it (no `last_opened` stamp).
    fn load(path: &Path) -> Result<Self> {
        let len = fs::metadata(path)?.len();
        if len > MAX_VAULT_MB * 1024 * 1024 {
            return Err(Error::TooLarge(MAX_VAULT_MB));
        }
        let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        harden(&src)?;
        let version: i64 = src.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let mut conn = new_conn()?;
        copy_db(&src, &mut conn)?;
        drop(src);
        // Structural integrity of what we copied; a damaged file is refused, not half-loaded.
        let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0)).map_err(|_| Error::Corrupt)?;
        if check != "ok" {
            return Err(Error::Corrupt);
        }
        let meta = read_meta(&conn)?;
        let rev = read_rev(&conn);
        Ok(Self { conn, path: Some(path.to_path_buf()), meta, dirty: false, rev })
    }

    /// Load a vault file into memory and stamp `last_opened`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut vault = Self::load(path.as_ref())?;
        set_meta(&vault.conn, "last_opened", &now().to_string())?;
        vault.meta = read_meta(&vault.conn)?;
        vault.dirty = true;
        // Best effort: a read-only vault file should still open (stays dirty).
        let _ = vault.checkpoint(false, false);
        Ok(vault)
    }

    /// Read just the metadata of a vault file (name, timestamps) without loading its contents.
    pub fn peek_meta(path: impl AsRef<Path>) -> Result<Meta> {
        let src = Connection::open_with_flags(path.as_ref(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        harden(&src)?;
        let version: i64 = src.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        read_meta(&src)
    }

    /// Load a vault for reading only (search, listing): nothing is stamped or written.
    pub fn peek(path: impl AsRef<Path>) -> Result<Self> {
        Self::load(path.as_ref())
    }

    /// Checkpoint the in-memory state to the vault file (temp file + atomic rename).
    /// Refuses with [`Error::ChangedOnDisk`] if another process saved the file
    /// since we loaded or last saved it; use [`Vault::save_overwrite`] to force.
    pub fn save(&mut self) -> Result<()> {
        self.checkpoint(false, true)
    }

    /// Like [`Vault::save`] but replaces whatever is on disk.
    pub fn save_overwrite(&mut self) -> Result<()> {
        self.checkpoint(true, true)
    }

    /// `bump`: count this as a content save (opening a vault only stamps
    /// `last_opened` and must not look like an edit to other instances).
    fn checkpoint(&mut self, force: bool, bump: bool) -> Result<()> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        if !force && path.exists() && disk_rev(&path)? != self.rev {
            return Err(Error::ChangedOnDisk(path));
        }
        let new_rev = if bump { self.rev + 1 } else { self.rev };
        set_meta(&self.conn, "rev", &new_rev.to_string())?;
        let written = self.write_to(&path);
        if written.is_err() {
            set_meta(&self.conn, "rev", &self.rev.to_string())?;
        }
        written?;
        self.rev = new_rev;
        self.dirty = false;
        Ok(())
    }

    /// Write the in-memory state next to the vault as `<name>.conflict-<unix time>.db`
    /// without touching the vault file. Returns the new path.
    pub fn save_conflict_copy(&self) -> Result<PathBuf> {
        let path = self.path.as_ref().ok_or(Error::NoPath)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let copy = path.with_file_name(format!("{stem}.conflict-{}.db", now()));
        self.write_to(&copy)?;
        Ok(copy)
    }

    fn write_to(&self, path: &Path) -> Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let _ = fs::remove_file(&tmp);

        // Pre-create the temp file with owner-only permissions so that vault
        // secrets are never world-readable, even briefly during the copy.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(Error::from)?;
        }
        #[cfg(not(unix))]
        {
            fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(Error::from)?;
        }

        let written = Connection::open(&tmp)
            .map_err(Error::from)
            .and_then(|mut dst| copy_db(&self.conn, &mut dst))
            .and_then(|()| fs::rename(&tmp, path).map_err(Error::from));
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        written
    }

    /// True when memory has changes not yet written to disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    pub fn rename(&mut self, name: &str) -> Result<()> {
        set_meta(&self.conn, "name", name)?;
        self.meta.name = name.to_string();
        self.dirty = true;
        Ok(())
    }

    // ---- entries ----

    /// Fill in fields and tags for `entries` with two queries total (not two per entry).
    fn hydrate_all(&self, mut entries: Vec<Entry>) -> Result<Vec<Entry>> {
        let mut fields: HashMap<i64, Vec<Field>> = HashMap::new();
        let mut st = self
            .conn
            .prepare_cached("SELECT entry_id, key, value, is_secret FROM fields ORDER BY entry_id, position")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, Field { key: r.get(1)?, value: r.get(2)?, is_secret: r.get(3)? }))
        })?;
        for row in rows {
            let (id, f) = row?;
            fields.entry(id).or_default().push(f);
        }
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        let mut st = self.conn.prepare_cached("SELECT entry_id, tag FROM tags ORDER BY rowid")?;
        for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, t) = row?;
            tags.entry(id).or_default().push(t);
        }
        for e in &mut entries {
            e.fields = fields.remove(&e.id).unwrap_or_default();
            e.tags = tags.remove(&e.id).unwrap_or_default();
        }
        Ok(entries)
    }

    fn hydrate(&self, e: Entry) -> Result<Entry> {
        let mut st = self
            .conn
            .prepare_cached("SELECT key, value, is_secret FROM fields WHERE entry_id = ?1 ORDER BY position")?;
        let mut e = e;
        e.fields = st
            .query_map([e.id], |r| Ok(Field { key: r.get(0)?, value: r.get(1)?, is_secret: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
        let mut st = self.conn.prepare_cached("SELECT tag FROM tags WHERE entry_id = ?1 ORDER BY rowid")?;
        e.tags = st.query_map([e.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(e)
    }

    fn entry_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
        Ok(Entry {
            id: r.get(0)?,
            label: r.get(1)?,
            notes: r.get(2)?,
            created: r.get(3)?,
            updated: r.get(4)?,
            fields: Vec::new(),
            tags: Vec::new(),
        })
    }

    /// All entries in creation order.
    pub fn list_entries(&self) -> Result<Vec<Entry>> {
        let mut st = self.conn.prepare_cached("SELECT id, label, notes, created, updated FROM entries ORDER BY id")?;
        let rows: Vec<Entry> = st.query_map([], Self::entry_from_row)?.collect::<rusqlite::Result<_>>()?;
        self.hydrate_all(rows)
    }

    pub fn get_entry(&self, id: i64) -> Result<Entry> {
        let e = self
            .conn
            .query_row(
                "SELECT id, label, notes, created, updated FROM entries WHERE id = ?1",
                [id],
                Self::entry_from_row,
            )
            .optional()?
            .ok_or(Error::NotFound { what: "entry", id })?;
        self.hydrate(e)
    }

    /// Entries matching `query` (see [`Entry::matches`]), in creation order.
    pub fn search(&self, query: &str) -> Result<Vec<Entry>> {
        Ok(self.list_entries()?.into_iter().filter(|e| e.matches(query)).collect())
    }

    /// Insert a new entry (its `id`/timestamps are ignored); returns the new id.
    pub fn add_entry(&mut self, entry: &Entry) -> Result<i64> {
        let t = now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO entries (label, notes, created, updated) VALUES (?1, ?2, ?3, ?3)",
            params![entry.label, entry.notes, t],
        )?;
        let id = tx.last_insert_rowid();
        write_children(&tx, id, &entry.fields, &entry.tags)?;
        tx.commit()?;
        self.dirty = true;
        Ok(id)
    }

    /// Overwrite the entry with `entry.id` (label, notes, fields, tags).
    pub fn update_entry(&mut self, entry: &Entry) -> Result<()> {
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE entries SET label = ?2, notes = ?3, updated = ?4 WHERE id = ?1",
            params![entry.id, entry.label, entry.notes, now()],
        )?;
        if changed == 0 {
            return Err(Error::NotFound { what: "entry", id: entry.id });
        }
        write_children(&tx, entry.id, &entry.fields, &entry.tags)?;
        tx.commit()?;
        self.dirty = true;
        Ok(())
    }

    /// Permanent delete (fields and tags go with it).
    pub fn delete_entry(&mut self, id: i64) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM entries WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound { what: "entry", id });
        }
        self.dirty = true;
        Ok(())
    }

    // ---- templates ----

    pub fn list_templates(&self) -> Result<Vec<Template>> {
        let mut st = self.conn.prepare_cached("SELECT id, name, pattern FROM templates ORDER BY id")?;
        let rows = st
            .query_map([], |r| Ok(Template { id: r.get(0)?, name: r.get(1)?, pattern: r.get(2)? }))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn add_template(&mut self, tpl: &Template) -> Result<i64> {
        self.conn.execute("INSERT INTO templates (name, pattern) VALUES (?1, ?2)", params![tpl.name, tpl.pattern])?;
        self.dirty = true;
        Ok(self.conn.last_insert_rowid())
    }

    /// Add a pack's templates. Ones already present (same name and pattern) are
    /// skipped, so importing twice changes nothing. A different template with a
    /// taken name is added as `name (tag)`. All or nothing.
    pub fn import_templates(&mut self, templates: &[Template], tag: &str) -> Result<ImportReport> {
        let existing = self.list_templates()?;
        let mut names: Vec<String> = existing.iter().map(|t| t.name.clone()).collect();
        let (mut added, mut skipped) = (0, 0);
        let tx = self.conn.transaction()?;
        for t in templates {
            if existing.iter().any(|e| e.name == t.name && e.pattern == t.pattern) {
                skipped += 1;
                continue;
            }
            if existing.len() + added >= MAX_TEMPLATES {
                return Err(Error::TooManyTemplates(MAX_TEMPLATES));
            }
            let mut name = t.name.clone();
            if names.contains(&name) {
                name = format!("{} ({tag})", t.name);
                let mut n = 2;
                while names.contains(&name) {
                    name = format!("{} ({tag} {n})", t.name);
                    n += 1;
                }
            }
            tx.execute("INSERT INTO templates (name, pattern) VALUES (?1, ?2)", params![name, t.pattern])?;
            names.push(name);
            added += 1;
        }
        tx.commit()?;
        if added > 0 {
            self.dirty = true;
        }
        Ok(ImportReport { added, skipped })
    }

    pub fn update_template(&mut self, tpl: &Template) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE templates SET name = ?2, pattern = ?3 WHERE id = ?1",
            params![tpl.id, tpl.name, tpl.pattern],
        )?;
        if changed == 0 {
            return Err(Error::NotFound { what: "template", id: tpl.id });
        }
        self.dirty = true;
        Ok(())
    }

    pub fn delete_template(&mut self, id: i64) -> Result<()> {
        let changed = self.conn.execute("DELETE FROM templates WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound { what: "template", id });
        }
        self.dirty = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Entry {
        let mut e = Entry::new("dc01");
        e.notes = "# domain controller\nkerberoast me".into();
        e.fields = vec![
            Field::new("host", "10.10.10.5"),
            Field::secret("password", "hunter2"),
            Field::new("user", "administrator"),
        ];
        e.tags = vec!["ad".into(), "htb".into()];
        e
    }

    fn count(v: &Vault, table: &str) -> i64 {
        v.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn save_and_reopen_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        let id1 = v.add_entry(&sample()).unwrap();
        let id2 = v.add_entry(&Entry::new("empty")).unwrap();
        v.save().unwrap();
        let before = v.list_entries().unwrap();

        let v2 = Vault::open(&path).unwrap();
        assert_eq!(v2.meta().name, "lab");
        let after = v2.list_entries().unwrap();
        assert_eq!(before, after);
        assert_eq!(after[0].id, id1);
        assert_eq!(after[1].id, id2);
        let e = v2.get_entry(id1).unwrap();
        assert_eq!(e.fields, sample().fields, "field order and secret flag survive");
        assert_eq!(e.tags, ["ad", "htb"]);
        assert_eq!(e.notes, sample().notes);
        assert!(e.field("password").unwrap().is_secret);
    }

    #[test]
    fn unsaved_changes_are_not_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        v.add_entry(&sample()).unwrap();
        assert!(Vault::open(&path).unwrap().list_entries().unwrap().is_empty());
        v.save().unwrap();
        assert_eq!(Vault::open(&path).unwrap().list_entries().unwrap().len(), 1);
    }

    #[test]
    fn dirty_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        assert!(!v.is_dirty());
        let id = v.add_entry(&sample()).unwrap();
        assert!(v.is_dirty());
        v.save().unwrap();
        assert!(!v.is_dirty());
        v.delete_entry(id).unwrap();
        assert!(v.is_dirty());
        v.save().unwrap();
        v.rename("lab2").unwrap();
        assert!(v.is_dirty());
        assert!(!Vault::open(&path).unwrap().is_dirty(), "fresh open is clean");
    }

    #[test]
    fn delete_cascades() {
        let mut v = Vault::in_memory("t").unwrap();
        let id = v.add_entry(&sample()).unwrap();
        let keep = v.add_entry(&sample()).unwrap();
        assert_eq!((count(&v, "fields"), count(&v, "tags")), (6, 4));
        v.delete_entry(id).unwrap();
        assert_eq!((count(&v, "entries"), count(&v, "fields"), count(&v, "tags")), (1, 3, 2));
        assert!(matches!(v.get_entry(id), Err(Error::NotFound { .. })));
        assert!(v.get_entry(keep).is_ok());
        assert!(matches!(v.delete_entry(id), Err(Error::NotFound { .. })));
    }

    #[test]
    fn update_replaces_children() {
        let mut v = Vault::in_memory("t").unwrap();
        let id = v.add_entry(&sample()).unwrap();
        let mut e = v.get_entry(id).unwrap();
        e.label = "dc02".into();
        e.fields = vec![Field::new("host", "10.10.10.6")];
        e.tags = vec!["  ".into(), "x".into(), "x".into(), " y ".into()];
        v.update_entry(&e).unwrap();
        let got = v.get_entry(id).unwrap();
        assert_eq!(got.label, "dc02");
        assert_eq!(got.fields, [Field::new("host", "10.10.10.6")]);
        assert_eq!(got.tags, ["x", "y"], "tags trimmed and deduped");

        e.id = 999;
        assert!(matches!(v.update_entry(&e), Err(Error::NotFound { id: 999, .. })));
    }

    #[test]
    fn create_refuses_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        Vault::create(&path, "a").unwrap();
        assert!(matches!(Vault::create(&path, "b"), Err(Error::AlreadyExists(_))));
    }

    #[test]
    fn create_makes_parent_dirs_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vaults/nested/lab.db");
        Vault::create(&path, "a").unwrap();
        assert!(path.exists());
        assert!(!dir.path().join("vaults/nested/lab.db.tmp").exists());
    }

    #[test]
    fn open_rejects_non_vaults() {
        let dir = tempfile::tempdir().unwrap();

        let missing = dir.path().join("nope.db");
        assert!(Vault::open(&missing).is_err());
        assert!(!missing.exists(), "open must not create files");

        let plain = dir.path().join("plain.db");
        Connection::open(&plain).unwrap().execute_batch("CREATE TABLE x (a);").unwrap();
        assert!(matches!(Vault::open(&plain), Err(Error::UnsupportedVersion(0))));

        let text = dir.path().join("text.db");
        fs::write(&text, "definitely not sqlite, just some text to fill a header").unwrap();
        assert!(matches!(Vault::open(&text), Err(Error::Db(_))));
    }

    #[test]
    fn damaged_and_hostile_vault_files_are_refused_not_crashed_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        for i in 0..200 {
            let mut e = sample();
            e.label = format!("host-{i}");
            e.notes = "x".repeat(500);
            v.add_entry(&e).unwrap();
        }
        v.save().unwrap();
        let good = fs::read(&path).unwrap();
        assert!(good.len() > 40_000);

        // flip bytes at many places, truncate, and zero-fill: never a panic, never bogus success
        for (i, at) in [100usize, 4096, 5000, 12_000, 20_000, 33_333, good.len() - 10].into_iter().enumerate() {
            let mut bad = good.clone();
            for k in 0..64 {
                bad[at + k % 8] ^= 0xa5 ^ (k as u8);
            }
            let p = dir.path().join(format!("bad{i}.db"));
            fs::write(&p, &bad).unwrap();
            // Either refused, or (if the damage hit only cell content) loads without panicking.
            if let Ok(v) = Vault::open(&p) {
                let _ = v.list_entries();
            }
        }
        let cut = dir.path().join("cut.db");
        fs::write(&cut, &good[..good.len() / 2]).unwrap();
        assert!(Vault::open(&cut).is_err() || Vault::open(&cut).unwrap().list_entries().is_ok());
        let zeros = dir.path().join("zeros.db");
        fs::write(&zeros, vec![0u8; 10_000]).unwrap();
        assert!(Vault::open(&zeros).is_err());
        let text = dir.path().join("text.db");
        fs::write(&text, "SQLite format 3\0 but really just text").unwrap();
        assert!(Vault::open(&text).is_err());
    }

    #[test]
    fn oversized_vault_files_are_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("huge.db");
        let f = fs::File::create(&p).unwrap();
        f.set_len(MAX_VAULT_MB * 1024 * 1024 + 1).unwrap(); // sparse file: no real disk used
        assert!(matches!(Vault::open(&p), Err(Error::TooLarge(_))));
        assert!(matches!(Vault::peek(&p), Err(Error::TooLarge(_))));
    }

    #[test]
    fn open_stamps_last_opened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        set_meta(&v.conn, "last_opened", "0").unwrap();
        v.save().unwrap();
        let v2 = Vault::open(&path).unwrap();
        assert!(v2.meta().last_opened > 0);
        assert_eq!(Vault::open(&path).unwrap().meta().created, v.meta().created);
    }

    #[cfg(unix)]
    #[test]
    fn saved_vault_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        Vault::create(&path, "lab").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn peek_meta_reads_only_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut v = Vault::create(&path, "lab").unwrap();
        v.add_entry(&sample()).unwrap();
        v.save().unwrap();
        assert_eq!(Vault::peek_meta(&path).unwrap().name, "lab");
        assert!(Vault::peek_meta(dir.path().join("nope.db")).is_err());
    }

    #[test]
    fn concurrent_writer_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lab.db");
        let mut a = Vault::create(&path, "lab").unwrap();
        let mut b = Vault::open(&path).unwrap();

        a.add_entry(&Entry::new("from-a")).unwrap();
        a.save().unwrap();
        b.add_entry(&Entry::new("from-b")).unwrap();
        assert!(matches!(b.save(), Err(Error::ChangedOnDisk(_))));
        assert!(b.is_dirty(), "a refused save leaves the vault dirty");
        assert_eq!(Vault::open(&path).unwrap().list_entries().unwrap()[0].label, "from-a");

        // the copy keeps b's work without touching the vault
        let copy = b.save_conflict_copy().unwrap();
        assert_eq!(Vault::peek(&copy).unwrap().list_entries().unwrap()[0].label, "from-b");
        assert_eq!(Vault::open(&path).unwrap().list_entries().unwrap()[0].label, "from-a");

        // forcing overwrites, and later saves work normally again
        b.save_overwrite().unwrap();
        assert!(!b.is_dirty());
        b.add_entry(&Entry::new("more")).unwrap();
        b.save().unwrap();
        assert_eq!(Vault::peek(&path).unwrap().list_entries().unwrap().len(), 2);
    }

    #[test]
    fn unicode_and_special_text_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("u.db");
        let mut v = Vault::create(&path, "café ☕").unwrap();
        let mut e = Entry::new("Ünïcödé — 日本語");
        e.notes = "quote ' \" ; DROP TABLE entries; --\nline2".into();
        e.fields = vec![Field::secret("pässwörd", "🔑 p@ss'w\"rd")];
        let id = v.add_entry(&e).unwrap();
        v.save().unwrap();
        let v2 = Vault::open(&path).unwrap();
        assert_eq!(v2.meta().name, "café ☕");
        let got = v2.get_entry(id).unwrap();
        assert_eq!(got.label, e.label);
        assert_eq!(got.notes, e.notes);
        assert_eq!(got.fields, e.fields);
        assert_eq!(v2.search("日本語").unwrap().len(), 1);
        assert_eq!(v2.search("ÜNÏCÖDÉ").unwrap().len(), 1, "case-insensitive beyond ASCII");
        assert_eq!(v2.list_entries().unwrap().len(), 1, "injection text stayed data");
    }

    #[test]
    fn large_vault_search_is_fast() {
        let mut v = Vault::in_memory("big").unwrap();
        for i in 0..3000 {
            let mut e = Entry::new(format!("host-{i}"));
            e.tags = vec!["lab".into()];
            e.fields = vec![Field::new("host", format!("10.0.{}.{}", i / 250, i % 250))];
            v.add_entry(&e).unwrap();
        }
        let t = std::time::Instant::now();
        let hits = v.search("10.0.7.").unwrap();
        let took = t.elapsed();
        assert_eq!(hits.len(), 250);
        assert!(took < Duration::from_secs(2), "search of 3000 entries took {took:?}");
    }

    #[test]
    fn importing_a_pack_is_idempotent_and_renames_clashes() {
        let mut v = Vault::in_memory("t").unwrap();
        let base = v.list_templates().unwrap().len();
        let pack = vec![Template::new("ping", "ping -c1 {host}"), Template::new("dig", "dig {name}")];
        assert_eq!(v.import_templates(&pack, "net").unwrap(), ImportReport { added: 2, skipped: 0 });
        assert_eq!(v.list_templates().unwrap().len(), base + 2);
        assert_eq!(
            v.import_templates(&pack, "net").unwrap(),
            ImportReport { added: 0, skipped: 2 },
            "second import changes nothing"
        );
        // same name, different pattern: kept side by side, never overwritten
        let clash = vec![Template::new("ping", "ping -c5 {host}")];
        assert_eq!(v.import_templates(&clash, "net").unwrap().added, 1);
        let names: Vec<String> = v.list_templates().unwrap().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"ping".to_string()) && names.contains(&"ping (net)".to_string()));
        assert_eq!(v.import_templates(&[Template::new("ping", "ping -c9 {host}")], "net").unwrap().added, 1);
        assert!(v.list_templates().unwrap().iter().any(|t| t.name == "ping (net 2)"));
    }

    #[test]
    fn importing_past_the_cap_changes_nothing() {
        let mut v = Vault::in_memory("t").unwrap();
        let base = v.list_templates().unwrap().len();
        let many: Vec<Template> =
            (0..MAX_TEMPLATES).map(|i| Template::new(format!("t{i}"), format!("echo {i}"))).collect();
        assert!(matches!(v.import_templates(&many, "x"), Err(Error::TooManyTemplates(_))));
        assert_eq!(v.list_templates().unwrap().len(), base, "all or nothing");
    }

    #[test]
    fn save_without_path_errors() {
        let mut v = Vault::in_memory("t").unwrap();
        assert!(matches!(v.save(), Err(Error::NoPath)));
    }

    #[test]
    fn templates_seeded_and_crud() {
        let mut v = Vault::in_memory("t").unwrap();
        let seeded = v.list_templates().unwrap();
        assert_eq!(seeded.len(), default_templates().len());
        assert_eq!(seeded[0].name, "SSH connect");

        let id = v.add_template(&Template::new("ping", "ping -c1 {host}")).unwrap();
        let mut t = v.list_templates().unwrap().into_iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.variables(), ["host"]);
        t.pattern = "ping -c{n} {host}".into();
        v.update_template(&t).unwrap();
        assert_eq!(v.list_templates().unwrap().last().unwrap().variables(), ["n", "host"]);
        v.delete_template(id).unwrap();
        assert_eq!(v.list_templates().unwrap().len(), seeded.len());
        assert!(matches!(v.delete_template(id), Err(Error::NotFound { .. })));
    }
}
