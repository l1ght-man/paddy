//! Customizable key bindings: the actions, their default chords, parsing and
//! formatting of chords, and conflict checking. UI-agnostic: the frontend turns
//! a `Chord` into whatever its toolkit needs.
//!
//! Stored in the settings file as `key.<action> = ctrl+n` (several chords
//! separated by `, `; an empty value unbinds the action). Only differences from
//! the defaults are written, so new defaults reach existing users.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    NewEntry,
    Save,
    DeleteEntry,
    TogglePreview,
    Find,
    FindAll,
    Vaults,
    Templates,
    Theme,
    Settings,
    Keys,
    Packs,
    Help,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

impl Action {
    pub const ALL: [Action; 16] = [
        Action::NewEntry,
        Action::Save,
        Action::DeleteEntry,
        Action::TogglePreview,
        Action::Find,
        Action::FindAll,
        Action::Vaults,
        Action::Templates,
        Action::Theme,
        Action::Settings,
        Action::Keys,
        Action::Packs,
        Action::Help,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
    ];

    /// Stable name used in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Action::NewEntry => "new_entry",
            Action::Save => "save",
            Action::DeleteEntry => "delete_entry",
            Action::TogglePreview => "toggle_preview",
            Action::Find => "find",
            Action::FindAll => "find_all",
            Action::Vaults => "vaults",
            Action::Templates => "templates",
            Action::Theme => "theme",
            Action::Settings => "settings",
            Action::Keys => "keys",
            Action::Packs => "packs",
            Action::Help => "help",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::ZoomReset => "zoom_reset",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }

    /// Short human name.
    pub fn label(self) -> &'static str {
        match self {
            Action::NewEntry => "new entry",
            Action::Save => "save vault",
            Action::DeleteEntry => "delete entry",
            Action::TogglePreview => "notes: edit / preview",
            Action::Find => "search this vault",
            Action::FindAll => "search all vaults",
            Action::Vaults => "switch or create vault",
            Action::Templates => "templates",
            Action::Theme => "light / dark",
            Action::Settings => "settings tab",
            Action::Keys => "keys tab",
            Action::Packs => "packs tab (themes, fonts, templates)",
            Action::Help => "shortcut help",
            Action::ZoomIn => "bigger text",
            Action::ZoomOut => "smaller text",
            Action::ZoomReset => "reset text size",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Action::NewEntry | Action::Save | Action::DeleteEntry | Action::TogglePreview => "entries",
            Action::Find | Action::FindAll | Action::Vaults => "find and switch",
            _ => "app",
        }
    }

    /// Default chords, in the file syntax.
    pub fn defaults(self) -> &'static [&'static str] {
        match self {
            Action::NewEntry => &["ctrl+n"],
            Action::Save => &["ctrl+s"],
            Action::DeleteEntry => &["ctrl+d"],
            Action::TogglePreview => &["ctrl+e"],
            Action::Find => &["ctrl+f"],
            Action::FindAll => &["ctrl+g"],
            Action::Vaults => &["ctrl+o"],
            Action::Templates => &["ctrl+t"],
            Action::Theme => &["ctrl+l"],
            Action::Settings => &["ctrl+,"],
            Action::Keys => &["ctrl+k"],
            Action::Packs => &["ctrl+p"],
            Action::Help => &["f1"],
            Action::ZoomIn => &["ctrl+="],
            Action::ZoomOut => &["ctrl+-"],
            Action::ZoomReset => &["ctrl+0"],
        }
    }

    /// Does the action still work while a popup (dialog, picker, help) is open?
    pub fn works_in_overlay(self) -> bool {
        matches!(
            self,
            Action::Save | Action::Theme | Action::Help | Action::ZoomIn | Action::ZoomOut | Action::ZoomReset
        )
    }
}

/// One key combination. `key` is canonical: a single lowercase character
/// (`n`, `=`, `,`) or a name (`enter`, `esc`, `up`, `f1`, `pageup`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String,
}

const NAMED_KEYS: &[&str] = &[
    "enter",
    "esc",
    "tab",
    "space",
    "backspace",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
];

fn is_function_key(k: &str) -> bool {
    k.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n))
}

