mod mpv;

use eframe::egui;
use std::path::PathBuf;
use std::time::Duration;

struct Player {
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
            self.command(&["set", "pause", "no"]);
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
        let first = self.playlist.len();
        self.playlist.extend(paths);
        if first < self.playlist.len() {
            self.play(first);
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
            self.command(&["cycle", "pause"]);
        }
        for (key, offset) in [(egui::Key::ArrowLeft, "-5"), (egui::Key::ArrowRight, "5")] {
            if ctx.input(|input| input.key_pressed(key)) {
                self.command(&["seek", offset, "relative"]);
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
            ui.horizontal(|ui| {
                if ui.button("上一项").clicked() {
                    if let Some(index) = self.selected {
                        self.play(index.saturating_sub(1));
                    }
                }
                let paused = self.property("pause").as_deref() == Some("yes");
                if ui
                    .button(if paused || !self.playing {
                        "播放"
                    } else {
                        "暂停"
                    })
                    .clicked()
                {
                    if !self.playing {
                        if let Some(index) = self.selected {
                            self.play(index);
                        }
                    } else {
                        self.command(&["cycle", "pause"]);
                    }
                }
                if ui.button("停止").clicked() {
                    self.command(&["stop"]);
                    self.playing = false;
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
                                self.selected = None;
                                self.playing = false;
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
                let size = [
                    available.x.clamp(1.0, 1920.0) as usize,
                    available.y.clamp(1.0, 1080.0) as usize,
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
                    ui.add(egui::Image::new(texture).fit_to_exact_size(available));
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
