//! Files dropped on the window, inside Flatpak.
//!
//! WebKit's drop target asks the file manager for `text/uri-list`, which holds
//! paths of the host that the sandbox cannot read. The file transfer portal
//! hands over the same files as `/run/user/…/doc/…` paths the app can read;
//! GTK turns that target into such paths in `gtk_selection_data_get_uris`, so
//! putting it first in the webview's target list is enough. Sources that do
//! not offer it (non-GTK apps) fall back to the plain URI list.

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

const PORTAL_TARGETS: [&str; 2] = [
    "application/vnd.portal.filetransfer",
    "application/vnd.portal.files",
];

pub fn prefer_portal(webview: &webkit2gtk::WebView) {
    let Some(old) = webview.drag_dest_get_target_list() else {
        return;
    };
    // the same `info` as the URI list, so wry treats the data as file paths
    let Some(info) = old.find(&gtk::gdk::Atom::intern("text/uri-list")) else {
        return;
    };
    let entries: Vec<gtk::TargetEntry> = PORTAL_TARGETS
        .iter()
        .map(|t| gtk::TargetEntry::new(t, gtk::TargetFlags::empty(), info))
        .collect();
    let list = gtk::TargetList::new(&entries);
    // keep every target WebKit had, after the portal's
    unsafe {
        let mut n = 0;
        let table = gtk::ffi::gtk_target_table_new_from_list(old.to_glib_none().0, &mut n);
        gtk::ffi::gtk_target_list_add_table(list.to_glib_none().0, table, n as u32);
        gtk::ffi::gtk_target_table_free(table, n);
    }
    webview.drag_dest_set_target_list(Some(&list));
}