/// Map spellings people might type to the canonical key name.
fn canonical_key(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    let k = match lower.as_str() {
        "return" => "enter",
        "escape" => "esc",
        "del" => "delete",
        "ins" => "insert",
        "pgup" | "page_up" | "pageup" => "pageup",
        "pgdn" | "pgdown" | "page_down" | "pagedown" => "pagedown",
        "arrowup" | "uparrow" => "up",
        "arrowdown" | "downarrow" => "down",
        "arrowleft" | "leftarrow" => "left",
        "arrowright" | "rightarrow" => "right",
        "plus" => "+",
        "minus" | "dash" | "hyphen" => "-",
        "comma" => ",",
        "period" | "dot" => ".",
        "equal" | "equals" => "=",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" => "'",
        "backquote" | "backtick" | "grave" => "`",
        other => other,
    };
    if NAMED_KEYS.contains(&k) || is_function_key(k) || k.chars().count() == 1 {
        Some(k.to_string())
    } else {
        None
    }
}

impl Chord {
    /// Parse `ctrl+shift+f`, `Ctrl+,`, `ctrl++`, `f1`. Modifiers may come in any
    /// order and case; the key is whatever remains after the modifiers.
    pub fn parse(s: &str) -> Result<Chord, String> {
        let mut rest = s.trim();
        let mut c = Chord::default();
        // Modifier prefixes are ASCII; compare bytes so odd Unicode can never break a char boundary.
        const PREFIXES: [(&str, u8); 8] = [
            ("ctrl+", 0),
            ("control+", 0),
            ("alt+", 1),
            ("shift+", 2),
            ("meta+", 3),
            ("super+", 3),
            ("cmd+", 3),
            ("win+", 3),
        ];
        'outer: loop {
            for (prefix, which) in PREFIXES {
                let Some(head) = rest.get(..prefix.len()) else { continue };
                // "ctrl+" alone means the key is missing; a lone modifier is not a chord.
                if head.eq_ignore_ascii_case(prefix) && rest.len() > prefix.len() {
                    match which {
                        0 => c.ctrl = true,
                        1 => c.alt = true,
                        2 => c.shift = true,
                        _ => c.meta = true,
                    }
                    rest = &rest[prefix.len()..];
                    continue 'outer;
                }
            }
            break;
        }
        if rest.is_empty() {
            return Err(format!("\"{s}\" has no key"));
        }
        c.key = canonical_key(rest).ok_or_else(|| format!("unknown key \"{rest}\""))?;
        Ok(c)
    }

    /// Bindings must not steal plain typing: they need Ctrl, Alt or Meta, unless
    /// they are a function key. Esc/Enter/Tab/arrows stay structural.
    pub fn validate_binding(&self) -> Result<(), String> {
        if is_function_key(&self.key) || self.ctrl || self.alt || self.meta {
            return Ok(());
        }
        Err(format!("\"{self}\" would break typing: add Ctrl, Alt or Meta (function keys are fine alone)"))
    }

    /// Readable form for the UI: `Ctrl+Shift+F`, `Ctrl+,`, `F1`, `Esc`.
    pub fn pretty(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if self.meta {
            parts.push("Meta".into());
        }
        parts.push(match self.key.as_str() {
            "enter" => "Enter".into(),
            "esc" => "Esc".into(),
            "tab" => "Tab".into(),
            "space" => "Space".into(),
            "backspace" => "Backspace".into(),
            "delete" => "Delete".into(),
            "insert" => "Insert".into(),
            "home" => "Home".into(),
            "end" => "End".into(),
            "pageup" => "PgUp".into(),
            "pagedown" => "PgDn".into(),
            "up" => "↑".into(),
            "down" => "↓".into(),
            "left" => "←".into(),
            "right" => "→".into(),
            k if is_function_key(k) => k.to_uppercase(),
            k => k.to_uppercase(),
        });
        parts.join("+")
    }
}

