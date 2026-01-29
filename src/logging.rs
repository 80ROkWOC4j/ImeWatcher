use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use log::{LevelFilter, Log, Metadata, Record};
use windows::Win32::System::SystemInformation::GetLocalTime;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

struct FileLogger {
    file: Mutex<File>,
    level: LevelFilter,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let ts = local_timestamp();
        let _ = writeln!(
            file,
            "{} [{:>5}] {}: {}",
            ts,
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        let mut file = self
            .file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = file.flush();
    }
}

pub fn init() {
    // Best-effort: if logging can't be initialized, do nothing.
    let Ok(path) = log_path() else {
        return;
    };

    let Ok(file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    let logger = FileLogger {
        file: Mutex::new(file),
        level,
    };

    if LOGGER.set(logger).is_err() {
        return;
    }

    let logger_ref = LOGGER.get().expect("logger set");
    if log::set_logger(logger_ref).is_err() {
        return;
    }
    log::set_max_level(level);

    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));
}

fn log_path() -> Result<PathBuf, ()> {
    let exe = std::env::current_exe().map_err(|_| ())?;
    let dir = exe.parent().ok_or(())?;

    let (y, m, d) = local_date_ymd();
    let filename = format!("imewatcher-{:04}-{:02}-{:02}.log", y, m, d);
    Ok(dir.join(filename))
}

fn local_date_ymd() -> (u16, u16, u16) {
    let st = unsafe { GetLocalTime() };
    (st.wYear, st.wMonth, st.wDay)
}

fn local_timestamp() -> String {
    let st = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}
