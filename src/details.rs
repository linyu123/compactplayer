use crate::{logging, mpv, Player};
use eframe::egui;
use serde_json::Value;
use std::time::{Duration, Instant};

pub struct Details {
    pub subtitle_open: bool,
    pub audio_open: bool,
    pub diagnostics_open: bool,
    pub hardware: bool,
    pub sub_delay: f64,
    pub tracks: Vec<Value>,
    stats: String,
    last_refresh: Instant,
}

impl Default for Details {
    fn default() -> Self {
        Self {
            subtitle_open: false,
            audio_open: false,
            diagnostics_open: false,
            hardware: true,
            sub_delay: 0.0,
            tracks: vec![],
            stats: String::new(),
            last_refresh: Instant::now() - Duration::from_secs(1),
        }
    }
}

pub fn track_label(track: &Value) -> String {
    let mut parts = vec![format!("#{}", track["id"].as_i64().unwrap_or(0))];
    for key in ["title", "lang", "codec"] {
        if let Some(text) = track[key].as_str().filter(|s| !s.is_empty()) {
            parts.push(text.to_owned());
        }
    }
    if let Some(channels) = track["demux-channel-count"].as_u64() {
        parts.push(format!("{channels} 声道"));
    }
    if track["external"] == true {
        parts.push("外挂".into());
    }
    parts.join(" · ")
}

impl Player {
    pub fn selected_track_label(&self, kind: &str) -> String {
        self.details
            .tracks
            .iter()
            .find(|track| track["type"] == kind && track["selected"] == true)
            .map(track_label)
            .unwrap_or_else(|| "未选择".into())
    }

    pub fn poll_backend(&mut self) {
        let events = self
            .backend
            .as_ref()
            .map(|backend| backend.notifications())
            .unwrap_or_default();
        for event in events {
            match event {
                mpv::Notification::Log(message) => logging::record(format!("mpv {message}")),
                mpv::Notification::Loaded => {
                    logging::record(format!(
                        "文件加载成功 codec={} hwdec={}",
                        self.property("video-codec").unwrap_or_default(),
                        self.property("hwdec-current").unwrap_or_default()
                    ));
                    self.details.last_refresh = Instant::now() - Duration::from_secs(1);
                }
                mpv::Notification::Failed(error) => {
                    logging::record(format!("文件加载失败：{error}"));
                    self.playing = false;
                    self.rewinding = false;
                    self.eof_armed = false;
                    self.texture = None;
                    self.details.tracks.clear();
                    self.status = format!("播放失败：{error}");
                    self.notice = Some(format!("无法播放当前文件：{error}\n请确认文件仍然存在、可读取且未损坏；也可在“诊断”中关闭硬件解码后重试。\n日志位于：{}", logging::directory().display()));
                }
            }
        }
    }

