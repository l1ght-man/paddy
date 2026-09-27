//! Key bindings in the UI: turns the core `Keymap` into Slint `KeyBinding`
//! chords, runs actions, and records new shortcuts on the keys tab.

use std::rc::Rc;

use paddy_core::{Action, BindError, Chord, Keymap};
use slint::platform::Key;
use slint::{ComponentHandle, Keys, Model, SharedString};

use crate::app::App;
use crate::{AppState, Binding, HelpRow, Hint, KeyRow, PopupState};

/// What a key press means for the recorder.
#[derive(Debug, PartialEq, Eq)]
pub enum Pressed {
    /// A Ctrl/Shift/Alt/Meta key on its own: keep waiting for the real key.
    ModifierOnly,
    Chord(Chord),
}

fn special_name(c: char) -> Option<&'static str> {
    let table: [(Key, &str); 21] = [
        (Key::Backspace, "backspace"),
        (Key::Tab, "tab"),
        (Key::Return, "enter"),
        (Key::Escape, "esc"),
        (Key::Delete, "delete"),
        (Key::Insert, "insert"),
        (Key::Home, "home"),
        (Key::End, "end"),
        (Key::PageUp, "pageup"),
        (Key::PageDown, "pagedown"),
        (Key::UpArrow, "up"),
        (Key::DownArrow, "down"),
        (Key::LeftArrow, "left"),
        (Key::RightArrow, "right"),
        (Key::Space, "space"),
        (Key::F1, "f1"),
        (Key::F2, "f2"),
        (Key::F3, "f3"),
        (Key::F4, "f4"),
        (Key::F5, "f5"),
        (Key::F6, "f6"),
    ];
    if let Some((_, n)) = table.iter().find(|(k, _)| char::from(*k) == c) {
        return Some(n);
    }
    // F7..F12 (consecutive code points after F6)
    const NAMES: [&str; 6] = ["f7", "f8", "f9", "f10", "f11", "f12"];
    let f7 = char::from(Key::F7) as u32;
    let code = c as u32;
    if (f7..f7 + 6).contains(&code) {
        return Some(NAMES[(code - f7) as usize]);
    }
    None
}

fn is_modifier_key(c: char) -> bool {
    [Key::Shift, Key::ShiftR, Key::Control, Key::ControlR, Key::Alt, Key::AltGr, Key::Meta, Key::MetaR, Key::CapsLock]
        .into_iter()
        .any(|k| char::from(k) == c)
}

/// Interpret a Slint key event as a chord. `None` = not something we can bind (composed text).
pub fn chord_from_event(text: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<Pressed> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if is_modifier_key(c) {
        return Some(Pressed::ModifierOnly);
    }
    let (key, shift) = match special_name(c) {
        Some(name) => (name.to_string(), shift),
        None => {
            let lower = c.to_lowercase().next().unwrap_or(c);
            // Shift is already baked into symbols and digits ('+', '!'): only letters keep it.
            (lower.to_string(), shift && lower.is_alphabetic())
        }
    };
    Some(Pressed::Chord(Chord { ctrl, alt, shift, meta, key }))
}

