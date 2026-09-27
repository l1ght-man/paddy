use std::collections::HashMap;

/// Command template with `{placeholder}` slots. A placeholder is `{` + an
/// identifier (`[A-Za-z_][A-Za-z0-9_]*`) + `}`; any other brace is literal, so
/// JSON bodies like `{"a":1}` pass through untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Assigned by the vault; ignored by `Vault::add_template`.
    pub id: i64,
    pub name: String,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("missing values for: {}", .missing.join(", "))]
pub struct RenderError {
    pub missing: Vec<String>,
}

enum Seg<'a> {
    Lit(&'a str),
    Var(&'a str),
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn segments(pattern: &str) -> Vec<Seg<'_>> {
    let bytes = pattern.as_bytes();
    let mut out = Vec::new();
    let (mut lit_start, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] == b'{' && bytes.get(i + 1).is_some_and(|&b| is_ident_start(b)) {
            let name_start = i + 1;
            let mut j = name_start;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if bytes.get(j) == Some(&b'}') {
                if lit_start < i {
                    out.push(Seg::Lit(&pattern[lit_start..i]));
                }
                out.push(Seg::Var(&pattern[name_start..j]));
                i = j + 1;
                lit_start = i;
                continue;
            }
        }
        i += 1;
    }
    if lit_start < bytes.len() {
        out.push(Seg::Lit(&pattern[lit_start..]));
    }
    out
}

impl Template {
    pub fn new(name: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self { id: 0, name: name.into(), pattern: pattern.into() }
    }

    /// Variable names in first-appearance order, deduplicated.
    pub fn variables(&self) -> Vec<String> {
        let mut vars: Vec<String> = Vec::new();
        for seg in segments(&self.pattern) {
            if let Seg::Var(v) = seg {
                if !vars.iter().any(|x| x == v) {
                    vars.push(v.to_string());
                }
            }
        }
        vars
    }

    /// Fill every placeholder; errors listing the variables with no value.
    pub fn render(&self, values: &HashMap<String, String>) -> Result<String, RenderError> {
        let missing: Vec<String> = self.variables().into_iter().filter(|v| !values.contains_key(v)).collect();
        if !missing.is_empty() {
            return Err(RenderError { missing });
        }
        Ok(self.render_partial(values))
    }

    /// Fill what is known, leave unknown placeholders as `{name}` (live preview).
    pub fn render_partial(&self, values: &HashMap<String, String>) -> String {
        let mut out = String::with_capacity(self.pattern.len());
        for seg in segments(&self.pattern) {
            match seg {
                Seg::Lit(s) => out.push_str(s),
                Seg::Var(v) => match values.get(v) {
                    Some(val) => out.push_str(val),
                    None => {
                        out.push('{');
                        out.push_str(v);
                        out.push('}');
                    }
                },
            }
        }
        out
    }
}

/// Generic command shapes seeded into every new vault.
pub fn default_templates() -> Vec<Template> {
    [
        ("SSH connect", "ssh {user}@{host} -p {port}"),
        ("SCP pull", "scp -P {port} {user}@{host}:{remote_path} {local_path}"),
        ("SCP push", "scp -P {port} {local_path} {user}@{host}:{remote_path}"),
        ("HTTP GET", "curl -i http://{host}:{port}{path}"),
        (
            "HTTP POST (JSON)",
            "curl -i -X POST http://{host}:{port}{path} -H 'Content-Type: application/json' -d '{body}'",
        ),
    ]
    .into_iter()
    .map(|(n, p)| Template::new(n, p))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn variables_ordered_and_deduped() {
        let t = Template::new("t", "ssh {user}@{host} -p {port} # {host}");
        assert_eq!(t.variables(), ["user", "host", "port"]);
    }

    #[test]
    fn non_placeholder_braces_are_literal() {
        let t = Template::new("t", r#"curl -d '{"a":1}' {host} {} {1x} {a b} {open"#);
        assert_eq!(t.variables(), ["host"]);
        let out = t.render(&vals(&[("host", "h")])).unwrap();
        assert_eq!(out, r#"curl -d '{"a":1}' h {} {1x} {a b} {open"#);
    }

    #[test]
    fn render_reports_missing() {
        let t = Template::new("t", "ssh {user}@{host}");
        let err = t.render(&vals(&[("host", "10.0.0.1")])).unwrap_err();
        assert_eq!(err.missing, ["user"]);
    }

    #[test]
    fn render_partial_keeps_unknown() {
        let t = Template::new("t", "ssh {user}@{host}");
        assert_eq!(t.render_partial(&vals(&[("host", "h")])), "ssh {user}@h");
    }

    #[test]
    fn defaults_parse() {
        for t in default_templates() {
            assert!(!t.variables().is_empty(), "{}", t.name);
        }
    }
}
