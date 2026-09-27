use std::path::PathBuf;

use paddy_core::{Config, Paths, Vault, AUTOSTART_FLAG};
use paddy_ui::{app, MainWindow};
use slint::ComponentHandle;

/// Pick the vault to start with. `$PADDY_VAULT` is an explicit request: if it
/// fails to open that is an error. The last-used and default vaults are
/// fallbacks: a broken file is reported on stderr and skipped, never overwritten.
fn open_startup_vault(config: &mut Config, paths: &Paths) -> Result<Vault, Box<dyn std::error::Error>> {
    if let Some(p) = std::env::var_os("PADDY_VAULT").map(PathBuf::from) {
        return Ok(if p.exists() { Vault::open(&p)? } else { Vault::create(&p, "default")? });
    }
    if let Some(p) = config.last_vault.clone().filter(|p| p.exists()) {
        match Vault::open(&p) {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!("paddy: can't open last vault {}: {e}; using the default vault", p.display());
                config.last_vault = None;
            }
        }
    }
    let p = paths.default_vault();
    let v = if p.exists() { Vault::open(&p)? } else { Vault::create(&p, "default")? };
    config.last_vault = Some(p);
    Ok(v)
}

/// Tell X11 window managers what the side windows are, so tiling ones (i3,
/// bspwm, ...) float them instead of tiling them, and the launcher button
/// doesn't take keyboard focus away from whatever you were doing.
fn select_backend() -> Result<(), slint::PlatformError> {
    #[cfg(target_os = "linux")]
    {
        use slint::winit_030::winit::platform::x11::{WindowAttributesExtX11, WindowType};
        slint::BackendSelector::new()
            .with_winit_window_attributes_hook(|a| match a.title.as_str() {
                "paddy quick list" => a.with_x11_window_type(vec![WindowType::Dialog]),
                "paddy launcher" => a.with_x11_window_type(vec![WindowType::Utility]).with_active(false),
                _ => a,
            })
            .select()?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths = Paths::discover();
    // One paddy per user: a second launch asks the running one to show its window.
    #[cfg(unix)]
    let mut primary = match paddy_ui::instance::claim(&paths.instance_socket()) {
        Ok(paddy_ui::instance::Claim::Forwarded) => return Ok(()),
        Ok(paddy_ui::instance::Claim::First(p)) => Some(p),
        Err(e) => {
            eprintln!("paddy: single-instance check failed ({e}); starting anyway");
            None
        }
    };
    let at_login = std::env::args().skip(1).any(|a| a == AUTOSTART_FLAG);

    select_backend()?;
    let mut config = Config::load(&paths.config_file);
    let vault = open_startup_vault(&mut config, &paths)?;
    let start_hidden = at_login && config.start_hidden;

    let ui = MainWindow::new()?;
    let app = app::App::new(&ui, vault, config, paths);
    ui.window().on_close_requested({
        let app = app.clone();
        move || app.close_requested()
    });
    app.start_desktop();
    #[cfg(unix)]
    if let Some(p) = primary.as_mut() {
        app.listen_for_launches(p);
    }
    ui.invoke_focus_list();
    if start_hidden {
        // Started at login: only the tray icon (or the floating launcher) and the hotkey.
        app.hide_main();
    } else {
        ui.show()?;
    }
    // Keeps running with no window on screen; only "Quit" (or close with close-to-tray off) ends it.
    slint::run_event_loop_until_quit()?;
    let _ = ui.hide();
    app.save_on_exit();
    Ok(())
}
