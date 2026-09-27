//! Notes are markdown. Slint's styled text handles inline formatting (bold,
//! italic, code, links, strikethrough) but not headings, quotes, rules or
//! per-line breaks, so we split a note into blocks here and let Slint style
//! the inside of each block.
//!
//! One source line becomes one block (notes are usually line-per-fact, like
//! `ssh admin@10.10.10.5`, so soft line breaks must survive).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Heading(u8),
    Para,
    /// `level` is nesting depth (0 = top); `marker` is `•`, `1.`, `☐` or `☑`.
    Item {
        level: u8,
        marker: String,
    },
    Quote,
    Code,
    Rule,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    /// Inline markdown for Para/Item/Quote; raw text for Heading/Code.
    pub text: String,
}

fn block(kind: Kind, text: impl Into<String>) -> Block {
    Block { kind, text: text.into() }
}

fn is_rule(t: &str) -> bool {
    let compact: Vec<char> = t.chars().filter(|c| *c != ' ').collect();
    compact.len() >= 3 && ['-', '*', '_'].iter().any(|m| compact.iter().all(|c| c == m))
}

/// `- x`, `* x`, `+ x`, `1. x`, `1) x`, optionally `[ ]` / `[x]`; returns (indent, marker, rest).
fn parse_item(line: &str) -> Option<(usize, String, String)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    let (marker, rest): (String, &str) = if let Some(r) = ["- ", "* ", "+ "].iter().find_map(|p| t.strip_prefix(p)) {
        ("•".into(), r)
    } else {
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        let after = &t[digits.len()..];
        let r = after.strip_prefix(". ").or_else(|| after.strip_prefix(") "))?;
        if digits.is_empty() || digits.len() > 3 {
            return None;
        }
        (format!("{digits}."), r)
    };
    let (marker, rest) = if let Some(r) = rest.strip_prefix("[ ] ") {
        ("☐".to_string(), r)
    } else if let Some(r) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
        ("☑".to_string(), r)
    } else {
        (marker, rest)
    };
    Some((indent, marker, rest.to_string()))
}

