//! Template packs: bundles of command templates (`{placeholder}` patterns) that
//! can be added to a vault. A pack is plain text; nothing in it is ever run.
//!
//! ```text
//! id = recon-enum
//! name = Recon and enumeration
//! author = you            (optional)
//! license = MIT           (optional)
//! description = ...       (optional)
//!
//! [templates]
//! nmap quick = nmap -sC -sV {ip}
//! ```

use std::fmt;

use crate::template::Template;
use crate::theme::valid_id;

pub const MAX_PACK_BYTES: usize = 64 * 1024;
pub const MAX_TEMPLATES_PER_PACK: usize = 200;
pub const MAX_NAME_CHARS: usize = 60;
pub const MAX_PATTERN_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub description: String,
    pub templates: Vec<Template>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    TooBig(usize),
    Missing(&'static str),
    Bad { line: usize, why: String },
    TooMany,
    Empty,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::TooBig(n) => write!(f, "pack is {n} bytes; the limit is {MAX_PACK_BYTES}"),
            PackError::Missing(k) => write!(f, "pack is missing \"{k}\""),
            PackError::Bad { line, why } => write!(f, "line {line}: {why}"),
            PackError::TooMany => write!(f, "more than {MAX_TEMPLATES_PER_PACK} templates"),
            PackError::Empty => write!(f, "pack has no templates"),
        }
    }
}

impl std::error::Error for PackError {}

/// Characters that make text look different from what it is: control characters
/// (except none), bidirectional overrides, zero-width and other invisible formatting.
pub fn is_deceptive(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{17B4}' | '\u{17B5}'
            | '\u{180B}'..='\u{180F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
            | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E0FFF}')
}

fn bad(line: usize, why: impl Into<String>) -> PackError {
    PackError::Bad { line, why: why.into() }
}

/// Is this a well-formed `{name}` placeholder body?
fn placeholder_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Pack {
    pub fn parse(text: &str) -> Result<Pack, PackError> {
        if text.len() > MAX_PACK_BYTES {
            return Err(PackError::TooBig(text.len()));
        }
        let (mut id, mut name) = (None, None);
        let (mut author, mut license, mut description) = (String::new(), String::new(), String::new());
        let mut templates: Vec<Template> = Vec::new();
        let mut in_templates = false;

        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line == "[templates]" {
                in_templates = true;
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(bad(n, "expected `key = value`"));
            };
            let (k, v) = (k.trim(), v.trim());
            if in_templates {
                if k.is_empty() || k.chars().count() > MAX_NAME_CHARS {
                    return Err(bad(n, format!("template name must be 1-{MAX_NAME_CHARS} characters")));
                }
                if v.is_empty() || v.chars().count() > MAX_PATTERN_CHARS {
                    return Err(bad(n, format!("pattern must be 1-{MAX_PATTERN_CHARS} characters")));
                }
                if k.chars().chain(v.chars()).any(is_deceptive) {
                    return Err(bad(n, "contains control or invisible characters"));
                }
                let t = Template::new(k, v);
                // every {...} must be a clean placeholder; stray braces are literal text
                for var in t.variables() {
                    if !placeholder_ok(&var) {
                        return Err(bad(n, format!("odd placeholder {{{var}}}")));
                    }
                }
                if t.variables().len() > 12 {
                    return Err(bad(n, "more than 12 placeholders"));
                }
                templates.push(t);
                if templates.len() > MAX_TEMPLATES_PER_PACK {
                    return Err(PackError::TooMany);
                }
                continue;
            }
            let field = |max: usize| -> Result<String, PackError> {
                if v.chars().count() > max || v.chars().any(is_deceptive) {
                    return Err(bad(n, format!("\"{k}\" must be plain text of at most {max} characters")));
                }
                Ok(v.to_string())
            };
            match k {
                "id" => {
                    if !valid_id(v) {
                        return Err(bad(n, "id: use 1-32 characters of a-z, 0-9 and -"));
                    }
                    id = Some(v.to_string());
                }
                "name" => {
                    let s = field(60)?;
                    if s.is_empty() {
                        return Err(bad(n, "name is empty"));
                    }
                    name = Some(s);
                }
                "author" => author = field(60)?,
                "license" => license = field(40)?,
                "description" => description = field(300)?,
                _ => {} // unknown header keys are ignored
            }
        }
        if templates.is_empty() {
            return Err(PackError::Empty);
        }
        Ok(Pack {
            id: id.ok_or(PackError::Missing("id"))?,
            name: name.ok_or(PackError::Missing("name"))?,
            author,
            license,
            description,
            templates,
        })
    }
}

