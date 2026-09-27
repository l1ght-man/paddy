use std::path::PathBuf;

use paddy_core::{Config, Paths, Vault};
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
    select_backend()?;
    let paths = Paths::discover();
    let mut config = Config::load(&paths.config_file);
    let vault = open_startup_vault(&mut config, &paths)?;

    let ui = MainWindow::new()?;
    let app = app::App::new(&ui, vault, config, paths);
    ui.window().on_close_requested({
        let app = app.clone();
        move || {
            // Closing the main window ends the app (popup included) after saving.
            app.save_on_exit();
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
        }
    });
    app.start_desktop();
    ui.invoke_focus_list();
    ui.run()?;
    app.save_on_exit();
    Ok(())
}
