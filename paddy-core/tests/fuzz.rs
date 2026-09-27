//! Cheap fuzzing: every parser that touches untrusted or user-edited text gets
//! random and adversarial input. The release build uses `panic = abort`, so a
//! panic here would be a crash in the field.
//!
//! Deterministic (fixed seed) so failures reproduce. Raise the count for a longer soak:
//!   FUZZ_ITER=500000 cargo test -p paddy-core --test fuzz --release

use std::collections::BTreeMap;
use std::fs;

use paddy_core::{parse_chord_list, Chord, Config, Keymap, Pack, Rgb, Template, Theme, Vault};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Fragments that tend to break parsers: separators, unicode edge cases, huge repeats.
const NASTY: &[&str] = &[
    "=",
    "==",
    "\n",
    "\r\n",
    "\0",
    " ",
    "\t",
    "#",
    "[templates]",
    "id",
    "name",
    "dark",
    "bg",
    "key.",
    "ctrl+",
    "shift+",
    "alt+",
    "+",
    ",",
    "{",
    "}",
    "{}",
    "{a}",
    "{{",
    "}}",
    "{a b}",
    "é",
    "İ",
    "ß",
    "K",
    "名前",
    "😀",
    "\u{202e}",
    "\u{200b}",
    "\u{feff}",
    "\u{0301}",
    "\u{e0041}",
    "#ffffff",
    "#GGGGGG",
    "true",
    "false",
    "-",
    "--",
    "\\",
    "'",
    "\"",
    "%s",
    "%n",
    "../",
    "..\\",
    "/etc/passwd",
    "null",
    "\u{ffff}",
    "\u{10ffff}",
];

fn junk(rng: &mut Rng) -> String {
    let mut s = String::new();
    for _ in 0..rng.below(24) {
        match rng.below(6) {
            0 => s.push_str(NASTY[rng.below(NASTY.len())]),
            1 => s.push(char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('?')),
            2 => s.push(char::from(rng.below(128) as u8)),
            3 => s.push_str(&NASTY[rng.below(NASTY.len())].repeat(rng.below(40))),
            4 => s.push_str(&format!("{}\n", NASTY[rng.below(NASTY.len())])),
            _ => s.push_str(&"a".repeat(rng.below(3000))),
        }
    }
    s
}

fn iterations() -> usize {
    std::env::var("FUZZ_ITER").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000)
}

#[test]
fn text_parsers_never_panic() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..iterations() {
        let s = junk(&mut rng);
        let _ = Theme::parse(&s);
        let _ = Pack::parse(&s);
        let _ = Chord::parse(&s);
        let _ = parse_chord_list(&s);
        let _ = Rgb::parse(&s);
        let t = Template::new("x", s.as_str());
        let values: std::collections::HashMap<String, String> =
            t.variables().into_iter().map(|v| (v, junk(&mut rng))).collect();
        let _ = t.render(&values);
        let _ = t.render_partial(&values);
    }
}

#[test]
fn keymap_and_config_survive_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    for i in 0..iterations() / 10 {
        // random override tables
        let mut o = BTreeMap::new();
        for _ in 0..rng.below(6) {
            o.insert(junk(&mut rng), junk(&mut rng));
        }
        let (map, _) = Keymap::from_overrides(&o);
        let _ = map.overrides();
        // random settings files, and a save/load round trip of whatever came out
        fs::write(&path, junk(&mut rng)).unwrap();
        let cfg = Config::load(&path);
        if i % 20 == 0 {
            cfg.save(&path).unwrap();
            let again = Config::load(&path);
            assert_eq!(again.font_size, cfg.font_size);
        }
    }
}

#[test]
fn vault_text_fields_round_trip_whatever_the_content() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut v = Vault::in_memory("fuzz").unwrap();
    for _ in 0..300 {
        let mut e = paddy_core::Entry::new(junk(&mut rng));
        e.notes = junk(&mut rng);
        e.tags = vec![junk(&mut rng), junk(&mut rng)];
        e.fields = vec![
            paddy_core::Field::new(junk(&mut rng), junk(&mut rng)),
            paddy_core::Field::secret(junk(&mut rng), junk(&mut rng)),
        ];
        let id = v.add_entry(&e).unwrap();
        let got = v.get_entry(id).unwrap();
        assert_eq!(got.label, e.label);
        assert_eq!(got.notes, e.notes);
        assert_eq!(got.fields, e.fields);
        // search must handle any query and any content
        let _ = v.search(&junk(&mut rng)).unwrap();
    }
}
