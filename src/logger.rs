use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();


/// Creates `~/.pointer/logs/pointer_YYYY-MM-DD.log` and writes a start banner.
/// Must be called once at startup before any logging macros are used.
pub fn init() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let log_dir = std::path::PathBuf::from(&home).join(".pointer").join("logs");
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("[pointer] Could not create log dir {}: {e}", log_dir.display());
        return;
    }
    let log_path = log_dir.join(format!("pointer_{}.log", current_date_str()));
    match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => {
            let _ = LOG_FILE.set(Mutex::new(file));
            write_raw(&format!(
                "\n══════════════════════════════════════════\n\
                 Pointer started  {}\n\
                 Log file: {}\n\
                 ══════════════════════════════════════════",
                timestamp_str(),
                log_path.display(),
            ));
        }
        Err(e) => eprintln!("[pointer] Could not open log file {}: {e}", log_path.display()),
    }
}


/// Write a timestamped line to stderr AND the log file.
pub fn write(level: &str, msg: &str) {
    let line = format!("[{}] [{level}] {msg}", timestamp_str());
    eprintln!("{line}");
    write_raw(&line);
}

fn write_raw(line: &str) {
    if let Some(m) = LOG_FILE.get() {
        if let Ok(mut f) = m.lock() {
            let _ = writeln!(f, "{line}");
        }
    }
}


fn timestamp_str() -> String {
    let (y, mo, d, h, min, s) = utc_parts();
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{min:02}:{s:02} UTC")
}

fn current_date_str() -> String {
    let (y, mo, d, _, _, _) = utc_parts();
    format!("{y:04}-{mo:02}-{d:02}")
}

fn utc_parts() -> (u64, u64, u64, u64, u64, u64) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s   = secs % 60;
    let min = (secs / 60) % 60;
    let h   = (secs / 3600) % 24;
    let (y, mo, d) = days_to_ymd(secs / 86400);
    (y, mo, d, h, min, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z  = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp  = (5 * doy + 2) / 153;
    let day   = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year  = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}


#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logger::write("INFO ", &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logger::write("ERROR", &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_ai {
    ($($arg:tt)*) => {
        $crate::logger::write("AI   ", &format!($($arg)*))
    };
}
