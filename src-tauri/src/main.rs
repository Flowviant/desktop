// A release build is a GUI app: without this Windows opens a console window
// beside it, the "terminal" people saw on every launch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args_os().any(|arg| arg == std::ffi::OsStr::new("--uninstall-cleanup")) {
        flowviant_desktop_lib::uninstall_cleanup();
        return;
    }
    flowviant_desktop_lib::run();
}