/// Slint's name for a canonical chord key.
fn slint_key_name(key: &str) -> String {
    let named = match key {
        "enter" => "Return",
        "esc" => "Escape",
        "tab" => "Tab",
        "space" => "Space",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "up" => "UpArrow",
        "down" => "DownArrow",
        "left" => "LeftArrow",
        "right" => "RightArrow",
        "+" => "Plus",
        "=" => "Equals",
        "," => "Comma",
        "-" => "HyphenMinus",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "'" => "Quote",
        "[" => "OpenBracket",
        "]" => "CloseBracket",
        "\\" => "BackSlash",
        "`" => "BackQuote",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    let mut it = key.chars();
    match (it.next(), it.next()) {
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        (Some(c), None) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        (Some(_), None) => key.to_string(), // literal fallback for other characters
        _ => key.to_uppercase(),            // f1..f24
    }
}

/// The Slint `Keys` value for a chord.
pub fn to_slint_keys(c: &Chord) -> Option<Keys> {
    let mut parts: Vec<String> = Vec::new();
    for (on, name) in [(c.ctrl, "Control"), (c.alt, "Alt"), (c.shift, "Shift"), (c.meta, "Meta")] {
        if on {
            parts.push(name.into());
        }
    }
    parts.push(slint_key_name(&c.key));
    Keys::from_parts(parts.iter().map(String::as_str)).ok()
}

fn pretty_list(chords: &[Chord]) -> String {
    if chords.is_empty() {
        "unbound".into()
    } else {
        chords.iter().map(Chord::pretty).collect::<Vec<_>>().join("  /  ")
    }
}

/// The four "copy field N" chords, shown compactly: `Ctrl+1..4` when they follow the
/// pattern, else each one (`Alt+1 Ctrl+2 ...`). Empty when none is bound.
fn copy_range(km: &Keymap) -> String {
    let firsts: Vec<Option<String>> = [Action::CopyField1, Action::CopyField2, Action::CopyField3, Action::CopyField4]
        .into_iter()
        .map(|a| km.chords(a).first().map(Chord::pretty))
        .collect();
    if let Some(Some(first)) = firsts.first() {
        if let Some(prefix) = first.strip_suffix('1') {
            let pattern = firsts.iter().enumerate().all(|(i, c)| c.as_deref() == Some(&format!("{prefix}{}", i + 1)));
            if pattern {
                return format!("{prefix}1..4");
            }
        }
    }
    firsts.into_iter().flatten().collect::<Vec<_>>().join(" ")
}

/// Short "key label" hint list, skipping unbound actions.
fn hint_line(parts: &[(Option<String>, &str)]) -> String {
    parts
        .iter()
        .filter_map(|(k, label)| k.as_ref().filter(|k| !k.is_empty()).map(|k| format!("{k} {label}")))
        .collect::<Vec<_>>()
        .join("   ")
}

const FOOTER: [(Action, &str); 5] = [
    (Action::NewEntry, "new"),
    (Action::Save, "save"),
    (Action::Find, "find"),
    (Action::Vaults, "vaults"),
    (Action::Settings, "settings"),
];

impl App {
    pub(crate) fn wire_keys(self: &Rc<Self>, st: &AppState<'_>) {
        st.set_bindings(self.bindings.clone().into());
        st.set_hints(self.hints.clone().into());
        st.set_help_left(self.help_left.clone().into());
        st.set_help_right(self.help_right.clone().into());
        st.set_key_rows(self.key_rows.clone().into());
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_run_action, run_action, id);
        bind!(on_key_capture, key_capture, t, c, a, s, m);
        bind!(on_open_keys, open_keys);
        bind!(on_key_change, key_change, id);
        bind!(on_key_add, key_add, id);
        bind!(on_key_clear, key_clear, id);
        bind!(on_key_reset, key_reset, id);
        bind!(on_keys_reset_all, keys_reset_all);
        bind!(on_record_key, record_key, t, c, a, s, m);
        bind!(on_cancel_record, cancel_recording);
    }

    /// Rebuild everything derived from the keymap: bindings, footer, help, keys tab.
    pub(crate) fn apply_keymap(&self) {
        let km = self.keymap.borrow();

        let mut binds = Vec::new();
        for a in Action::ALL {
            for c in km.chords(a) {
                if let Some(keys) = to_slint_keys(c) {
                    binds.push(Binding { action: a.id().into(), keys });
                }
            }
        }
        self.bindings.set_vec(binds);

        let hints: Vec<Hint> = FOOTER
            .iter()
            .filter_map(|(a, label)| {
                km.chords(*a).first().map(|c| Hint { key: c.pretty().into(), label: (*label).into() })
            })
            .collect();
        self.hints.set_vec(hints);
        let help_key = km.chords(Action::Help).first().map(Chord::pretty).unwrap_or_default();

        let row = |a: Action| HelpRow { header: false, keys: pretty_list(km.chords(a)).into(), what: a.label().into() };
        let header = |t: &str| HelpRow { header: true, keys: "".into(), what: t.into() };
        let fixed = |k: &str, w: &str| HelpRow { header: false, keys: k.into(), what: w.into() };
        let group = |g: &'static str| Action::ALL.into_iter().filter(move |a| a.group() == g);

        let mut left = vec![header("entries")];
        left.extend(group("entries").map(row));
        left.extend([fixed("↑  ↓", "pick entry"), fixed("Enter", "edit entry"), fixed("Esc", "back to the list")]);
        left.push(header("move"));
        left.extend(group("move").map(row));
        left.extend([fixed("↑  ↓  Tab", "in fields: pick a field"), fixed("Enter  /  Ctrl+C", "in fields: copy it")]);
        let mut right = vec![header("copy")];
        right.extend(group("copy").map(row));
        right.push(header("find and switch"));
        right.extend(group("find and switch").map(row));
        right.push(header("app"));
        right.extend(group("app").map(row));
        let quick = Chord::parse(&self.config.borrow().hotkey)
            .map(|c| c.pretty())
            .unwrap_or_else(|_| self.config.borrow().hotkey.clone());
        right.push(fixed(&quick, "quick list (anywhere)"));
        self.help_left.set_vec(left);
        self.help_right.set_vec(right);

        let first = |a: Action| km.chords(a).first().map(Chord::pretty);
        let copies = Some(copy_range(&km));
        let fields_hint = hint_line(&[(first(Action::NextField), "pick"), (copies.clone(), "copy")]);
        let popup_hint = hint_line(&[
            (Some("↑↓".into()), "entry"),
            (Some("Tab".into()), "field"),
            (Some("Enter".into()), "copy"),
            (copies, "copy field"),
            (Some("Esc".into()), "close"),
        ]);
        let copy_keys: Vec<SharedString> =
            [Action::CopyField1, Action::CopyField2, Action::CopyField3, Action::CopyField4]
                .into_iter()
                .map(|a| first(a).unwrap_or_default().into())
                .collect();

        let mut rows = Vec::new();
        for g in Action::GROUPS {
            rows.push(KeyRow { header: true, id: "".into(), label: g.into(), chords: "".into(), is_default: true });
            for a in group(g) {
                rows.push(KeyRow {
                    header: false,
                    id: a.id().into(),
                    label: a.label().into(),
                    chords: pretty_list(km.chords(a)).into(),
                    is_default: km.is_default(a),
                });
            }
        }
        self.key_rows.set_vec(rows);
        drop(km);
        self.with_state(|s| {
            s.set_help_key(help_key.into());
            s.set_fields_hint(fields_hint.into());
        });
        let ps = self.quick.global::<PopupState>();
        ps.set_hint(format!("type to search   {popup_hint}").into());
        ps.set_copy_keys(std::rc::Rc::new(slint::VecModel::from(copy_keys)).into());
    }

    fn persist_keys(&self) {
        let overrides = self.keymap.borrow().overrides();
        self.config.borrow_mut().keys = overrides;
        self.save_config();
        self.apply_keymap();
    }

    fn keys_note(&self, msg: &str) {
        self.with_state(|s| s.set_keys_note(msg.into()));
    }

    // ---- running actions ----

    /// The chord's action in the current keymap, if any (for the capture handlers).
    pub(crate) fn action_for_event(
        &self,
        text: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> Option<Action> {
        match chord_from_event(text, ctrl, alt, shift, meta)? {
            Pressed::Chord(c) => self.keymap.borrow().lookup(&c),
            Pressed::ModifierOnly => None,
        }
    }

    /// Main window, capture phase: run a bound chord before the focused text field
    /// can swallow it. Unbound keys go on to the widget as usual.
    fn key_capture(&self, text: SharedString, ctrl: bool, alt: bool, shift: bool, meta: bool) -> bool {
        if self.recording.borrow().is_some() {
            return false;
        }
        let Some(action) = self.action_for_event(text.as_str(), ctrl, alt, shift, meta) else { return false };
        self.run_action(action.id().into());
        true
    }

    /// Select the entry `delta` rows away (clamped); from nothing, start at an end.
    pub(crate) fn step_entry(&self, delta: i32) {
        self.show_entries_tab();
        let n = self.entries.row_count() as i32;
        if n == 0 {
            return;
        }
        let cur = self.with_state(|s| s.get_selected());
        let next = if cur < 0 {
            if delta > 0 {
                0
            } else {
                n - 1
            }
        } else {
            (cur + delta).clamp(0, n - 1)
        };
        if next != cur {
            self.select(next);
        }
    }

    /// Back to the entries tab; the page that had focus is gone then, so the list takes it.
    fn show_entries_tab(&self) {
        if self.with_state(|s| s.get_tab()) != 0 {
            self.cancel_recording();
            self.with_state(|s| s.set_tab(0));
            self.focus_list();
        }
    }

    fn fields_focused(&self) -> bool {
        self.ui.upgrade().is_some_and(|ui| ui.invoke_fields_focused())
    }

    /// Next / previous field: the first press moves focus into the fields area
    /// (first or last row); stepping back past the first row returns to the list.
    pub(crate) fn step_field(&self, delta: i32) {
        self.show_entries_tab();
        let n = self.field_count() as i32;
        if self.with_state(|s| s.get_selected()) < 0 || n == 0 {
            self.set_status("no fields in this entry", false);
            return;
        }
        let next = if self.fields_focused() {
            let cur = self.with_state(|s| s.get_field_cursor());
            if cur + delta < 0 {
                self.focus_list();
                return;
            }
            (cur + delta).clamp(0, n - 1)
        } else if delta > 0 {
            0
        } else {
            n - 1
        };
        self.with_state(|s| s.set_field_cursor(next));
        if let Some(ui) = self.ui.upgrade() {
            ui.invoke_focus_fields();
        }
    }

    /// Keep the field cursor on a real row after the fields changed; with no
    /// fields left, the fields area hands focus back to the list.
    pub(crate) fn clamp_field_cursor(&self) {
        let n = self.field_count() as i32;
        let cur = self.with_state(|s| s.get_field_cursor());
        let next = if n == 0 { -1 } else { cur.clamp(0, n - 1) };
        if next != cur {
            self.with_state(|s| s.set_field_cursor(next));
        }
        if n == 0 && self.fields_focused() {
            self.focus_list();
        }
    }

    /// Copy field `i` of the selected entry (same path as its [c] button).
    fn copy_field_n(&self, i: usize) {
        if self.with_state(|s| s.get_selected()) < 0 {
            self.set_status("no entry selected", true);
        } else if i >= self.field_count() {
            self.set_status(&format!("this entry has no field {}", i + 1), true);
        } else {
            self.field_copy(i as i32);
        }
    }

    fn copy_notes_action(&self) {
        let empty = self.with_state(|s| s.get_selected() < 0 || s.get_draft_notes().trim().is_empty());
        if empty {
            self.set_status("no notes to copy", true);
        } else {
            self.copy_notes();
        }
    }

    pub fn run_action(&self, id: SharedString) {
        let Some(action) = Action::from_id(id.as_str()) else { return };
        // While a dialog is open only a few global actions apply.
        if self.with_state(|s| s.get_overlay_open()) && !action.works_in_overlay() {
            return;
        }
        match action {
            Action::NewEntry => {
                self.add_entry();
                if let Some(ui) = self.ui.upgrade() {
                    ui.invoke_focus_label();
                }
            }
            Action::Save => self.save(),
            Action::DeleteEntry => {
                if self.with_state(|s| s.get_selected()) >= 0 {
                    self.with_state(|s| {
                        s.set_tab(0);
                        s.set_confirm_delete(true);
                    });
                }
            }
            Action::TogglePreview => self.toggle_preview(),
            Action::Find => {
                self.with_state(|s| s.set_tab(0));
                if let Some(ui) = self.ui.upgrade() {
                    ui.invoke_focus_search();
                }
            }
            Action::FindAll => self.open_global(),
            Action::Vaults => self.open_vaults(),
            Action::Templates => self.open_templates(),
            Action::Theme => self.toggle_theme(),
            Action::Settings => {
                if self.with_state(|s| s.get_tab()) == 1 {
                    self.close_settings();
                } else {
                    self.open_settings();
                }
            }
            Action::Keys => {
                if self.with_state(|s| s.get_tab()) == 2 {
                    self.close_settings();
                } else {
                    self.open_keys();
                }
            }
            Action::Packs => {
                if self.with_state(|s| s.get_tab()) == 3 {
                    self.close_settings();
                } else {
                    self.open_packs();
                }
            }
            Action::Help => {
                if self.with_state(|s| s.get_show_help()) {
                    self.close_help();
                } else {
                    self.open_help();
                }
            }
            Action::ZoomIn => self.bump_font_size(1),
            Action::ZoomOut => self.bump_font_size(-1),
            Action::ZoomReset => self.reset_font_size(),
            Action::NextEntry => self.step_entry(1),
            Action::PrevEntry => self.step_entry(-1),
            Action::NextField => self.step_field(1),
            Action::PrevField => self.step_field(-1),
            Action::CopyField1 | Action::CopyField2 | Action::CopyField3 | Action::CopyField4 => {
                self.copy_field_n(action.copy_field_index().unwrap_or(0));
            }
            Action::CopyNotes => self.copy_notes_action(),
        }
    }

    // ---- the keys tab ----

    pub fn open_keys(&self) {
        self.diag_timer.stop();
        self.cancel_recording();
        self.keys_note("");
        self.with_state(|s| s.set_tab(2));
    }

    pub(crate) fn cancel_recording(&self) {
        *self.recording.borrow_mut() = None;
        self.with_state(|s| {
            s.set_recording("".into());
            s.set_recording_label("".into());
        });
    }

    fn start_recording(&self, id: SharedString, add: bool) {
        let Some(action) = Action::from_id(id.as_str()) else { return };
        *self.recording.borrow_mut() = Some((action, add));
        self.keys_note("");
        self.with_state(|s| {
            s.set_recording(action.id().into());
            s.set_recording_label(action.label().into());
        });
    }

    fn key_change(&self, id: SharedString) {
        self.start_recording(id, false);
    }

    fn key_add(&self, id: SharedString) {
        self.start_recording(id, true);
    }

    fn key_clear(&self, id: SharedString) {
        if let Some(a) = Action::from_id(id.as_str()) {
            let _ = self.keymap.borrow_mut().set(a, Vec::new());
            self.keys_note("");
            self.persist_keys();
        }
    }

    fn key_reset(&self, id: SharedString) {
        if let Some(a) = Action::from_id(id.as_str()) {
            self.keymap.borrow_mut().reset(a);
            self.keys_note("");
            self.persist_keys();
        }
    }

    fn keys_reset_all(&self) {
        *self.keymap.borrow_mut() = Keymap::default();
        self.cancel_recording();
        self.keys_note("");
        self.persist_keys();
    }

    /// The next key press while recording: cancel, keep waiting, refuse, or bind.
    fn record_key(&self, text: SharedString, ctrl: bool, alt: bool, shift: bool, meta: bool) {
        let Some((action, add)) = *self.recording.borrow() else { return };
        let chord = match chord_from_event(text.as_str(), ctrl, alt, shift, meta) {
            None | Some(Pressed::ModifierOnly) => return,
            Some(Pressed::Chord(c)) => c,
        };
        if chord.key == "esc" && !(chord.ctrl || chord.alt || chord.meta) {
            self.cancel_recording();
            self.keys_note("");
            return;
        }
        let result = {
            let mut km = self.keymap.borrow_mut();
            if add {
                km.add(action, chord.clone())
            } else {
                km.set(action, vec![chord.clone()])
            }
        };
        match result {
            Ok(()) => {
                self.cancel_recording();
                self.keys_note("");
                self.persist_keys();
                self.set_status(&format!("{} → {}", action.label(), chord.pretty()), false);
            }
            // stay in recording mode so the user can just try another chord
            Err(e @ (BindError::Invalid(_) | BindError::Taken { .. })) => self.keys_note(&format!("{e} — try another")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Chord {
        Chord::parse(s).unwrap()
    }

    #[test]
    fn events_become_chords() {
        assert_eq!(chord_from_event("n", true, false, false, false), Some(Pressed::Chord(ch("ctrl+n"))));
        assert_eq!(chord_from_event("N", true, false, true, false), Some(Pressed::Chord(ch("ctrl+shift+n"))));
        assert_eq!(
            chord_from_event("+", true, false, true, false),
            Some(Pressed::Chord(ch("ctrl++"))),
            "shift is baked into '+'"
        );
        assert_eq!(chord_from_event(",", true, false, false, false), Some(Pressed::Chord(ch("ctrl+,"))));
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::F1)), false, false, false, false),
            Some(Pressed::Chord(ch("f1")))
        );
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::Escape)), false, false, false, false),
            Some(Pressed::Chord(ch("esc")))
        );
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::UpArrow)), false, true, false, false),
            Some(Pressed::Chord(ch("alt+up")))
        );
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::F9)), false, false, false, false),
            Some(Pressed::Chord(ch("f9")))
        );
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::Control)), true, false, false, false),
            Some(Pressed::ModifierOnly)
        );
        assert_eq!(
            chord_from_event(&String::from(char::from(Key::Shift)), false, false, true, false),
            Some(Pressed::ModifierOnly)
        );
        assert_eq!(chord_from_event("", true, false, false, false), None);
        assert_eq!(chord_from_event("ab", true, false, false, false), None);
    }

    #[test]
    fn copy_keys_are_shown_compactly_when_they_follow_the_pattern() {
        let mut km = Keymap::default();
        assert_eq!(copy_range(&km), "Ctrl+1..4");
        for (i, a) in
            [Action::CopyField1, Action::CopyField2, Action::CopyField3, Action::CopyField4].into_iter().enumerate()
        {
            km.set(a, vec![ch(&format!("alt+{}", i + 1))]).unwrap();
        }
        assert_eq!(copy_range(&km), "Alt+1..4");
        km.set(Action::CopyField2, vec![ch("f7")]).unwrap();
        assert_eq!(copy_range(&km), "Alt+1 F7 Alt+3 Alt+4");
        km.set(Action::CopyField1, Vec::new()).unwrap();
        assert_eq!(copy_range(&km), "F7 Alt+3 Alt+4", "unbound ones are left out");
        assert_eq!(
            hint_line(&[(Some("Alt+→".into()), "pick"), (None, "gone"), (Some(String::new()), "empty")]),
            "Alt+→ pick"
        );
    }

    #[test]
    fn every_default_chord_converts_to_slint_keys() {
        let km = Keymap::default();
        for a in Action::ALL {
            for c in km.chords(a) {
                assert!(to_slint_keys(c).is_some(), "{a:?} {c}");
            }
        }
        for s in
            ["ctrl+shift+f", "alt+enter", "f12", "ctrl+pageup", "meta+space", "ctrl+/", "ctrl+[", "ctrl+7", "ctrl+é"]
        {
            assert!(to_slint_keys(&ch(s)).is_some(), "{s}");
        }
    }
}