/// Wrap bare `http(s)://` URLs as markdown links so they are clickable.
pub fn autolink(line: &str) -> String {
    if line.contains("](") || line.contains('`') {
        return line.to_string();
    }
    line.split(' ')
        .map(|w| {
            let trimmed = w.trim_end_matches(['.', ',', ')', ';']);
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                format!("[{trimmed}]({trimmed}){}", &w[trimmed.len()..])
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A note is rendered up to this many characters / blocks; the rest is cut with a notice.
/// (Stops a pasted multi-megabyte blob from freezing the preview.)
pub const MAX_RENDER_CHARS: usize = 20_000;
pub const MAX_RENDER_BLOCKS: usize = 1_500;

pub fn parse(src: &str) -> Vec<Block> {
    let mut cut = src.len() > MAX_RENDER_CHARS;
    let src = if cut {
        // cut on a character boundary
        let end = (0..=MAX_RENDER_CHARS).rev().find(|i| src.is_char_boundary(*i)).unwrap_or(0);
        &src[..end]
    } else {
        src
    };
    let mut out = parse_all(src);
    if out.len() > MAX_RENDER_BLOCKS {
        out.truncate(MAX_RENDER_BLOCKS);
        cut = true;
    }
    if cut {
        out.push(block(Kind::Gap, ""));
        out.push(block(
            Kind::Para,
            "… (note continues; the preview shows the first part only. Switch to edit to see all of it.)",
        ));
    }
    out
}

fn parse_all(src: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let mut table: Vec<&str> = Vec::new();

    let flush_table = |out: &mut Vec<Block>, table: &mut Vec<&str>| {
        if !table.is_empty() {
            out.push(block(Kind::Code, table.join("\n")));
            table.clear();
        }
    };

    for line in src.lines() {
        // fenced code: everything up to the closing fence is literal
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(lines) => out.push(block(Kind::Code, lines.join("\n"))),
                None => code = Some(Vec::new()),
            }
            continue;
        }
        if let Some(lines) = code.as_mut() {
            lines.push(line);
            continue;
        }
        let t = line.trim();
        // pipe tables stay monospace so the author's alignment survives
        if t.starts_with('|') && t.ends_with('|') && t.len() > 1 {
            table.push(line);
            continue;
        }
        flush_table(&mut out, &mut table);

        if t.is_empty() {
            if !matches!(out.last().map(|b| &b.kind), Some(Kind::Gap) | None) {
                out.push(block(Kind::Gap, ""));
            }
        } else if let Some(h) = t.strip_prefix('#').map(|r| (1 + r.chars().take_while(|&c| c == '#').count(), r)) {
            let (level, rest) = h;
            let text = rest.trim_start_matches('#');
            if level <= 6 && text.starts_with(' ') {
                out.push(block(Kind::Heading(level.min(3) as u8), text.trim()));
            } else {
                out.push(block(Kind::Para, autolink(t)));
            }
        } else if is_rule(t) {
            out.push(block(Kind::Rule, ""));
        } else if let Some(rest) = t.strip_prefix('>') {
            out.push(block(Kind::Quote, autolink(rest.trim_start())));
        } else if let Some((indent, marker, rest)) = parse_item(line) {
            out.push(block(Kind::Item { level: (indent / 2).min(4) as u8, marker }, autolink(&rest)));
        } else {
            out.push(block(Kind::Para, autolink(t)));
        }
    }
    flush_table(&mut out, &mut table);
    if let Some(lines) = code {
        // unterminated fence: still show what was written
        out.push(block(Kind::Code, lines.join("\n")));
    }
    while matches!(out.last().map(|b| &b.kind), Some(Kind::Gap)) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Kind> {
        parse(src).into_iter().map(|b| b.kind).collect()
    }

    #[test]
    fn headings_and_paragraphs() {
        let b = parse("# Domain controller\n## creds\n#### deep\nplain line\n#nothashtag");
        assert_eq!(b[0], block(Kind::Heading(1), "Domain controller"));
        assert_eq!(b[1], block(Kind::Heading(2), "creds"));
        assert_eq!(b[2].kind, Kind::Heading(3), "levels beyond 3 clamp");
        assert_eq!(b[3], block(Kind::Para, "plain line"));
        assert_eq!(b[4].kind, Kind::Para, "#hashtag without a space is text");
    }

    #[test]
    fn every_line_is_its_own_block_so_line_breaks_survive() {
        let b = parse("ssh admin@10.10.10.5\nnmap -sC 10.10.10.5\n\nnext");
        assert_eq!(b.len(), 4);
        assert_eq!(b[2].kind, Kind::Gap);
        assert_eq!(parse("a\n\n\n\nb").len(), 3, "runs of blank lines collapse to one gap");
        assert!(parse("a\n\n\n").iter().all(|b| b.kind != Kind::Gap), "no trailing gap");
        assert!(parse("").is_empty());
    }

    #[test]
    fn lists_numbers_and_checkboxes() {
        let b = parse("- one\n* two\n  - nested\n1. first\n12) twelfth\n- [ ] todo\n- [x] done\n- [X] Done");
        let items: Vec<(u8, String, String)> = b
            .iter()
            .map(|b| match &b.kind {
                Kind::Item { level, marker } => (*level, marker.clone(), b.text.clone()),
                k => panic!("not an item: {k:?}"),
            })
            .collect();
        assert_eq!(items[0], (0, "•".into(), "one".into()));
        assert_eq!(items[2], (1, "•".into(), "nested".into()));
        assert_eq!(items[3].1, "1.");
        assert_eq!(items[4].1, "12.");
        assert_eq!(items[5], (0, "☐".into(), "todo".into()));
        assert_eq!(items[6].1, "☑");
        assert_eq!(items[7].1, "☑");
        assert_eq!(kinds("10.10.10.5 is the dc"), vec![Kind::Para], "an IP is not a list");
        assert_eq!(kinds("-5 degrees"), vec![Kind::Para]);
    }

    #[test]
    fn code_quote_rule_and_table() {
        let b = parse("```\nsudo nmap -sV\n# not a heading\n```\n> note this\n---\n| a | b |\n|---|---|\n| 1 | 2 |");
        assert_eq!(b[0], block(Kind::Code, "sudo nmap -sV\n# not a heading"));
        assert_eq!(b[1], block(Kind::Quote, "note this"));
        assert_eq!(b[2].kind, Kind::Rule);
        assert_eq!(b[3].kind, Kind::Code, "table stays monospace");
        assert!(b[3].text.contains("| 1 | 2 |"));
        assert_eq!(parse("```\nunterminated").len(), 1, "unterminated fence still shows");
        assert_eq!(kinds("***"), vec![Kind::Rule]);
        assert_eq!(kinds("- - -"), vec![Kind::Rule]);
    }

    #[test]
    fn huge_notes_are_cut_not_frozen_on() {
        let big = "line of text\n".repeat(50_000);
        let t = std::time::Instant::now();
        let b = parse(&big);
        assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());
        assert!(b.len() <= MAX_RENDER_BLOCKS + 2);
        assert!(matches!(b.last().map(|b| &b.kind), Some(Kind::Para)) && b.last().unwrap().text.contains("continues"));
        // multi-byte text right at the limit must not split a character
        let wide = "名".repeat(MAX_RENDER_CHARS);
        let _ = parse(&wide);
        assert!(parse("short").iter().all(|b| !b.text.contains("continues")));
    }

    #[test]
    fn markdown_parser_survives_garbage() {
        let alphabet = [
            "#", "##", " ", "\n", "-", "*", "1.", "> ", "```", "|", "[", "](", ")", "http://", "https://", "`", "名",
            "é", "\u{202e}", "[ ]", "[x]", "---", "\t", "\r",
        ];
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..20_000 {
            let mut s = String::new();
            for _ in 0..24 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                s.push_str(alphabet[(x % alphabet.len() as u64) as usize]);
            }
            let _ = parse(&s);
            let _ = autolink(&s);
        }
    }

    #[test]
    fn urls_become_links_but_existing_links_and_code_are_left_alone() {
        assert_eq!(autolink("see https://a.b/c, ok"), "see [https://a.b/c](https://a.b/c), ok");
        assert_eq!(autolink("[x](https://a.b)"), "[x](https://a.b)");
        assert_eq!(autolink("`curl https://a.b`"), "`curl https://a.b`");
        assert_eq!(autolink("no links here"), "no links here");
    }
}
