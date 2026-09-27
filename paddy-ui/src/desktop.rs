//! Tray icon and global hotkey. Both run on their own threads and only send
//! `DesktopEvent`s over a channel; the UI thread drains it from a timer.
//! Linux: StatusNotifierItem tray (ksni, no GTK) + X11 hotkey. Elsewhere: no-ops for now.

use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEvent {
    TogglePopup,
    ShowMain,
    Quit,
}

pub struct Desktop {
    rx: Receiver<DesktopEvent>,
    /// Kept so other sources (a second paddy launch) can feed the same queue.
    tx: Sender<DesktopEvent>,
    /// What could not be set up (no tray host, hotkey taken, ...), for the status line.
    pub problems: Vec<String>,
    /// True when a tray icon was registered with a tray host.
    pub tray_ok: bool,
    #[cfg(target_os = "linux")]
    _keep: linux::Keep,
}

impl Desktop {
    pub fn start(hotkey: &str) -> Self {
        let (tx, rx) = channel();
        let mut problems = Vec::new();
        #[cfg(target_os = "linux")]
        let (keep, tray_ok) = linux::start(tx.clone(), hotkey, &mut problems);
        #[cfg(not(target_os = "linux"))]
        let tray_ok = false;
        #[cfg(not(target_os = "linux"))]
        {
            let _ = hotkey;
            problems.push("tray and hotkey are not implemented on this platform yet".into());
        }
        Self {
            rx,
            tx,
            problems,
            tray_ok,
            #[cfg(target_os = "linux")]
            _keep: keep,
        }
    }

    pub fn try_recv(&self) -> Option<DesktopEvent> {
        self.rx.try_recv().ok()
    }

    /// Another way in to the same event queue the tray and hotkey use.
    pub fn sender(&self) -> Sender<DesktopEvent> {
        self.tx.clone()
    }
}

/// Check a hotkey string like `ctrl+alt+p` without registering it.
pub fn validate_hotkey(spec: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        spec.parse::<global_hotkey::hotkey::HotKey>().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = spec;
        Ok(())
    }
}

/// Tray icon: the app icon as ARGB32 in network byte order (StatusNotifierItem format).
pub fn icon_argb(size: usize) -> Vec<u8> {
    crate::logo::icon_rgba(size).chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect()
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use global_hotkey::hotkey::HotKey;
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use ksni::blocking::TrayMethods;

    pub struct Keep {
        _tray: Option<ksni::blocking::Handle<Tray>>,
        _hotkeys: Option<GlobalHotKeyManager>,
    }

    struct Tray {
        tx: Sender<DesktopEvent>,
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "paddy".into()
        }
        fn title(&self) -> String {
            "paddy".into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            [22, 32, 48, 64]
                .iter()
                .map(|&n| ksni::Icon { width: n as i32, height: n as i32, data: icon_argb(n) })
                .collect()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "paddy".into(),
                description: "click for the quick list".into(),
                ..Default::default()
            }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(DesktopEvent::TogglePopup);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, ev: DesktopEvent| {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |t: &mut Tray| {
                        let _ = t.tx.send(ev);
                    }),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("Quick list", DesktopEvent::TogglePopup),
                item("Open paddy", DesktopEvent::ShowMain),
                ksni::MenuItem::Separator,
                item("Quit", DesktopEvent::Quit),
            ]
        }
    }

    pub fn start(tx: Sender<DesktopEvent>, hotkey: &str, problems: &mut Vec<String>) -> (Keep, bool) {
        let tray = match (Tray { tx: tx.clone() }).spawn() {
            Ok(h) => Some(h),
            Err(e) => {
                problems.push(format!("tray unavailable: {e}"));
                None
            }
        };

        let hotkeys = match register_hotkey(tx, hotkey) {
            Ok(m) => Some(m),
            Err(e) => {
                problems.push(format!("hotkey {hotkey} unavailable: {e}"));
                None
            }
        };
        let tray_ok = tray.is_some();
        (Keep { _tray: tray, _hotkeys: hotkeys }, tray_ok)
    }

    fn register_hotkey(tx: Sender<DesktopEvent>, spec: &str) -> Result<GlobalHotKeyManager, String> {
        let hk: HotKey = spec.parse().map_err(|e| format!("{e}"))?;
        let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        manager.register(hk).map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            while let Ok(ev) = GlobalHotKeyEvent::receiver().recv() {
                if ev.id == hk.id() && ev.state == HotKeyState::Pressed {
                    let _ = tx.send(DesktopEvent::TogglePopup);
                }
            }
        });
        Ok(manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn hotkey_validation() {
        assert!(validate_hotkey("ctrl+alt+p").is_ok());
        assert!(validate_hotkey("ctrl+alt+").is_err());
        assert!(validate_hotkey("banana").is_err());
    }

    #[test]
    fn tray_icon_is_argb_of_the_app_icon() {
        let d = icon_argb(32);
        assert_eq!(d.len(), 32 * 32 * 4);
        assert_eq!(d[0], 0x00, "rounded corner is transparent (alpha first)");
        let mid = (16 * 32 + 3) * 4;
        assert_eq!((d[mid], d[mid + 1], d[mid + 2], d[mid + 3]), (0xff, 0x0e, 0x10, 0x0f), "dark tile at the edge");
    }
}
