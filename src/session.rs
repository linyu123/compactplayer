use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    #[default]
    Sequential,
    RepeatOne,
    RepeatAll,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sequential => "顺序播放",
            Self::RepeatOne => "单曲循环",
            Self::RepeatAll => "列表循环",
        }
    }
    pub fn cycle(&mut self) {
        *self = match self {
            Self::Sequential => Self::RepeatOne,
            Self::RepeatOne => Self::RepeatAll,
            Self::RepeatAll => Self::Sequential,
        };
    }
    pub fn next(self, current: usize, count: usize) -> Option<usize> {
        if current >= count {
            return None;
        }
        match self {
            Self::RepeatOne => Some(current),
            Self::RepeatAll => Some((current + 1) % count),
            Self::Sequential => (current + 1 < count).then_some(current + 1),
        }
    }
}

pub struct Session {
    pub mode: Mode,
    pub positions: BTreeMap<String, f64>,
    pub window: [f32; 2],
    writer: Option<(mpsc::Sender<Value>, thread::JoinHandle<()>)>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            positions: BTreeMap::new(),
            window: [1120.0, 700.0],
            writer: None,
        }
    }
}

impl Session {
    pub fn position(&self, path: &Path) -> f64 {
        self.positions
            .get(path.to_string_lossy().as_ref())
            .copied()
            .filter(|p| p.is_finite() && *p >= 0.0)
            .unwrap_or(0.0)
    }
    pub fn remember(&mut self, path: &Path, position: f64, duration: f64) {
        if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
            return;
        }
        let key = path.to_string_lossy().into_owned();
        if position < 3.0 || duration - position <= 2.0 {
            self.positions.remove(&key);
        } else {
            self.positions.insert(key, position);
        }
    }
    pub fn load() -> (Self, Vec<PathBuf>, Option<usize>, f32, bool) {
        let session = Self::default();
        let Some(home) = std::env::var_os("HOME") else {
            return (session, vec![], None, 75.0, true);
        };
        let path =
            PathBuf::from(home).join("Library/Application Support/CompactPlayer/session.json");
        Self::load_path(path)
    }

    fn load_path(path: PathBuf) -> (Self, Vec<PathBuf>, Option<usize>, f32, bool) {
        let mut session = Self::default();
        let data: Value = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        session.mode = match data["mode"].as_str() {
            Some("单曲循环") => Mode::RepeatOne,
            Some("列表循环") => Mode::RepeatAll,
            _ => Mode::Sequential,
        };
        session.positions = serde_json::from_value(data["positions"].clone()).unwrap_or_default();
        if let Some(window) = data["window"].as_array() {
            if window.len() == 2 {
                session.window = [
                    window[0].as_f64().unwrap_or(1120.0).clamp(960.0, 3840.0) as f32,
                    window[1].as_f64().unwrap_or(700.0).clamp(540.0, 2160.0) as f32,
                ];
            }
        }
        let playlist = data["playlist"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected = data["selected"]
            .as_str()
            .and_then(|p| playlist.iter().position(|item| item == Path::new(p)));
        let volume = data["volume"].as_f64().unwrap_or(75.0).clamp(0.0, 100.0) as f32;
        let show = data["show_playlist"].as_bool().unwrap_or(true);
        let (sender, receiver) = mpsc::channel::<Value>();
        let worker = thread::spawn(move || {
            for snapshot in receiver {
                let result = (|| -> std::io::Result<()> {
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
                    std::fs::write(&temp, serde_json::to_vec_pretty(&snapshot)?)?;
                    std::fs::rename(temp, &path)
                })();
                if let Err(error) = result {
                    eprintln!("保存播放会话失败：{error}");
                }
            }
        });
        session.writer = Some((sender, worker));
        (session, playlist, selected, volume, show)
    }
    pub fn save(&self, playlist: &[PathBuf], selected: Option<usize>, volume: f32, show: bool) {
        if let Some((sender, _)) = &self.writer {
            let _ = sender.send(json!({ "version": 1, "mode": self.mode.label(), "positions": self.positions, "window": self.window, "playlist": playlist, "selected": selected.and_then(|i| playlist.get(i)), "volume": volume, "show_playlist": show }));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some((sender, worker)) = self.writer.take() {
            drop(sender);
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_roundtrip_and_invalid_json() {
        let folder =
            std::env::temp_dir().join(format!("compact-session-test-{}", std::process::id()));
        let path = folder.join("session.json");
        let (mut session, _, _, _, _) = Session::load_path(path.clone());
        session.mode = Mode::RepeatAll;
        session.window = [1200.0, 800.0];
        session.remember(Path::new("/example.mp4"), 25.0, 90.0);
        session.save(&[PathBuf::from("/example.mp4")], Some(0), 42.0, false);
        drop(session); // 退出时等待后台写入完成。
        let (session, playlist, selected, volume, show) = Session::load_path(path.clone());
        assert_eq!(session.mode, Mode::RepeatAll);
        assert_eq!(session.window, [1200.0, 800.0]);
        assert_eq!(session.position(Path::new("/example.mp4")), 25.0);
        assert_eq!(playlist, [PathBuf::from("/example.mp4")]);
        assert_eq!(selected, Some(0));
        assert_eq!(volume, 42.0);
        assert!(!show);
        drop(session);
        std::fs::write(&path, b"invalid json").unwrap();
        let (session, playlist, selected, volume, _) = Session::load_path(path);
        assert!(playlist.is_empty());
        assert_eq!(selected, None);
        assert_eq!(volume, 75.0);
        drop(session);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn playback_boundaries() {
        assert_eq!(Mode::Sequential.next(0, 2), Some(1));
        assert_eq!(Mode::Sequential.next(1, 2), None);
        assert_eq!(Mode::RepeatAll.next(1, 2), Some(0));
        assert_eq!(Mode::RepeatOne.next(1, 2), Some(1));
        assert_eq!(Mode::RepeatAll.next(0, 0), None);
    }
    #[test]
    fn completed_and_short_files_do_not_resume() {
        let mut session = Session::default();
        let path = Path::new("episode.mp4");
        session.remember(path, 30.0, 120.0);
        assert_eq!(session.position(path), 30.0);
        session.remember(path, 119.0, 120.0);
        assert_eq!(session.position(path), 0.0);
        session.remember(path, f64::NAN, 120.0);
        assert_eq!(session.position(path), 0.0);
    }
}