/// File syntax: lowercase, modifiers in fixed order.
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (on, name) in [(self.ctrl, "ctrl"), (self.alt, "alt"), (self.shift, "shift"), (self.meta, "meta")] {
            if on {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

/// Parse a comma-separated chord list, e.g. `ctrl+=, ctrl++`. Commas are also a
/// valid key (`ctrl+,`), so a comma directly after `+` or at the start belongs to the key.
pub fn parse_chord_list(s: &str) -> Result<Vec<Chord>, String> {
    let mut chords = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        let is_separator = c == ',' && !cur.trim().is_empty() && !cur.trim_end().ends_with('+');
        if is_separator {
            chords.push(Chord::parse(&cur)?);
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !cur.trim().is_empty() {
        chords.push(Chord::parse(&cur)?);
    }
    Ok(chords)
}

pub fn format_chord_list(chords: &[Chord]) -> String {
    chords.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
}

/// Why a binding was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    Invalid(String),
    Taken { chord: Chord, by: Action },
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindError::Invalid(m) => write!(f, "{m}"),
            BindError::Taken { chord, by } => write!(f, "{} is already used by \"{}\"", chord.pretty(), by.label()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let bindings = Action::ALL
            .into_iter()
            .map(|a| (a, a.defaults().iter().map(|d| Chord::parse(d).expect("default chord parses")).collect()))
            .collect();
        Self { bindings }
    }
}

impl Keymap {
    /// Defaults with the user's overrides (`key.<id>` -> chord list) applied.
    /// Bad lines are skipped and reported in the returned warnings; an override
    /// that would clash with an earlier binding is dropped too.
    pub fn from_overrides(overrides: &BTreeMap<String, String>) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut warnings = Vec::new();
        // Unbind everything that is overridden first, so swapping two actions' keys works.
        let mut wanted: Vec<(Action, Vec<Chord>)> = Vec::new();
        for (id, value) in overrides {
            let Some(action) = Action::from_id(id) else {
                warnings.push(format!("key.{id}: unknown action"));
                continue;
            };
            match parse_chord_list(value) {
                Ok(chords) => match chords.iter().try_for_each(Chord::validate_binding) {
                    Ok(()) => wanted.push((action, chords)),
                    Err(e) => warnings.push(format!("key.{id}: {e}")),
                },
                Err(e) => warnings.push(format!("key.{id}: {e}")),
            }
        }
        for (a, _) in &wanted {
            map.bindings.insert(*a, Vec::new());
        }
        for (action, chords) in wanted {
            for chord in chords {
                match map.owner(&chord) {
                    Some(other) if other != action => warnings.push(format!(
                        "key.{}: {} is already used by \"{}\"",
                        action.id(),
                        chord.pretty(),
                        other.label()
                    )),
                    _ => map.bindings.entry(action).or_default().push(chord),
                }
            }
        }
        (map, warnings)
    }

    pub fn chords(&self, action: Action) -> &[Chord] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Which action owns this chord?
    pub fn owner(&self, chord: &Chord) -> Option<Action> {
        self.bindings.iter().find(|(_, cs)| cs.contains(chord)).map(|(a, _)| *a)
    }

    pub fn lookup(&self, chord: &Chord) -> Option<Action> {
        self.owner(chord)
    }

    /// Replace an action's chords. Refused if a chord is invalid or owned by another action.
    pub fn set(&mut self, action: Action, chords: Vec<Chord>) -> Result<(), BindError> {
        for c in &chords {
            c.validate_binding().map_err(BindError::Invalid)?;
            if let Some(by) = self.owner(c).filter(|&o| o != action) {
                return Err(BindError::Taken { chord: c.clone(), by });
            }
        }
        let mut seen = Vec::new();
        for c in chords {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        self.bindings.insert(action, seen);
        Ok(())
    }

    /// Add one chord to an action (keeps the existing ones).
    pub fn add(&mut self, action: Action, chord: Chord) -> Result<(), BindError> {
        let mut all = self.chords(action).to_vec();
        all.push(chord);
        self.set(action, all)
    }

    pub fn reset(&mut self, action: Action) {
        let d = Keymap::default();
        // Resetting must not silently steal a chord from another action: unbind the other one.
        for c in d.chords(action) {
            if let Some(other) = self.owner(c).filter(|&o| o != action) {
                if let Some(v) = self.bindings.get_mut(&other) {
                    v.retain(|x| x != c);
                }
            }
        }
        self.bindings.insert(action, d.chords(action).to_vec());
    }

    pub fn is_default(&self, action: Action) -> bool {
        self.chords(action) == Keymap::default().chords(action)
    }

    /// Only what differs from the defaults, in settings-file form.
    pub fn overrides(&self) -> BTreeMap<String, String> {
        Action::ALL
            .into_iter()
            .filter(|a| !self.is_default(*a))
            .map(|a| (a.id().to_string(), format_chord_list(self.chords(a))))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap()
    }

    #[test]
    fn parses_many_spellings() {
        assert_eq!(chord("ctrl+n"), Chord { ctrl: true, key: "n".into(), ..Default::default() });
        assert_eq!(chord("Ctrl+Shift+F"), chord("shift+control+f"), "order and case don't matter");
        assert_eq!(chord("ctrl+,").key, ",");
        assert_eq!(chord("ctrl++").key, "+");
        assert_eq!(chord("ctrl+plus").key, "+");
        assert_eq!(chord("F1").key, "f1");
        assert_eq!(chord("alt+Return").key, "enter");
        assert_eq!(chord("ctrl+PgUp").key, "pageup");
        assert!(chord("super+space").meta);
        assert!(Chord::parse("").is_err());
        assert!(Chord::parse("ctrl+").is_err());
        assert!(Chord::parse("ctrl+banana").is_err());
        assert!(Chord::parse("ctrl+f99").is_err());
    }

    #[test]
    fn parse_survives_hostile_unicode() {
        // multi-byte characters right where a prefix would end, and lowercase expansions
        for s in [
            "ctrl+é",
            "ctrl+İ",
            "İ+x",
            "ctr\u{212a}+x",
            "ＣＴＲＬ+x",
            "ctrl+\u{202e}n",
            "😀+😀",
            "ct\u{0301}rl+x",
            "+",
            "++",
            "+++",
            "ctrl+ctrl+ctrl+",
            "alt+alt+x",
            "Σ+σ",
        ] {
            let _ = Chord::parse(s); // must not panic
            let _ = parse_chord_list(s);
        }
        assert_eq!(Chord::parse("ctrl+ctrl+x").unwrap().key, "x");
        assert!(Chord::parse("ctrl+é").is_ok(), "a single non-ASCII character is a valid key");
    }

    #[test]
    fn formats_round_trip() {
        for s in
            ["ctrl+n", "ctrl+shift+f", "alt+enter", "f1", "ctrl+,", "ctrl++", "ctrl+alt+shift+meta+x", "ctrl+pageup"]
        {
            assert_eq!(chord(s).to_string(), s);
        }
        assert_eq!(chord("ctrl+shift+f").pretty(), "Ctrl+Shift+F");
        assert_eq!(chord("f1").pretty(), "F1");
        assert_eq!(chord("ctrl+,").pretty(), "Ctrl+,");
        assert_eq!(chord("alt+up").pretty(), "Alt+↑");
    }

    #[test]
    fn chord_lists_handle_commas() {
        assert_eq!(parse_chord_list("ctrl+=, ctrl++").unwrap(), vec![chord("ctrl+="), chord("ctrl++")]);
        assert_eq!(parse_chord_list("ctrl+,").unwrap(), vec![chord("ctrl+,")]);
        assert_eq!(parse_chord_list("ctrl+,, f2").unwrap(), vec![chord("ctrl+,"), chord("f2")]);
        assert_eq!(parse_chord_list("").unwrap(), vec![]);
        assert!(parse_chord_list("ctrl+n, nope+").is_err());
        assert_eq!(format_chord_list(&[chord("ctrl+="), chord("ctrl++")]), "ctrl+=, ctrl++");
    }

    #[test]
    fn plain_typing_keys_cannot_be_bound() {
        assert!(chord("n").validate_binding().is_err());
        assert!(chord("shift+n").validate_binding().is_err());
        assert!(chord("esc").validate_binding().is_err());
        assert!(chord("f5").validate_binding().is_ok());
        assert!(chord("ctrl+n").validate_binding().is_ok());
        assert!(chord("alt+x").validate_binding().is_ok());
    }

    #[test]
    fn defaults_are_valid_and_unique() {
        let map = Keymap::default();
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(!map.chords(a).is_empty(), "{a:?} has a default");
            for c in map.chords(a) {
                assert!(c.validate_binding().is_ok(), "{c}");
                assert!(seen.insert(c.clone()), "{c} bound twice by default");
                assert_eq!(map.lookup(c), Some(a));
            }
            assert_eq!(Action::from_id(a.id()), Some(a));
        }
        assert!(map.overrides().is_empty(), "defaults write nothing to the file");
    }

    #[test]
    fn overrides_apply_and_round_trip() {
        let mut o = BTreeMap::new();
        o.insert("save".to_string(), "ctrl+shift+s, f2".to_string());
        o.insert("zoom_in".to_string(), "".to_string());
        let (map, warnings) = Keymap::from_overrides(&o);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(map.lookup(&chord("f2")), Some(Action::Save));
        assert_eq!(map.lookup(&chord("ctrl+s")), None, "old default is gone");
        assert!(map.chords(Action::ZoomIn).is_empty(), "empty value unbinds");
        assert_eq!(map.overrides(), o);
        let (again, _) = Keymap::from_overrides(&map.overrides());
        assert_eq!(again, map);
    }

    #[test]
    fn bad_and_conflicting_lines_are_skipped_with_warnings() {
        let mut o = BTreeMap::new();
        o.insert("nonsense".to_string(), "ctrl+x".to_string());
        o.insert("save".to_string(), "banana".to_string());
        o.insert("find".to_string(), "n".to_string());
        o.insert("theme".to_string(), "ctrl+n".to_string()); // clashes with new_entry's default
        let (map, warnings) = Keymap::from_overrides(&o);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert_eq!(map.chords(Action::Save), Keymap::default().chords(Action::Save));
        assert_eq!(map.lookup(&chord("ctrl+n")), Some(Action::NewEntry));
        assert!(map.chords(Action::Theme).is_empty(), "the clashing override left theme unbound");
    }

    #[test]
    fn swapping_two_actions_keys_works() {
        let mut o = BTreeMap::new();
        o.insert("save".to_string(), "ctrl+n".to_string());
        o.insert("new_entry".to_string(), "ctrl+s".to_string());
        let (map, warnings) = Keymap::from_overrides(&o);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(map.lookup(&chord("ctrl+n")), Some(Action::Save));
        assert_eq!(map.lookup(&chord("ctrl+s")), Some(Action::NewEntry));
    }

    #[test]
    fn set_add_reset_and_conflicts() {
        let mut map = Keymap::default();
        // taking another action's chord is refused
        let err = map.set(Action::Theme, vec![chord("ctrl+s")]).unwrap_err();
        assert_eq!(err, BindError::Taken { chord: chord("ctrl+s"), by: Action::Save });
        assert!(err.to_string().contains("save vault"));
        assert!(matches!(map.set(Action::Theme, vec![chord("q")]), Err(BindError::Invalid(_))));
        assert_eq!(map.chords(Action::Theme), Keymap::default().chords(Action::Theme), "failed set changes nothing");
        // rebinding to your own chord is fine, duplicates collapse
        map.set(Action::Theme, vec![chord("ctrl+l"), chord("ctrl+l"), chord("f9")]).unwrap();
        assert_eq!(map.chords(Action::Theme).len(), 2);
        map.add(Action::Theme, chord("f10")).unwrap();
        assert_eq!(map.chords(Action::Theme).len(), 3);
        assert!(!map.is_default(Action::Theme));
        map.reset(Action::Theme);
        assert!(map.is_default(Action::Theme));
        // reset takes the default back from whoever is using it
        map.set(Action::Save, vec![chord("ctrl+l")]).unwrap_err(); // still owned by theme
        map.set(Action::Theme, vec![chord("f9")]).unwrap();
        map.set(Action::Save, vec![chord("ctrl+l")]).unwrap();
        map.reset(Action::Theme);
        assert_eq!(map.lookup(&chord("ctrl+l")), Some(Action::Theme));
        assert!(map.chords(Action::Save).is_empty(), "save lost the key theme took back");
    }
}