    pub fn refresh_details(&mut self) {
        if self.details.last_refresh.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.details.last_refresh = Instant::now();
        self.details.tracks = self
            .property("track-list")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if self.details.diagnostics_open {
            let mut lines = vec![
                format!(
                    "轻巧播放器 {} / {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::ARCH
                ),
                "渲染：libmpv 软件 RGBA → egui 纹理（非 GPU 直绘）".into(),
                format!(
                    "缓冲：{} × {}，上限 1920 × 1080",
                    self.size[0], self.size[1]
                ),
                format!("播放库：{}", self.runtime.display()),
                format!(
                    "当前文件：{}",
                    self.selected
                        .and_then(|i| self.playlist.get(i))
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "无".into())
                ),
            ];
            for (label, property) in [
                ("mpv", "mpv-version"),
                ("视频编码", "video-codec"),
                ("实际硬件解码", "hwdec-current"),
                ("视频尺寸", "video-params"),
                ("源帧率", "container-fps"),
                ("A/V 差值（秒）", "avsync"),
                ("解码丢帧", "decoder-frame-drop-count"),
                ("输出丢帧", "frame-drop-count"),
                ("音频输出", "current-ao"),
            ] {
                lines.push(format!(
                    "{label}：{}",
                    self.property(property).unwrap_or_else(|| "暂无数据".into())
                ));
            }
            lines.push(format!("当前音轨：{}", self.selected_track_label("audio")));
            self.details.stats = lines.join("\n");
        }
    }

    pub fn details_windows(&mut self, ctx: &egui::Context) {
        let mut subtitles = self.details.subtitle_open;
        if subtitles {
            egui::Window::new("字幕设置")
                .open(&mut subtitles)
                .default_width(440.0)
                .show(ctx, |ui| {
                    if ui.button("加载外挂字幕…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("字幕文件", &["srt", "ass", "ssa", "vtt"])
                            .pick_file()
                        {
                            self.command(&["sub-add", &path.to_string_lossy(), "select"]);
                            self.details.last_refresh = Instant::now() - Duration::from_secs(1);
                        }
                    }
                    self.track_choices(ui, "sub", "sid");
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("字幕延迟（秒）");
                        let mut delay = self.details.sub_delay;
                        if ui
                            .add(
                                egui::DragValue::new(&mut delay)
                                    .speed(0.1)
                                    .range(-60.0..=60.0)
                                    .fixed_decimals(1),
                            )
                            .changed()
                            && self.command(&["set", "sub-delay", &delay.to_string()])
                        {
                            self.details.sub_delay = delay;
                        }
                        if ui.button("重置").clicked() && self.command(&["set", "sub-delay", "0"])
                        {
                            self.details.sub_delay = 0.0;
                        }
                    });
                    ui.label("正数让字幕晚出现，负数让字幕提前；切换视频后归零。");
                });
        }
        self.details.subtitle_open = subtitles;
        let mut audio = self.details.audio_open;
        if audio {
            egui::Window::new("音轨选择")
                .open(&mut audio)
                .default_width(440.0)
                .show(ctx, |ui| {
                    ui.label(format!("当前：{}", self.selected_track_label("audio")));
                    self.track_choices(ui, "audio", "aid");
                });
        }
        self.details.audio_open = audio;
        let mut diagnostics = self.details.diagnostics_open;
        if diagnostics {
            egui::Window::new("播放诊断")
                .open(&mut diagnostics)
                .default_width(560.0)
                .show(ctx, |ui| {
                    let mut hardware = self.details.hardware;
                    if ui
                        .checkbox(&mut hardware, "自动硬件解码（不可用时回退软件）")
                        .changed()
                    {
                        if self.command(&[
                            "set",
                            "hwdec",
                            if hardware { "auto-copy" } else { "no" },
                        ]) {
                            self.details.hardware = hardware;
                            logging::record(format!("硬件解码设置：{hardware}"));
                        }
                    }
                    ui.label("硬解是否生效以“实际硬件解码”为准；显示 no 表示软件解码。");
                    ui.horizontal(|ui| {
                        if ui.button("复制诊断信息").clicked() {
                            ctx.copy_text(self.details.stats.clone());
                        }
                        if ui.button("打开日志目录").clicked() {
                            if let Err(error) = std::process::Command::new("open")
                                .arg(logging::directory())
                                .spawn()
                            {
                                self.status = format!("日志目录打开失败：{error}");
                            }
                        }
                    });
                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&self.details.stats).monospace());
                        });
                    ui.label("CPU、温度和风扇请结合活动监视器及 Intel 实机对照记录。");
                });
        }
        self.details.diagnostics_open = diagnostics;
    }

    fn track_choices(&mut self, ui: &mut egui::Ui, kind: &str, property: &str) {
        let tracks = self
            .details
            .tracks
            .iter()
            .filter(|t| t["type"] == kind)
            .cloned()
            .collect::<Vec<_>>();
        if kind == "sub"
            && ui
                .radio(!tracks.iter().any(|t| t["selected"] == true), "关闭字幕")
                .clicked()
        {
            self.command(&["set", property, "no"]);
        }
        if tracks.is_empty() {
            ui.label("当前文件没有可用轨道");
        }
        egui::ScrollArea::vertical()
            .id_salt(kind)
            .max_height(220.0)
            .show(ui, |ui| {
                for track in tracks {
                    if let Some(id) = track["id"].as_i64() {
                        if ui
                            .radio(track["selected"] == true, track_label(&track))
                            .clicked()
                        {
                            self.command(&["set", property, &id.to_string()]);
                            self.details.last_refresh = Instant::now() - Duration::from_secs(1);
                        }
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires local libmpv and COMPACT_TEST_VIDEO fixture"]
    fn subtitles_and_async_failures() {
        let mut player = Player::new();
        let video = std::path::PathBuf::from(std::env::var_os("COMPACT_TEST_VIDEO").unwrap());
        player.playlist.push(video);
        player.play(0);
        let start = Instant::now();
        let mut pixels = vec![0; 320 * 180 * 4];
        while player.number("duration") == 0.0 {
            player
                .backend
                .as_ref()
                .unwrap()
                .render(320, 180, &mut pixels, false)
                .unwrap();
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        player.poll_backend();
        let subtitle = std::env::temp_dir().join(format!("compact-sub-{}.srt", std::process::id()));
        std::fs::write(&subtitle, "1\n00:00:00,000 --> 00:00:05,000\n字幕测试\n").unwrap();
        assert!(player.command(&["sub-add", &subtitle.to_string_lossy(), "select"]));
        assert!(player.command(&["set", "sub-delay", "0.5"]));
        assert_eq!(player.number("sub-delay"), 0.5);
        let start = Instant::now();
        loop {
            player.details.last_refresh = Instant::now() - Duration::from_secs(1);
            player.refresh_details();
            if player.details.tracks.iter().any(|t| t["type"] == "sub") {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(player.command(&["set", "sid", "no"]));
        player.playlist.push(subtitle.with_extension("missing.mp4"));
        player.play(1);
        let start = Instant::now();
        while player.notice.is_none() {
            player.poll_backend();
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!player.playing);
        assert!(player.notice.as_ref().unwrap().contains("无法播放"));
        std::fs::remove_file(subtitle).unwrap();
    }
    #[test]
    fn track_names_include_language_and_external_source() {
        let label = track_label(
            &serde_json::json!({"id":2,"title":"中文","lang":"zh","codec":"subrip","external":true}),
        );
        assert!(label.contains("中文") && label.contains("zh") && label.contains("外挂"));
    }
}
