//! UI controller: owns the `Vault`, mirrors it into Slint models, and turns
//! `AppState` callbacks into vault operations.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use paddy_core::{Action, Config, Entry, Error, Field, Keymap, Paths, Template, Theme, Vault};
use slint::{ComponentHandle, Model, SharedString, Timer, TimerMode, VecModel, Weak};

use crate::desktop::Desktop;
use crate::diag;
use crate::nav::{pick_model, NavState};
use crate::packs::{NetMsg, Pending};
use crate::{
    AppState, Binding, EntryRow, FieldRow, FontRow, HelpRow, Hint, KeyRow, MainWindow, MdBlock, MiniWindow, PackRow,
    PickRow, PopupState, QuickRow, QuickWindow, TemplateRow, VarRow,
};

pub struct App {
    pub(crate) ui: Weak<MainWindow>,
    pub(crate) vault: RefCell<Vault>,
    pub(crate) config: RefCell<Config>,
    pub(crate) paths: Paths,
    pub(crate) nav: RefCell<NavState>,
    pub(crate) diag: RefCell<diag::Tracker>,
    pub(crate) fonts: RefCell<Vec<String>>,
    pub(crate) fp_names: RefCell<Vec<String>>,
    pub(crate) fp_rows: Rc<VecModel<PickRow>>,
    pub(crate) pk_fonts: Rc<VecModel<FontRow>>,
    pub(crate) pk_packs: Rc<VecModel<PackRow>>,
    pub(crate) fonts_done: RefCell<std::collections::HashSet<&'static str>>,
    pub(crate) net_tx: std::sync::mpsc::Sender<NetMsg>,
    pub(crate) net_rx: RefCell<std::sync::mpsc::Receiver<NetMsg>>,
    pub(crate) net_pending: std::cell::Cell<u32>,
    pub(crate) net_timer: Timer,
    pub(crate) clip_timer: Timer,
    pub(crate) pending: RefCell<Option<Pending>>,
    pub(crate) themes: RefCell<Vec<Theme>>,
    pub(crate) tp_ids: RefCell<Vec<String>>,
    pub(crate) tp_rows: Rc<VecModel<PickRow>>,
    pub(crate) keymap: RefCell<Keymap>,
    pub(crate) recording: RefCell<Option<(Action, bool)>>,
    pub(crate) bindings: Rc<VecModel<Binding>>,
    pub(crate) hints: Rc<VecModel<Hint>>,
    pub(crate) help_left: Rc<VecModel<HelpRow>>,
    pub(crate) help_right: Rc<VecModel<HelpRow>>,
    pub(crate) key_rows: Rc<VecModel<KeyRow>>,
    pub(crate) md_blocks: Rc<VecModel<MdBlock>>,
    pub(crate) me: RefCell<std::rc::Weak<App>>,
    pub(crate) logo_frame: std::cell::Cell<usize>,
    pub(crate) logo_timer: Timer,
    pub(crate) diag_timer: Timer,
    /// Set after a save was refused because the file changed on disk; the next save overwrites.
    pub(crate) overwrite_armed: std::cell::Cell<bool>,
    pub(crate) vs_rows: Rc<VecModel<PickRow>>,
    pub(crate) gs_rows: Rc<VecModel<PickRow>>,
    pub(crate) quick: QuickWindow,
    pub(crate) mini: MiniWindow,
    pub(crate) quick_rows: Rc<VecModel<QuickRow>>,
    pub(crate) quick_fields: Rc<VecModel<FieldRow>>,
    pub(crate) desktop: RefCell<Option<Desktop>>,
    pub(crate) desktop_timer: Timer,
    pub(crate) entries: Rc<VecModel<EntryRow>>,
    fields: Rc<VecModel<FieldRow>>,
    templates: Rc<VecModel<TemplateRow>>,
    vars: Rc<VecModel<VarRow>>,
    // Kept alive for the whole session: on X11/Wayland the clipboard contents
    // vanish when the owning `Clipboard` is dropped.
    clipboard: RefCell<Option<arboard::Clipboard>>,
    status_timer: Timer,
    /// What the last copy request was for (field key or "notes") and whether it took
    /// the secret path (auto-clear). Never the value itself. For tests.
    last_copy: RefCell<Option<(String, bool)>>,
}

