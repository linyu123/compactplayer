mod media;
mod mpv;

use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Intake {
    paths: Vec<PathBuf>,
}

struct Player {
    runtime: PathBuf,
    backend: Option<mpv::Mpv>,
    playlist: Vec<PathBuf>,
    selected: Option<usize>,
    texture: Option<egui::TextureHandle>,
    pixels: Vec<u8>,
    size: [usize; 2],
    status: String,
    volume: f32,
    show_playlist: bool,
    playing: bool,
    rotation: u16,
    intake: Option<Receiver<Result<Intake, String>>>,
    notice: Option<String>,
    speed: f64,
    rewinding: bool,
    last_rewind: std::time::Instant,
}

impl Player {
    fn new() -> Self {
        let path = std::env::var_os("COMPACT_PLAYER_LIBMPV")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/libmpv.dylib")
            });
        let result = mpv::Mpv::new(&path);
        let status = match &result {
            Ok(backend) => format!(
                "准备就绪 / {}",
                backend.property("mpv-version").unwrap_or_default()
            ),
            Err(error) => format!("播放内核加载失败：{error}"),
        };
        Self {
            runtime: path,
            backend: result.ok(),
            playlist: Vec::new(),
            selected: None,
            texture: None,
            pixels: Vec::new(),
            size: [0, 0],
            status,
            volume: 75.0,
            show_playlist: true,
            playing: false,
            rotation: 0,
            intake: None,
            notice: None,
            speed: 1.0,
            rewinding: false,
            last_rewind: std::time::Instant::now(),
        }
    }

    fn command(&mut self, values: &[&str]) -> bool {
        let Some(backend) = &self.backend else {
            self.status = "播放内核不可用，请点击“播放内核”选择运行库。".into();
            return false;
        };
        match backend.command(values) {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    fn property(&self, name: &str) -> Option<String> {
        self.backend.as_ref()?.property(name)
    }
    fn number(&self, name: &str) -> f64 {
        self.property(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0.0)
    }

    fn play(&mut self, index: usize) {
        let Some(path) = self.playlist.get(index).cloned() else {
            return;
        };
        if self.command(&["loadfile", &path.to_string_lossy(), "replace"]) {
            self.rotation = 0;
            self.command(&["set", "pause", "no"]);
            self.command(&["set", "speed", "1"]);
            self.speed = 1.0;
            self.rewinding = false;
            self.selected = Some(index);
            self.playing = true;
            self.texture = None;
            self.status = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
        }
    }

    fn add(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for path in paths {
            if media::is_media(&path) {
                let path = path.canonicalize().unwrap_or(path);
                if !self.playlist.contains(&path) {
                    self.playlist.push(path.clone());
                }
                accepted.push(path);
            } else {
                rejected.push(path.display().to_string());
            }
        }
        if let Some(path) = accepted.first() {
            if let Some(index) = self.playlist.iter().position(|item| item == path) {
                self.play(index);
            }
        }
        if !rejected.is_empty() {
            self.notice = Some(format!(
                "不支持的文件类型，不能播放：\n{}",
                rejected.join("\n")
            ));
        }
        if accepted.is_empty() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.intake = Some(receiver);
        std::thread::spawn(move || {
            let paths = media::candidates(accepted)
                .into_iter()
                .filter_map(|(path, explicit)| if explicit { None } else { Some(path) })
                .collect();
            let _ = sender.send(Ok(Intake { paths }));
        });
    }

    fn toggle_play(&mut self) {
        if !self.playing || self.property("eof-reached").as_deref() == Some("yes") {
            if let Some(index) = self.selected {
                self.play(index);
            }
        } else if self.rewinding || self.speed != 1.0 {
            self.rewinding = false;
            self.speed = 1.0;
            self.command(&["set", "speed", "1"]);
            self.command(&["set", "pause", "no"]);
        } else {
            self.command(&["cycle", "pause"]);
        }
    }

    fn seek(&mut self, offset: f64) {
        if !self.playing {
            return;
        }
        let position =
            (self.number("time-pos") + offset).clamp(0.0, self.number("duration").max(0.0));
        self.command(&["seek", &position.to_string(), "absolute+exact"]);
    }

    fn shuttle(&mut self, reverse: bool) {
        if !self.playing {
            return;
        }
        let same_direction = self.rewinding == reverse;
        let speeds = [2.0, 5.0, 10.0, 30.0, 60.0];
        self.speed = if same_direction {
            speeds
                .into_iter()
                .find(|speed| *speed > self.speed)
                .unwrap_or(2.0)
        } else {
            2.0
        };
        self.rewinding = reverse;
        self.last_rewind = std::time::Instant::now();
        if reverse {
            self.command(&["set", "pause", "yes"]);
        } else {
            self.command(&["set", "speed", &self.speed.to_string()]);
            self.command(&["set", "pause", "no"]);
        }
    }

    fn open(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("打开媒体文件")
            .pick_files()
        {
            self.add(paths);
        }
    }

    fn audio_tracks(&self) -> Vec<serde_json::Value> {
        self.property("track-list")
            .and_then(|value| serde_json::from_str::<Vec<serde_json::Value>>(&value).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|track| track["type"] == "audio")
            .collect()
    }

    fn switch_audio(&mut self) {
        let tracks = self.audio_tracks();
        if tracks.is_empty() {
            self.status = "当前文件没有音轨".into();
            return;
        }
        let current = tracks.iter().position(|track| track["selected"] == true);
        let next = current.map_or(0, |index| (index + 1) % tracks.len());
        if let Some(id) = tracks[next]["id"].as_i64() {
            self.command(&["set", "aid", &id.to_string()]);
        }
    }
}

impl eframe::App for Player {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));
        if let Some(result) = self
            .intake
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.intake = None;
            match result {
                Ok(result) => {
                    for path in result.paths {
                        if !self.playlist.contains(&path) {
                            self.playlist.push(path);
                        }
                    }
                }
                Err(error) => self.notice = Some(format!("文件检测失败：{error}")),
            }
        }
        if self.rewinding
            && self.playing
            && self.last_rewind.elapsed() >= Duration::from_millis(100)
        {
            let elapsed = self.last_rewind.elapsed().as_secs_f64();
            self.last_rewind = std::time::Instant::now();
            self.seek(-elapsed * self.speed);
            if self.number("time-pos") <= 0.0 {
                self.rewinding = false;
                self.speed = 1.0;
            }
        }
        if self.notice.is_some() {
            egui::Window::new("播放提示")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            ui.label(self.notice.as_deref().unwrap_or_default());
                        });
                    if ui.button("知道了").clicked() {
                        self.notice = None;
                    }
                });
        }
        let paths = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect::<Vec<_>>()
        });
        if !paths.is_empty() {
            self.add(paths);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Space)) {
            self.toggle_play();
        }
        for (key, offset) in [(egui::Key::ArrowLeft, "-5"), (egui::Key::ArrowRight, "5")] {
            if ctx.input(|input| input.key_pressed(key)) {
                self.seek(offset.parse().unwrap_or(0.0));
            }
        }
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("轻巧播放器")
                        .strong()
                        .color(egui::Color32::LIGHT_BLUE),
                );
                if ui.button("打开").clicked() {
                    self.open();
                }
                if ui.button("播放内核").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("选择 libmpv 播放内核")
                        .add_filter("播放内核动态库", &["dylib"])
                        .pick_file()
                    {
                        match mpv::Mpv::new(&path) {
                            Ok(backend) => {
                                self.runtime = path;
                                self.backend = Some(backend);
                                self.playing = false;
                                self.texture = None;
                                self.status = "播放内核已就绪".into();
                            }
                            Err(error) => self.status = error,
                        }
                    }
                }
                ui.toggle_value(&mut self.show_playlist, "播放列表");
                if ui.button("字幕").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("加载字幕文件")
                        .add_filter("字幕文件", &["srt", "ass", "ssa", "vtt"])
                        .pick_file()
                    {
                        self.command(&["sub-add", &path.to_string_lossy(), "select"]);
                    }
                }
                if ui.button("切换音轨").clicked() {
                    self.switch_audio();
                }
                if ui.button("全屏").clicked() {
                    let fullscreen =
                        ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                if ui
                    .add_enabled(self.playing, egui::Button::new("旋转 90°"))
                    .on_hover_text("每次顺时针旋转 90°，连续点击两次可纠正倒拍视频")
                    .clicked()
                {
                    self.rotation = (self.rotation + 90) % 360;
                }
            });
        });
        egui::TopBottomPanel::bottom("controls").show(ctx, |ui| {
            let mut position = self.number("time-pos");
            let duration = self.number("duration");
            ui.spacing_mut().slider_width = ui.available_width();
            if ui
                .add(egui::Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false))
                .changed()
            {
                self.command(&["seek", &position.to_string(), "absolute"]);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("上一项").clicked() {
                    if let Some(index) = self.selected {
                        self.play(index.saturating_sub(1));
                    }
                }
                let paused = self.property("pause").as_deref() == Some("yes");
                let ended = self.property("eof-reached").as_deref() == Some("yes");
                if ui
                    .button(if ended {
                        "重播"
                    } else if self.rewinding || self.speed != 1.0 {
                        "正常播放"
                    } else if paused || !self.playing {
                        "播放"
                    } else {
                        "暂停"
                    })
                    .clicked()
                {
                    self.toggle_play();
                }
                if ui.button("停止").clicked() {
                    self.command(&["stop"]);
                    self.playing = false;
                    self.rewinding = false;
                    self.texture = None;
                    self.status = "已停止".into();
                }
                if ui.button("下一项").clicked() {
                    if let Some(index) = self.selected {
                        self.play(index + 1);
                    }
                }
                ui.monospace(format!("{} / {}", timestamp(position), timestamp(duration)));
                ui.spacing_mut().slider_width = 100.0;
                let muted = self.property("mute").as_deref() == Some("yes");
                if ui.button(if muted { "取消静音" } else { "静音" }).clicked() {
                    self.command(&["cycle", "mute"]);
                }
                ui.label("音量");
                if ui
                    .add(egui::Slider::new(&mut self.volume, 0.0..=100.0).suffix("%"))
                    .changed()
                {
                    self.command(&["set", "volume", &self.volume.to_string()]);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("后退 10 秒").clicked() {
                    self.seek(-10.0);
                }
                if ui
                    .button("快退")
                    .on_hover_text("连续点击切换 2、5、10、30、60 倍；点击播放恢复正常速度")
                    .clicked()
                {
                    self.shuttle(true);
                }
                if ui
                    .button("快进")
                    .on_hover_text("连续点击切换 2、5、10、30、60 倍；点击播放恢复正常速度")
                    .clicked()
                {
                    self.shuttle(false);
                }
                if ui.button("前进 10 秒").clicked() {
                    self.seek(10.0);
                }
                ui.label(format!(
                    "{}{} 倍速",
                    if self.rewinding { "倒退 " } else { "" },
                    self.speed
                ));
                if self.intake.is_some() {
                    ui.spinner();
                    ui.label("正在扫描同目录视频");
                }
            });
            ui.label(egui::RichText::new(&self.status).small());
            if self.playing && duration > 0.0 {
                let audio_status = if self.audio_tracks().is_empty() {
                    "当前文件没有音轨".to_string()
                } else if self.property("aid").as_deref() == Some("no") {
                    "音轨已关闭，请点击“切换音轨”恢复".to_string()
                } else if self.property("mute").as_deref() == Some("yes") {
                    "已静音".to_string()
                } else if self.volume == 0.0 {
                    "音量为零".to_string()
                } else {
                    match self.property("current-ao") {
                        Some(output) if output != "null" => format!("音频输出正常（{output}）"),
                        _ => "音频输出未就绪，请检查系统输出设备".to_string(),
                    }
                };
                ui.label(egui::RichText::new(audio_status).small());
            }
        });
        if self.show_playlist {
            egui::SidePanel::right("playlist")
                .default_width(200.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("播放列表 / {} 项", self.playlist.len()));
                        if ui
                            .add_enabled(!self.playlist.is_empty(), egui::Button::new("清空"))
                            .clicked()
                        {
                            if self.backend.is_none() || self.command(&["stop"]) {
                                self.playlist.clear();
                                self.intake = None;
                                self.selected = None;
                                self.playing = false;
                                self.rewinding = false;
                                self.texture = None;
                                self.status = "播放列表已清空".into();
                            }
                        }
                    });
                    let mut selected = None;
                    let mut removed = None;
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (index, path) in self.playlist.iter().enumerate() {
                            let response = ui.selectable_label(
                                self.selected == Some(index),
                                path.file_name().unwrap_or_default().to_string_lossy(),
                            );
                            if response.clicked() {
                                selected = Some(index);
                            }
                            response.context_menu(|ui| {
                                if ui.button("删除").clicked() {
                                    removed = Some(index);
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                    if let Some(index) = removed {
                        let is_current = self.selected == Some(index);
                        if !is_current || self.backend.is_none() || self.command(&["stop"]) {
                            self.playlist.remove(index);
                            if is_current {
                                self.selected = None;
                                self.playing = false;
                                self.rewinding = false;
                                self.texture = None;
                                self.status = "当前播放项已从列表删除".into();
                            } else if let Some(current) = self.selected {
                                if index < current {
                                    self.selected = Some(current - 1);
                                }
                            }
                        }
                    } else if let Some(index) = selected {
                        self.play(index);
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let available = ui.available_size();
                let render_size = if self.rotation % 180 == 90 {
                    egui::vec2(available.y, available.x)
                } else {
                    available
                };
                let scale = (1920.0 / render_size.x.max(1.0))
                    .min(1080.0 / render_size.y.max(1.0))
                    .min(1.0);
                let size = [
                    (render_size.x * scale).max(1.0) as usize,
                    (render_size.y * scale).max(1.0) as usize,
                ];
                let resized = self.size != size;
                self.size = size;
                self.pixels.resize(size[0] * size[1] * 4, 0);
                if self.playing {
                    if let Some(backend) = &self.backend {
                        match backend.render(
                            size[0],
                            size[1],
                            &mut self.pixels,
                            resized || self.texture.is_none(),
                        ) {
                            Ok(true) => {
                                let image =
                                    egui::ColorImage::from_rgba_unmultiplied(size, &self.pixels);
                                if let Some(texture) = &mut self.texture {
                                    texture.set(image, egui::TextureOptions::LINEAR);
                                } else {
                                    self.texture = Some(ctx.load_texture(
                                        "video",
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    ));
                                }
                            }
                            Ok(false) => {}
                            Err(error) => self.status = error,
                        }
                    }
                }
                if let Some(texture) = &self.texture {
                    let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
                    let image_rect = egui::Rect::from_center_size(rect.center(), render_size);
                    egui::Image::new(texture)
                        .fit_to_exact_size(render_size)
                        .rotate((self.rotation as f32).to_radians(), egui::vec2(0.5, 0.5))
                        .paint_at(ui, image_rect);
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(available.y * 0.38);
                        ui.label(
                            egui::RichText::new("轻巧播放器")
                                .size(32.0)
                                .color(egui::Color32::LIGHT_BLUE),
                        );
                        ui.label("将视频文件拖到这里");
                        if ui.button("打开媒体文件").clicked() {
                            self.open();
                        }
                    });
                }
            });
    }
}

fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local libmpv and COMPACT_TEST_VIDEO fixture"]
    fn detects_invalid_media_and_replays_after_eof() {
        let video =
            PathBuf::from(std::env::var_os("COMPACT_TEST_VIDEO").expect("test video required"));
        let mut player = Player::new();
        let probe = mpv::Mpv::probe(&player.runtime).unwrap();
        assert!(probe.accepts_media(&video));
        let invalid =
            std::env::temp_dir().join(format!("compact-invalid-{}.mp4", std::process::id()));
        std::fs::write(&invalid, b"This is text, not a video").unwrap();
        assert!(!probe.accepts_media(&invalid));
        std::fs::remove_file(invalid).unwrap();
        player.playlist.push(video);
        player.play(0);
        player.command(&["set", "mute", "yes"]);
        let mut pixels = vec![0; 320 * 180 * 4];
        let start = std::time::Instant::now();
        while player.property("eof-reached").as_deref() != Some("yes") {
            player
                .backend
                .as_ref()
                .unwrap()
                .render(320, 180, &mut pixels, false)
                .unwrap();
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        player.toggle_play();
        let start = std::time::Instant::now();
        loop {
            player
                .backend
                .as_ref()
                .unwrap()
                .render(320, 180, &mut pixels, false)
                .unwrap();
            if player.number("time-pos") > 0.1
                && player.number("time-pos") < 1.0
                && player.property("eof-reached").as_deref() == Some("no")
            {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        player.shuttle(false);
        assert_eq!(
            player.property("speed").unwrap().parse::<f64>().unwrap(),
            2.0
        );
        player.shuttle(false);
        assert_eq!(
            player.property("speed").unwrap().parse::<f64>().unwrap(),
            5.0
        );
        player.toggle_play();
        assert_eq!(
            player.property("speed").unwrap().parse::<f64>().unwrap(),
            1.0
        );
        player.command(&["stop"]);
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "轻巧播放器",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 590.0])
                .with_min_inner_size([760.0, 420.0]),
            ..Default::default()
        },
        Box::new(|creation| {
            creation.egui_ctx.set_theme(egui::ThemePreference::Dark);
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(data) = std::fs::read("/System/Library/Fonts/PingFang.ttc") {
                fonts
                    .font_data
                    .insert("chinese".into(), egui::FontData::from_owned(data));
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .push("chinese".into());
                }
                creation.egui_ctx.set_fonts(fonts);
            }
            let mut player = Player::new();
            player.add(std::env::args_os().skip(1).map(PathBuf::from).collect());
            Ok(Box::new(player))
        }),
    )
}
