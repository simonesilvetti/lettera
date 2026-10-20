//! Files dropped on the window, inside Flatpak.
//!
//! The drop event of Tauri carries the `text/uri-list` that WebKit asks the
//! file manager for: paths of the host, which the sandbox cannot read. The
//! file transfer portal hands over the same files as `/run/user/…/doc/…`
//! paths the app can read, but WebKit only requests a fixed set of targets
//! (`DropTargetGtk3.cpp`), so lettera requests the portal's target itself when
//! a drag enters the window. GTK resolves it into readable paths in
//! `gtk_selection_data_get_uris`; `dropped_files` then swaps them in. Sources
//! that do not offer the portal (non-GTK apps) keep their plain paths.

use std::sync::Mutex;

use gtk::prelude::*;

const PORTAL_TARGET: &str = "application/vnd.portal.filetransfer";
/// Any `info` WebKit and wry do not use, so neither treats this data as theirs.
const PORTAL_INFO: u32 = 0x1e77;

/// The readable paths of the drag in progress, once the portal sent them.
static PORTAL_PATHS: Mutex<Option<Vec<String>>> = Mutex::new(None);

pub fn listen(webview: &webkit2gtk::WebView) {
    let atom = gtk::gdk::Atom::intern(PORTAL_TARGET);
    let Some(list) = webview.drag_dest_get_target_list() else {
        return;
    };
    // the received data is matched to its `info` through the target list;
    // GTK's own entry for the portal (from `gtk_target_list_add_uri_targets`)
    // has the URI list's, which would hand the reply to wry
    list.remove(&atom);
    list.add(&atom, 0, PORTAL_INFO);

    // one request per drag; on Wayland the same context comes back for the
    // next drag, so leaving the window (which precedes every drop) resets it
    let current = std::rc::Rc::new(std::cell::RefCell::new(None::<gtk::gdk::DragContext>));
    let reset = current.clone();
    webview.connect_drag_leave(move |_, _, _| *reset.borrow_mut() = None);
    webview.connect_drag_motion(move |w, ctx, _, _, time| {
        if current.borrow().as_ref() != Some(ctx) {
            *current.borrow_mut() = Some(ctx.clone());
            *PORTAL_PATHS.lock().unwrap() = None;
            if ctx.list_targets().contains(&atom) {
                w.drag_get_data(ctx, &atom, time);
            }
        }
        false
    });
    webview.connect_drag_data_received(|w, _, _, _, data, info, _| {
        if info != PORTAL_INFO {
            return;
        }
        let paths: Vec<String> = data
            .uris()
            .iter()
            .filter_map(|u| gtk::glib::filename_from_uri(u).ok())
            .map(|(p, _)| p.display().to_string())
            .collect();
        if !paths.is_empty() {
            *PORTAL_PATHS.lock().unwrap() = Some(paths);
        }
        // WebKit counts the data it asked for; this reply is not one of them
        w.stop_signal_emission_by_name("drag-data-received");
    });
}

/// The paths of a drop the sandbox can read: the portal's, when it sent them.
pub fn readable(paths: Vec<String>) -> Vec<String> {
    match PORTAL_PATHS.lock().unwrap().take() {
        Some(portal) if portal.len() == paths.len() => portal,
        _ => paths,
    }
}
