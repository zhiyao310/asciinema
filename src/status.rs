use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn disable() {
    ENABLED.store(false, SeqCst);
}

macro_rules! info {
    ($fmt:expr) => (crate::status::do_info(format!($fmt)));
    ($fmt:expr, $($arg:tt)*) => (crate::status::do_info(format!($fmt, $($arg)*)));
}

macro_rules! warning {
    ($fmt:expr) => (crate::status::do_warn(format!($fmt)));
    ($fmt:expr, $($arg:tt)*) => (crate::status::do_warn(format!($fmt, $($arg)*)));
}

macro_rules! highlight {
    ($fmt:expr) => (crate::status::do_highlight(format!($fmt)));
    ($fmt:expr, $($arg:tt)*) => (crate::status::do_highlight(format!($fmt, $($arg)*)));
}

pub fn do_info(message: String) {
    if ENABLED.load(SeqCst) {
        println!("::: {message}");
    }
}

pub fn do_warn(message: String) {
    if ENABLED.load(SeqCst) {
        eprintln!("!!! {message}");
    }
}

pub fn do_highlight(message: String) {
    if ENABLED.load(SeqCst) {
        if std::io::stderr().is_terminal() {
            eprintln!("\x1b[1;36m::: {message}\x1b[0m");
        } else {
            eprintln!("::: {message}");
        }
    }
}

pub(crate) use highlight;
pub(crate) use info;
pub(crate) use warning;
