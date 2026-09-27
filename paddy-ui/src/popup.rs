//! Quick-list popup: a small always-on-top window with search, the entry's
//! fields, click-to-copy and inline edit. Opened from the tray icon or the
//! global hotkey. Works on the same in-memory vault as the main window.

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Model, SharedString, TimerMode};

use crate::app::{field_row, App};
use crate::desktop::{Desktop, DesktopEvent};
use crate::{FieldRow, PopupState, QuickRow};

impl App {
    pub(crate) fn wire_popup(self: &Rc<Self>) {
        let st = self.quick.global::<PopupState>();
        st.set_rows(self.quick_rows.clone().into());
        st.set_fields(self.quick_fields.clone().into());
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_query_edited, quick_query_edited);
        bind!(on_select, quick_select, i);
        bind!(on_field_edited, quick_field_edited, i, v);
        bind!(on_field_copy, quick_field_copy, i);
        bind!(on_field_toggle_hidden, quick_toggle_hidden, i);
        bind!(on_hide, quick_hide);
        {
            let app = self.clone();
            st.on_open_main(move || {
                app.quick_hide();
                app.show_main();
            });
        }
    }

    /// Start the tray icon + global hotkey and route their events to the UI.
    pub fn start_desktop(self: &Rc<Self>) {
        let hotkey = self.config.borrow().hotkey.clone();
        let desktop = Desktop::start(&hotkey);
        if !desktop.problems.is_empty() {
            self.set_status(&desktop.problems.join("; "), true);
        }
        // No tray to click (WSL, minimal WMs): show the floating launcher instead.
        if !desktop.tray_ok || self.config.borrow().mini_button {
            self.set_mini_visible(true);
            // keep the keyboard on the main window, not the button
            if let Some(ui) = self.ui.upgrade() {
                crate::windowctl::focus(ui.window());
            }
        }
        *self.desktop.borrow_mut() = Some(desktop);

        let app = Rc::downgrade(self);
        self.desktop_timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            let Some(app) = app.upgrade() else { return };
            loop {
                let ev = app.desktop.borrow().as_ref().and_then(|d| d.try_recv());
                match ev {
                    Some(ev) => app.on_desktop_event(ev),
                    None => break,
                }
            }
        });
    }

    fn on_desktop_event(&self, ev: DesktopEvent) {
        match ev {
            DesktopEvent::TogglePopup => self.toggle_popup(),
            DesktopEvent::ShowMain => self.show_main(),
            DesktopEvent::Quit => self.quit(),
        }
    }

    /// The popup's state global (for embedding apps and tests).
    pub fn quick_state(&self) -> PopupState<'_> {
        self.quick.global::<PopupState>()
    }

    pub fn quick_visible(&self) -> bool {
        self.quick.window().is_visible()
    }

    pub fn toggle_popup(&self) {
        if self.quick.window().is_visible() {
            self.quick_hide();
        } else {
            self.show_popup();
        }
    }

    pub fn show_popup(&self) {
        let st = self.quick.global::<PopupState>();
        st.set_vault_name(self.vault.borrow().meta().name.as_str().into());
        st.set_query("".into());
        st.set_status("".into());
        self.refresh_quick(None);
        let _ = self.quick.show();
        crate::windowctl::keep_on_top(self.quick.window());
        crate::windowctl::focus(self.quick.window());
        self.quick.invoke_focus_query();
    }

    /// Drop popup state that belonged to the previous vault (stale ids would hit the wrong entries).
    pub(crate) fn reset_popup(&self) {
        let st = self.quick.global::<PopupState>();
        st.set_vault_name(self.vault.borrow().meta().name.as_str().into());
        st.set_query("".into());
        self.refresh_quick(None);
    }

    fn quick_hide(&self) {
        let _ = self.quick.hide();
    }

    /// Rebuild the popup list from the vault for the current query.
    fn refresh_quick(&self, keep_id: Option<i64>) {
        let query = self.quick.global::<PopupState>().get_query().to_string();
        let res = self.vault.borrow().search(&query);
        let Some(found) = self.report(res) else { return };
        let idx = keep_id.and_then(|id| found.iter().position(|e| e.id == id)).or(if found.is_empty() {
            None
        } else {
            Some(0)
        });
        self.quick_rows.set_vec(
            found
                .iter()
                .map(|e| QuickRow { id: e.id as i32, label: e.label.as_str().into(), tags: e.tags.join(", ").into() })
                .collect::<Vec<_>>(),
        );
        self.quick_load(idx.map_or(-1, |i| i as i32));
    }

    fn quick_load(&self, idx: i32) {
        let row = usize::try_from(idx).ok().and_then(|i| self.quick_rows.row_data(i));
        let entry = row.and_then(|r| {
            let res = self.vault.borrow().get_entry(r.id as i64);
            self.report(res)
        });
        self.quick.global::<PopupState>().set_selected(if entry.is_some() { idx } else { -1 });
        let rows: Vec<FieldRow> = entry.map(|e| e.fields.iter().map(field_row).collect()).unwrap_or_default();
        self.quick_fields.set_vec(rows);
    }

    fn quick_query_edited(&self) {
        self.refresh_quick(None);
    }

    fn quick_select(&self, i: i32) {
        if (i as usize) < self.quick_rows.row_count() {
            self.quick_load(i);
        }
    }

    fn quick_selected_id(&self) -> Option<i64> {
        let idx = self.quick.global::<PopupState>().get_selected();
        usize::try_from(idx).ok().and_then(|i| self.quick_rows.row_data(i)).map(|r| r.id as i64)
    }

    /// Inline edit of a field value: write it through to the vault and refresh the main window.
    fn quick_field_edited(&self, i: i32, value: SharedString) {
        let Some(id) = self.quick_selected_id() else { return };
        let res = self.vault.borrow().get_entry(id);
        let Some(mut entry) = self.report(res) else { return };
        let Some(field) = entry.fields.get_mut(i as usize) else { return };
        field.value = value.to_string();
        let res = self.vault.borrow_mut().update_entry(&entry);
        if self.report(res).is_none() {
            return;
        }
        if let Some(row) = self.quick_fields.row_data(i as usize) {
            self.quick_fields.set_row_data(i as usize, FieldRow { value, ..row });
        }
        self.refresh(Some(id));
        self.sync_dirty();
    }

    fn quick_toggle_hidden(&self, i: i32) {
        if let Some(row) = self.quick_fields.row_data(i as usize) {
            let hidden = !row.hidden;
            self.quick_fields.set_row_data(i as usize, FieldRow { hidden, ..row });
        }
    }

    fn quick_field_copy(&self, i: i32) {
        let Some(row) = usize::try_from(i).ok().and_then(|i| self.quick_fields.row_data(i)) else {
            return;
        };
        let copied = if row.secret { self.copy_secret(row.value.as_str()) } else { self.copy_text(row.value.as_str()) };
        if copied {
            let what = if row.key.is_empty() { "value" } else { row.key.as_str() };
            self.set_status(&format!("copied {what}"), false);
            self.quick_hide();
        }
    }
}