/// Packs shipped in the binary.
pub fn builtin() -> Vec<Pack> {
    [
        include_str!("../packs/network-engineer.pack"),
        include_str!("../packs/recon-enum.pack"),
        include_str!("../packs/transfer-tunnel.pack"),
        include_str!("../packs/windows-ad.pack"),
    ]
    .iter()
    .map(|t| Pack::parse(t).expect("built-in packs are valid"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "id = mine\nname = Mine\nlicense = MIT\n\n[templates]\nping = ping -c1 {host}\nssh = ssh {user}@{host} -p {port}\n";

    #[test]
    fn builtin_packs_are_valid_unique_and_useful() {
        let packs = builtin();
        assert_eq!(packs.len(), 4);
        let mut ids = std::collections::HashSet::new();
        for p in &packs {
            assert!(ids.insert(p.id.clone()), "duplicate {}", p.id);
            assert!(p.templates.len() >= 10, "{} has {}", p.id, p.templates.len());
            assert!(!p.license.is_empty() && !p.description.is_empty());
            let mut names = std::collections::HashSet::new();
            for t in &p.templates {
                assert!(names.insert(t.name.clone()), "{}: duplicate template {}", p.id, t.name);
                assert!(!t.variables().is_empty() || t.pattern.len() < 40, "{}: {} has no placeholders", p.id, t.name);
            }
        }
    }

    #[test]
    fn stray_braces_are_literal_text_not_placeholders() {
        let p = Pack::parse("id = a\nname = A\n[templates]\nx = awk '{print $1}' {file} {b c}\n").unwrap();
        assert_eq!(p.templates[0].variables(), ["file"]);
    }

    #[test]
    fn parses_a_pack() {
        let p = Pack::parse(OK).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str(), p.license.as_str()), ("mine", "Mine", "MIT"));
        assert_eq!(p.templates.len(), 2);
        assert_eq!(p.templates[1].variables(), ["user", "host", "port"]);
        assert_eq!(p.templates[0].pattern, "ping -c1 {host}");
        // '=' inside a pattern is kept
        let q = Pack::parse("id = a\nname = A\n[templates]\nx = curl -H 'A: b=c' {url}\n").unwrap();
        assert_eq!(q.templates[0].pattern, "curl -H 'A: b=c' {url}");
    }

    #[test]
    fn rejects_bad_packs() {
        let mut too_many = String::from("id = a\nname = A\n[templates]\n");
        for i in 0..=MAX_TEMPLATES_PER_PACK {
            too_many.push_str(&format!("t{i} = echo {i}\n"));
        }
        for (text, needle) in [
            ("id = a\nname = A\n".to_string(), "no templates"),
            ("name = A\n[templates]\nx = y\n".to_string(), "missing \"id\""),
            ("id = a\n[templates]\nx = y\n".to_string(), "missing \"name\""),
            ("id = A_B\nname = A\n[templates]\nx = y\n".to_string(), "id:"),
            ("id = a\nname = A\n[templates]\njust text\n".to_string(), "key = value"),
            ("id = a\nname = A\n[templates]\n = y\n".to_string(), "template name"),
            ("id = a\nname = A\n[templates]\nx =\n".to_string(), "pattern"),
            (format!("id = a\nname = A\n[templates]\nx = {}\n", "y".repeat(MAX_PATTERN_CHARS + 1)), "pattern"),
            (
                "id = a\nname = A\n[templates]\nx = {a}{b}{c}{d}{e}{f}{g}{h}{i}{j}{k}{l}{m}\n".to_string(),
                "12 placeholders",
            ),
            (too_many, "more than"),
            ("x".repeat(MAX_PACK_BYTES + 1), "limit"),
        ] {
            let err = Pack::parse(&text).unwrap_err().to_string();
            assert!(err.contains(needle), "{needle:?} not in {err:?}");
        }
    }

    #[test]
    fn deceptive_characters_are_refused_everywhere() {
        // a right-to-left override can make a command display differently from what is copied
        for evil in ["\u{202e}", "\u{200b}", "\u{feff}", "\u{2066}", "\u{0007}", "\u{e0041}", "\u{00ad}"] {
            for template in [
                format!("id = a\nname = A\n[templates]\nx = echo ok{evil}rm\n"),
                format!("id = a\nname = A\n[templates]\nna{evil}me = echo ok\n"),
                format!("id = a\nname = A{evil}\n[templates]\nx = echo ok\n"),
                format!("id = a\nname = A\ndescription = hi{evil}\n[templates]\nx = echo ok\n"),
            ] {
                assert!(Pack::parse(&template).is_err(), "accepted {evil:?}");
            }
        }
        assert!(
            Pack::parse("id = a\nname = Ünïcode ok 名前\n[templates]\nx = echo héllo\n").is_ok(),
            "normal unicode is fine"
        );
    }

    #[test]
    fn arbitrary_text_never_panics() {
        for s in [
            "[templates]",
            "=",
            "[templates]\n=",
            "\u{0}",
            "id=\nname=\n[templates]\n=x",
            &"[templates]\n".repeat(100),
            "名 = 値\n[templates]\n名 = 値",
        ] {
            let _ = Pack::parse(s);
        }
    }
}
