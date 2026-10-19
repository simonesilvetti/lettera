//! The tray icon: left click shows the window, right click opens a menu
//! (open, compose, config, quit). While the icon is there, closing the window
//! only hides it, so sync and notifications keep running.
//!
//! On Linux the icon is a StatusNotifierItem spoken directly over D-Bus
//! (`ksni`): Tauri's tray goes through libappindicator, which never reports a
//! left click. Elsewhere it is Tauri's own tray.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

static READY: AtomicBool = AtomicBool::new(false);

/// Whether the icon is in the tray, i.e. whether a hidden window can come back.
pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

fn compose(app: &AppHandle) {
    crate::show_window(app);
    let _ = app.emit("tray:compose", ());
}

fn config(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::open_config_file(&app).await {
            tracing::warn!("opening config: {e}");
        }
    });
}

/// Puts the icon in the tray; `done` learns whether that worked.
pub fn start(app: &AppHandle, done: impl FnOnce(bool) + Send + 'static) {
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let ok = sni::spawn(app).await;
            READY.store(ok, Ordering::Relaxed);
            done(ok);
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let ok = match native::build(app) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("tray icon: {e}");
                false
            }
        };
        READY.store(ok, Ordering::Relaxed);
        done(ok);
    }
}

#[cfg(target_os = "linux")]
mod sni {
    use ksni::menu::{MenuItem, StandardItem};
    use ksni::{Icon, ToolTip, TrayMethods};
    use tauri::AppHandle;

    struct Tray {
        app: AppHandle,
        icons: Vec<Icon>,
    }

    /// The bundled PNGs as the ARGB32 pixmaps the protocol wants.
    fn icons() -> Vec<Icon> {
        [
            &include_bytes!("../icons/32x32.png")[..],
            &include_bytes!("../icons/64x64.png")[..],
        ]
        .into_iter()
        .filter_map(|png| tauri::image::Image::from_bytes(png).ok())
        .map(|img| Icon {
            width: img.width() as i32,
            height: img.height() as i32,
            data: img
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [a, r, g, b])
                .collect(),
        })
        .collect()
    }

    fn item(label: &str, run: fn(&AppHandle)) -> MenuItem<Tray> {
        StandardItem {
            label: label.into(),
            activate: Box::new(move |t: &mut Tray| run(&t.app)),
            ..Default::default()
        }
        .into()
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "lettera".into()
        }

        fn title(&self) -> String {
            "lettera".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                title: "lettera".into(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            crate::show_window(&self.app);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                item("Open lettera", crate::show_window),
                item("Compose email", super::compose),
                item("Open config file", super::config),
                MenuItem::Separator,
                item("Quit", |app| app.exit(0)),
            ]
        }
    }

    pub async fn spawn(app: AppHandle) -> bool {
        let tray = Tray {
            app: app.clone(),
            icons: icons(),
        };
        // no well-known bus name: inside Flatpak only the watcher can be talked to
        match tray.disable_dbus_name(true).spawn().await {
            Ok(handle) => {
                // the service lives as long as the handle; so does the app
                std::mem::forget(handle);
                true
            }
            Err(e) => {
                tracing::warn!("tray icon unavailable, closing the window will quit: {e}");
                false
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod native {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::AppHandle;

    pub fn build(app: &AppHandle) -> tauri::Result<()> {
        let menu = Menu::with_items(
            app,
            &[
                &MenuItem::with_id(app, "open", "Open lettera", true, None::<&str>)?,
                &MenuItem::with_id(app, "compose", "Compose email", true, None::<&str>)?,
                &MenuItem::with_id(app, "config", "Open config file", true, None::<&str>)?,
                &PredefinedMenuItem::separator(app)?,
                &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
            ],
        )?;
        let mut tray = TrayIconBuilder::with_id("lettera")
            .tooltip("lettera")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, e| match e.id().as_ref() {
                "open" => crate::show_window(app),
                "compose" => super::compose(app),
                "config" => super::config(app),
                "quit" => app.exit(0),
                _ => {}
            })
            .on_tray_icon_event(|tray, e| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    crate::show_window(tray.app_handle());
                }
            });
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        tray.build(app)?;
        Ok(())
    }
}
