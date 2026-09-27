/// One generic key/value pair on an entry. Secrets are visible by default;
/// `is_secret` only tells the UI that a mask toggle applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

impl Field {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), is_secret: false }
    }

    pub fn secret(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { is_secret: true, ..Self::new(key, value) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Assigned by the vault; ignored by `Vault::add_entry`.
    pub id: i64,
    pub label: String,
    /// Markdown-capable free text.
    pub notes: String,
    pub fields: Vec<Field>,
    pub tags: Vec<String>,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds.
    pub updated: i64,
}

impl Entry {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            id: 0,
            label: label.into(),
            notes: String::new(),
            fields: Vec::new(),
            tags: Vec::new(),
            created: 0,
            updated: 0,
        }
    }

    /// Case-insensitive search: every whitespace-separated word of `query` must
    /// appear somewhere in the label, notes, tags, or a field key/value.
    /// An empty query matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let hay = self.haystack();
        query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }

    fn haystack(&self) -> String {
        let mut h = String::new();
        let mut push = |s: &str| {
            h.push_str(&s.to_lowercase());
            h.push('\n');
        };
        push(&self.label);
        push(&self.notes);
        self.tags.iter().for_each(|t| push(t));
        for f in &self.fields {
            push(&f.key);
            push(&f.value);
        }
        h
    }

    /// First field with this key, for template fill-in.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }
}

/// Project (vault) metadata. Timestamps are unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub created: i64,
    pub last_opened: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dc() -> Entry {
        let mut e = Entry::new("DC01");
        e.notes = "kerberoast me".into();
        e.tags = vec!["AD".into()];
        e.fields = vec![Field::new("host", "10.10.10.5"), Field::secret("password", "Hunter2")];
        e
    }

    #[test]
    fn matches_everywhere_case_insensitive() {
        let e = dc();
        for q in ["dc01", "kerberoast", "ad", "10.10.10.5", "hunter2", "PASSWORD", ""] {
            assert!(e.matches(q), "{q}");
        }
        assert!(e.matches("dc01 10.10"), "words are ANDed");
        assert!(!e.matches("dc01 nope"));
        assert!(!e.matches("web01"));
    }
}
