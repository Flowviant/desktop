fn main() {
    if std::env::args_os().any(|arg| arg == std::ffi::OsStr::new("--uninstall-cleanup")) {
        flowviant_desktop_lib::uninstall_cleanup();
        return;
    }
    flowviant_desktop_lib::run();
}