fn entry_row(e: &Entry) -> EntryRow {
    EntryRow {
        id: i32::try_from(e.id).unwrap_or(i32::MAX),
        label: e.label.as_str().into(),
        tags: e.tags.join(", ").into(),
    }
}

pub(crate) fn field_row(f: &Field) -> FieldRow {
    FieldRow { key: f.key.as_str().into(), value: f.value.as_str().into(), secret: f.is_secret, hidden: false }
}

/// Only wipe the clipboard if it still holds what we put there (never someone else's copy).
pub(crate) fn should_clear(current: Option<&str>, copied: &str) -> bool {
    current == Some(copied)
}

fn split_tags(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(String::from).collect()
}

impl App {
    pub fn new(ui: &MainWindow, vault: Vault, config: Config, paths: Paths) -> Rc<Self> {
        let (keymap, key_warnings) = Keymap::from_overrides(&config.keys);
        let (net_tx, net_rx) = std::sync::mpsc::channel();
        let app = Rc::new(Self {
            ui: ui.as_weak(),
            vault: RefCell::new(vault),
            config: RefCell::new(config),
            paths,
            nav: RefCell::new(NavState::default()),
            diag: RefCell::new(diag::Tracker::new()),
            fonts: RefCell::new(Vec::new()),
            fp_names: RefCell::new(Vec::new()),
            fp_rows: pick_model(),
            pk_fonts: Rc::new(VecModel::default()),
            pk_packs: Rc::new(VecModel::default()),
            fonts_done: RefCell::new(std::collections::HashSet::new()),
            net_tx,
            net_rx: RefCell::new(net_rx),
            net_pending: std::cell::Cell::new(0),
            net_timer: Timer::default(),
            clip_timer: Timer::default(),
            pending: RefCell::new(None),
            themes: RefCell::new(Vec::new()),
            tp_ids: RefCell::new(Vec::new()),
            tp_rows: pick_model(),
            keymap: RefCell::new(keymap),
            recording: RefCell::new(None),
            bindings: Rc::new(VecModel::default()),
            hints: Rc::new(VecModel::default()),
            help_left: Rc::new(VecModel::default()),
            help_right: Rc::new(VecModel::default()),
            key_rows: Rc::new(VecModel::default()),
            md_blocks: Rc::new(VecModel::default()),
            me: RefCell::new(std::rc::Weak::new()),
            logo_frame: std::cell::Cell::new(0),
            logo_timer: Timer::default(),
            diag_timer: Timer::default(),
            overwrite_armed: std::cell::Cell::new(false),
            vs_rows: pick_model(),
            gs_rows: pick_model(),
            quick: QuickWindow::new().expect("create popup window"),
            mini: MiniWindow::new().expect("create launcher window"),
            quick_rows: Rc::new(VecModel::default()),
            quick_fields: Rc::new(VecModel::default()),
            desktop: RefCell::new(None),
            desktop_timer: Timer::default(),
            entries: Rc::new(VecModel::default()),
            fields: Rc::new(VecModel::default()),
            templates: Rc::new(VecModel::default()),
            vars: Rc::new(VecModel::default()),
            clipboard: RefCell::new(None),
            status_timer: Timer::default(),
            last_copy: RefCell::new(None),
        });
        *app.me.borrow_mut() = Rc::downgrade(&app);
        let st = ui.global::<AppState>();
        st.set_entries(app.entries.clone().into());
        st.set_draft_fields(app.fields.clone().into());
        st.set_templates(app.templates.clone().into());
        st.set_tpl_vars(app.vars.clone().into());
        st.set_vault_name(app.vault.borrow().meta().name.as_str().into());
        app.wire(&st);
        app.wire_nav(&st);
        app.wire_settings(&st);
        app.wire_windows(ui);
        app.wire_look(&st);
        app.wire_themes(&st);
        app.wire_packs(&st);
        app.wire_keys(&st);
        app.wire_notes(&st);
        app.apply_keymap();
        if !key_warnings.is_empty() {
            st.set_keys_note(key_warnings.join("; ").into());
        }
        app.apply_window_bar();
        app.register_fonts();
        app.load_themes();
        app.apply_theme();
        app.apply_look();
        app.set_logo_frame(0);
        st.set_about_text(
            format!("v{}\nquick-access pad for IPs, creds and configs you reuse all day.", env!("CARGO_PKG_VERSION"))
                .into(),
        );
        app.play_logo();
        app.wire_popup();
        app.refresh(None);
        app.sync_dirty();
        app
    }

