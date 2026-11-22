use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{mpsc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static SENDER: OnceLock<mpsc::SyncSender<String>> = OnceLock::new();

pub fn directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Library/Logs/CompactPlayer")
}

fn append(path: &std::path::Path, message: &str) -> std::io::Result<()> {
    if fs::metadata(path).is_ok_and(|m| m.len() > 5 * 1024 * 1024) {
        let _ = fs::remove_file(path.with_extension("previous.log"));
        fs::rename(path, path.with_extension("previous.log"))?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{message}")
}

pub fn record(message: impl AsRef<str>) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let line = format!("{timestamp} {}", message.as_ref());
    if let Some(sender) = SENDER.get() {
        let _ = sender.try_send(line);
    }
}

pub fn init() {
    if SENDER.get().is_some() {
        return;
    }
    let folder = directory();
    if let Err(error) = fs::create_dir_all(&folder) {
        eprintln!("日志目录创建失败：{error}");
        return;
    }
    let (sender, receiver) = mpsc::sync_channel::<String>(512);
    if SENDER.set(sender).is_err() {
        return;
    }
    let path = folder.join(format!("player-{}.log", std::process::id()));
    // 每进程独立日志，避免同时运行开发版和安装版时互相覆盖。仅保留最近五份历史日志。
    if let Ok(entries) = fs::read_dir(&folder) {
        let mut files = entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("player-"))
            .collect::<Vec<_>>();
        files.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
        let excess = files.len().saturating_sub(5);
        for entry in files.into_iter().take(excess) {
            let _ = fs::remove_file(entry.path());
        }
    }
    std::thread::spawn(move || {
        for line in receiver {
            if let Err(error) = append(&path, &line) {
                eprintln!("写入日志失败：{error}");
            }
        }
    });
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // panic 可能直接终止进程，单独同步写入，不依赖后台线程收尾。
        let crash = format!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        let _ = fs::write(
            directory().join(format!("panic-{}.log", std::process::id())),
            crash,
        );
        previous(info);
    }));
    record(format!(
        "启动 version={} arch={} os={} pid={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::process::id()
    ));
}