    fn wire(self: &Rc<Self>, st: &AppState<'_>) {
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_select, select, i);
        bind!(on_add_entry, add_entry);
        bind!(on_delete_entry, delete_entry);
        bind!(on_cancel_delete, cancel_delete);
        bind!(on_save, save);
        bind!(on_draft_edited, commit_draft);
        bind!(on_field_edited, field_edited, i, k, v);
        bind!(on_field_add, field_add);
        bind!(on_field_remove, field_remove, i);
        bind!(on_field_toggle_secret, field_toggle_secret, i);
        bind!(on_field_toggle_hidden, field_toggle_hidden, i);
        {
            let app = self.clone();
            st.on_field_copy(move |i| {
                app.field_copy(i);
            });
        }
        bind!(on_open_templates, open_templates);
        bind!(on_close_templates, close_templates);
        bind!(on_tpl_select, tpl_select, i);
        bind!(on_tpl_new, tpl_new);
        bind!(on_tpl_delete, tpl_delete);
        bind!(on_tpl_edited, tpl_edited);
        bind!(on_tpl_var_edited, tpl_var_edited, i, v);
        bind!(on_tpl_copy, tpl_copy);
    }

    // ---- plumbing ----

    pub(crate) fn with_state<R>(&self, f: impl FnOnce(&AppState<'_>) -> R) -> R {
        let ui = self.ui.upgrade().expect("window outlives its callbacks");
        let st = ui.global::<AppState>();
        f(&st)
    }

    /// Give keyboard focus back to whatever is on screen: the entry list, or the
    /// settings/keys page when one of those tabs is open.
    pub(crate) fn focus_list(&self) {
        if self.with_state(|s| s.get_tab()) != 0 {
            self.with_state(|s| s.set_focus_page(s.get_focus_page() + 1));
        } else if let Some(ui) = self.ui.upgrade() {
            ui.invoke_focus_list();
        }
    }

    pub(crate) fn set_status(&self, msg: &str, error: bool) {
        self.with_state(|s| {
            s.set_status(msg.into());
            s.set_status_error(error);
        });
        self.quick.global::<PopupState>().set_status(msg.into());
        let weak = self.ui.clone();
        let quick = self.quick.as_weak();
        self.status_timer.start(TimerMode::SingleShot, Duration::from_secs(3), move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<AppState>().set_status("".into());
            }
            if let Some(q) = quick.upgrade() {
                q.global::<PopupState>().set_status("".into());
            }
        });
    }

    /// Unwrap a core result, surfacing the error in the status line.
    pub(crate) fn report<T>(&self, r: paddy_core::Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.set_status(&e.to_string(), true);
                None
            }
        }
    }

    pub(crate) fn sync_dirty(&self) {
        let dirty = self.vault.borrow().is_dirty();
        self.with_state(|s| s.set_dirty(dirty));
    }

    pub fn save(&self) {
        let force = self.overwrite_armed.replace(false);
        let res = if force { self.vault.borrow_mut().save_overwrite() } else { self.vault.borrow_mut().save() };
        match res {
            Ok(()) => {
                self.set_status(if force { "overwritten" } else { "saved" }, false);
                self.play_logo();
            }
            Err(Error::ChangedOnDisk(_)) => {
                self.overwrite_armed.set(true);
                self.set_status("changed on disk by another paddy — press Ctrl+S again to overwrite", true);
            }
            Err(e) => self.set_status(&e.to_string(), true),
        }
        self.sync_dirty();
    }

    /// Called when the window closes so unsaved edits are not lost silently.
    /// If another instance saved the vault meanwhile, keep our state in a
    /// `.conflict-<time>.db` copy rather than overwrite theirs.
    pub fn save_on_exit(&self) {
        if !self.vault.borrow().is_dirty() {
            return;
        }
        let res = self.vault.borrow_mut().save();
        if let Err(Error::ChangedOnDisk(_)) = res {
            match self.vault.borrow().save_conflict_copy() {
                Ok(p) => eprintln!("paddy: vault changed on disk; your edits were kept in {}", p.display()),
                Err(e) => eprintln!("paddy: vault changed on disk and the conflict copy failed: {e}"),
            }
        }
    }

    // ---- entries ----

    /// Reload the list from the vault and select `select_id` (or the first entry).
    pub(crate) fn refresh(&self, select_id: Option<i64>) {
        let query = self.with_state(|s| s.get_search().to_string());
        let res = self.vault.borrow().search(&query);
        let Some(list) = self.report(res) else { return };
        let idx = select_id.and_then(|id| list.iter().position(|e| e.id == id)).or(if list.is_empty() {
            None
        } else {
            Some(0)
        });
        self.entries.set_vec(list.iter().map(entry_row).collect::<Vec<_>>());
        self.load_draft(idx.map_or(-1, |i| i as i32));
    }

    fn load_draft(&self, idx: i32) {
        let row = usize::try_from(idx).ok().and_then(|i| self.entries.row_data(i));
        let entry = row.and_then(|r| {
            let res = self.vault.borrow().get_entry(r.id as i64);
            self.report(res)
        });
        let idx = if entry.is_some() { idx } else { -1 };
        self.with_state(|s| {
            s.set_selected(idx);
            let (label, tags, notes) = match &entry {
                Some(e) => (e.label.clone(), e.tags.join(", "), e.notes.clone()),
                None => Default::default(),
            };
            s.set_draft_label(label.into());
            s.set_draft_tags(tags.into());
            s.set_draft_notes(notes.into());
        });
        let rows: Vec<FieldRow> = entry.map(|e| e.fields.iter().map(field_row).collect()).unwrap_or_default();
        self.fields.set_vec(rows);
        self.clamp_field_cursor();
        if self.with_state(|s| s.get_notes_preview()) {
            self.rebuild_md();
        }
    }

    pub(crate) fn selected_entry_id(&self) -> Option<i64> {
        let idx = self.with_state(|s| s.get_selected());
        usize::try_from(idx).ok().and_then(|i| self.entries.row_data(i)).map(|r| r.id as i64)
    }

    pub fn select(&self, i: i32) {
        if (i as usize) < self.entries.row_count() {
            self.load_draft(i);
        }
    }

    /// Write the on-screen draft (label/tags/notes/fields) into the vault.
    pub fn commit_draft(&self) {
        let (idx, label, tags, notes) = self.with_state(|s| {
            (
                s.get_selected(),
                s.get_draft_label().to_string(),
                s.get_draft_tags().to_string(),
                s.get_draft_notes().to_string(),
            )
        });
        let Some(row) = usize::try_from(idx).ok().and_then(|i| self.entries.row_data(i)) else {
            return;
        };
        let mut entry = Entry::new(label);
        entry.id = row.id as i64;
        entry.notes = notes;
        entry.tags = split_tags(&tags);
        entry.fields = self
            .fields
            .iter()
            .map(|f| Field { key: f.key.to_string(), value: f.value.to_string(), is_secret: f.secret })
            .collect();
        let res = self.vault.borrow_mut().update_entry(&entry);
        if self.report(res).is_none() {
            return;
        }
        self.entries.set_row_data(idx as usize, entry_row(&entry));
        self.sync_dirty();
    }

    pub fn add_entry(&self) {
        self.with_state(|s| {
            s.set_tab(0);
            s.set_notes_preview(false);
        });
        // A fresh, empty entry would be filtered out by an active search.
        self.with_state(|s| s.set_search("".into()));
        let res = self.vault.borrow_mut().add_entry(&Entry::new(""));
        if let Some(id) = self.report(res) {
            self.refresh(Some(id));
            self.sync_dirty();
        }
    }

    pub fn delete_entry(&self) {
        let idx = self.with_state(|s| s.get_selected());
        self.with_state(|s| s.set_confirm_delete(false));
        let Some(i) = usize::try_from(idx).ok() else { return };
        let Some(row) = self.entries.row_data(i) else { return };
        // Land on the neighbour that will remain after the delete.
        let next = self
            .entries
            .row_data(i + 1)
            .or_else(|| i.checked_sub(1).and_then(|p| self.entries.row_data(p)))
            .map(|r| r.id as i64);
        let res = self.vault.borrow_mut().delete_entry(row.id as i64);
        if self.report(res).is_some() {
            self.refresh(next);
            self.sync_dirty();
        }
        self.focus_list();
    }

    pub fn cancel_delete(&self) {
        self.with_state(|s| s.set_confirm_delete(false));
        self.focus_list();
    }

    // ---- fields ----

    pub fn field_edited(&self, i: i32, key: SharedString, value: SharedString) {
        if let Some(row) = self.fields.row_data(i as usize) {
            self.fields.set_row_data(i as usize, FieldRow { key, value, ..row });
            self.commit_draft();
        }
    }

    pub fn field_add(&self) {
        let n = self.fields.row_count();
        self.with_state(|s| s.set_focus_field(n as i32));
        self.fields.push(FieldRow { key: "".into(), value: "".into(), secret: false, hidden: false });
        self.commit_draft();
    }

    pub fn field_remove(&self, i: i32) {
        if (i as usize) < self.fields.row_count() {
            self.fields.remove(i as usize);
            self.commit_draft();
            self.clamp_field_cursor();
        }
    }

    pub(crate) fn field_count(&self) -> usize {
        self.fields.row_count()
    }

    pub fn field_toggle_secret(&self, i: i32) {
        if let Some(row) = self.fields.row_data(i as usize) {
            let secret = !row.secret;
            // Un-secreting also drops the mask, so nothing stays hidden by accident.
            let hidden = row.hidden && secret;
            self.fields.set_row_data(i as usize, FieldRow { secret, hidden, ..row });
            self.commit_draft();
        }
    }

    pub fn field_toggle_hidden(&self, i: i32) {
        if let Some(row) = self.fields.row_data(i as usize) {
            let hidden = !row.hidden;
            self.fields.set_row_data(i as usize, FieldRow { hidden, ..row });
        }
    }

    /// Copy one field (click, keyboard, main window or quick list): secret fields go
    /// through `copy_secret` so the clipboard auto-clear applies. Returns true if copied.
    pub fn field_copy(&self, i: i32) -> bool {
        match usize::try_from(i).ok().and_then(|i| self.fields.row_data(i)) {
            Some(row) => self.copy_field_row(&row),
            None => false,
        }
    }

    pub(crate) fn copy_field_row(&self, row: &FieldRow) -> bool {
        let what = if row.key.is_empty() { "value" } else { row.key.as_str() };
        *self.last_copy.borrow_mut() = Some((what.to_string(), row.secret));
        let copied = if row.secret { self.copy_secret(row.value.as_str()) } else { self.copy_text(row.value.as_str()) };
        if copied {
            self.set_status(&format!("copied {what}"), false);
        }
        copied
    }

    pub(crate) fn note_copy(&self, what: &str, secret: bool) {
        *self.last_copy.borrow_mut() = Some((what.to_string(), secret));
    }

    /// The last copy request: (field key or "notes", went through the secret path).
    /// The copied value itself is not kept. For tests and embedding apps.
    pub fn last_copy(&self) -> Option<(String, bool)> {
        self.last_copy.borrow().clone()
    }

    /// Copy something secret: like `copy_text`, then remove it from the clipboard after
    /// the configured delay, if the clipboard still holds exactly that text.
    pub(crate) fn copy_secret(&self, text: &str) -> bool {
        if !self.copy_text(text) {
            return false;
        }
        let secs = self.config.borrow().clip_clear_secs;
        if secs > 0 {
            let copied = text.to_string();
            let weak = self.me.borrow().clone();
            self.clip_timer.start(TimerMode::SingleShot, Duration::from_secs(secs as u64), move || {
                if let Some(app) = weak.upgrade() {
                    app.clear_clipboard_if(&copied);
                }
            });
        }
        true
    }

    fn clear_clipboard_if(&self, copied: &str) {
        let mut cb = self.clipboard.borrow_mut();
        let Some(c) = cb.as_mut() else { return };
        if should_clear(c.get_text().ok().as_deref(), copied) && c.clear().is_ok() {
            drop(cb);
            self.set_status("clipboard cleared", false);
        }
    }

    pub(crate) fn copy_text(&self, text: &str) -> bool {
        let mut cb = self.clipboard.borrow_mut();
        if cb.is_none() {
            *cb = arboard::Clipboard::new().ok();
        }
        let ok = cb.as_mut().is_some_and(|c| c.set_text(text.to_string()).is_ok());
        if !ok {
            // Drop a possibly dead connection so the next copy reconnects.
            *cb = None;
        }
        drop(cb);
        if !ok {
            self.set_status("clipboard unavailable", true);
        }
        ok
    }

    // ---- templates ----

    fn load_templates(&self) -> Vec<Template> {
        let res = self.vault.borrow().list_templates();
        self.report(res).unwrap_or_default()
    }

    fn reload_template_rows(&self) -> Vec<Template> {
        let list = self.load_templates();
        self.templates.set_vec(
            list.iter()
                .map(|t| TemplateRow { id: i32::try_from(t.id).unwrap_or(i32::MAX), name: t.name.as_str().into() })
                .collect::<Vec<_>>(),
        );
        list
    }

    pub fn open_templates(&self) {
        let list = self.reload_template_rows();
        self.with_state(|s| s.set_show_templates(true));
        self.tpl_select(if list.is_empty() { -1 } else { 0 });
    }

    pub fn close_templates(&self) {
        self.with_state(|s| s.set_show_templates(false));
        self.focus_list();
    }

    /// Value for a template variable taken from the entry on screen:
    /// first field whose key matches (case-insensitive) and is non-empty.
    fn entry_value_for(&self, var: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.key.as_str().eq_ignore_ascii_case(var) && !f.value.is_empty())
            .map(|f| f.value.to_string())
            .unwrap_or_default()
    }

    /// Rebuild the variable rows for `pattern`, keeping values already typed.
    fn rebuild_vars(&self, pattern: &str, keep: &HashMap<String, String>) {
        let rows: Vec<VarRow> = Template::new("", pattern)
            .variables()
            .into_iter()
            .map(|name| {
                let value = keep.get(&name).cloned().unwrap_or_else(|| self.entry_value_for(&name));
                VarRow { name: name.into(), value: value.into() }
            })
            .collect();
        self.vars.set_vec(rows);
        self.update_preview();
    }

    fn var_values(&self) -> HashMap<String, String> {
        self.vars.iter().filter(|v| !v.value.is_empty()).map(|v| (v.name.to_string(), v.value.to_string())).collect()
    }

    fn update_preview(&self) {
        let pattern = self.with_state(|s| s.get_tpl_pattern().to_string());
        let preview = Template::new("", pattern).render_partial(&self.var_values());
        self.with_state(|s| s.set_tpl_preview(preview.into()));
    }

    pub fn tpl_select(&self, i: i32) {
        let tpl = usize::try_from(i)
            .ok()
            .and_then(|idx| self.templates.row_data(idx))
            .and_then(|row| self.load_templates().into_iter().find(|t| t.id == row.id as i64));
        let (idx, name, pattern) = match &tpl {
            Some(t) => (i, t.name.clone(), t.pattern.clone()),
            None => (-1, String::new(), String::new()),
        };
        self.with_state(|s| {
            s.set_tpl_selected(idx);
            s.set_tpl_name(name.into());
            s.set_tpl_pattern(pattern.clone().into());
        });
        self.rebuild_vars(&pattern, &HashMap::new());
    }

    pub fn tpl_edited(&self) {
        let (idx, name, pattern) =
            self.with_state(|s| (s.get_tpl_selected(), s.get_tpl_name().to_string(), s.get_tpl_pattern().to_string()));
        let Some(row) = usize::try_from(idx).ok().and_then(|i| self.templates.row_data(i)) else {
            return;
        };
        let tpl = Template { id: row.id as i64, name, pattern };
        let res = self.vault.borrow_mut().update_template(&tpl);
        if self.report(res).is_none() {
            return;
        }
        self.templates.set_row_data(idx as usize, TemplateRow { id: row.id, name: tpl.name.as_str().into() });
        let keep = self.var_values();
        self.rebuild_vars(&tpl.pattern, &keep);
        self.sync_dirty();
    }

    pub fn tpl_var_edited(&self, i: i32, value: SharedString) {
        if let Some(row) = self.vars.row_data(i as usize) {
            self.vars.set_row_data(i as usize, VarRow { value, ..row });
            self.update_preview();
        }
    }

    pub fn tpl_new(&self) {
        let res = self.vault.borrow_mut().add_template(&Template::new("new template", ""));
        if self.report(res).is_some() {
            let list = self.reload_template_rows();
            self.tpl_select(list.len() as i32 - 1);
            self.sync_dirty();
        }
    }

    pub fn tpl_delete(&self) {
        let idx = self.with_state(|s| s.get_tpl_selected());
        let Some(row) = usize::try_from(idx).ok().and_then(|i| self.templates.row_data(i)) else {
            return;
        };
        let res = self.vault.borrow_mut().delete_template(row.id as i64);
        if self.report(res).is_some() {
            let list = self.reload_template_rows();
            let next = idx.min(list.len() as i32 - 1);
            self.tpl_select(next);
            self.sync_dirty();
        }
    }

    pub fn tpl_copy(&self) {
        let pattern = self.with_state(|s| s.get_tpl_pattern().to_string());
        match Template::new("", pattern).render(&self.var_values()) {
            Ok(text) => {
                let has_secret =
                    self.fields.iter().any(|f| f.secret && !f.value.is_empty() && text.contains(f.value.as_str()));
                let copied = if has_secret { self.copy_secret(&text) } else { self.copy_text(&text) };
                if copied {
                    self.close_templates();
                    self.set_status("copied command", false);
                }
            }
            Err(e) => self.set_status(&e.to_string(), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::should_clear;

    #[test]
    fn clipboard_is_only_cleared_when_it_still_holds_our_secret() {
        assert!(should_clear(Some("hunter2"), "hunter2"));
        assert!(!should_clear(Some("something else"), "hunter2"), "user copied something else meanwhile");
        assert!(!should_clear(None, "hunter2"), "empty or non-text clipboard is left alone");
        assert!(!should_clear(Some("hunter2 "), "hunter2"));
    }
}
