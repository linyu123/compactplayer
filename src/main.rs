mod capture;
mod decoder;
mod gl_video;
mod interaction;
mod macos_open;
mod media;
mod mpv;
mod playback;
mod ui;
mod video_layout;

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
    /// 非空时使用 OpenGL 零拷贝渲染，否则回退软件 RGBA 路径。
    gl: Option<std::sync::Arc<eframe::glow::Context>>,
    gl_video: Option<gl_video::GlVideo>,
    gl_ready: bool,
    video_image: Option<std::sync::Arc<egui::ColorImage>>,
    brand: Option<egui::TextureHandle>,
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
    interaction: interaction::Interaction,
    cursor_visible: Option<bool>,
    /// 当前内核是否已注册新帧回调。
    waker_installed: bool,
    mode: playback::Mode,
    eof_armed: bool,
    capture: capture::Capture,
    decoder: decoder::Decoder,
}

impl Player {
    #[cfg(test)]
    fn new() -> Self {
        Self::create(None)
    }

    fn create(gl: Option<std::sync::Arc<eframe::glow::Context>>) -> Self {
        let path = std::env::var_os("COMPACT_PLAYER_LIBMPV")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if let Ok(executable) = std::env::current_exe() {
                    if let Some(contents) =
                        executable.parent().and_then(|directory| directory.parent())
                    {
                        let bundled = contents.join("Frameworks/libmpv.dylib");
                        if bundled.is_file() {
                            return bundled;
                        }
                    }
                }
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(if cfg!(target_arch = "x86_64") {
                    "runtime/libmpv-libs_v0.7.3_macos-amd64-video-default/libmpv.dylib"
                } else {
                    "runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/libmpv.dylib"
                })
            });
        let (result, gl) = Self::load_backend(&path, true, gl);
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
            gl,
            gl_video: None,
            gl_ready: false,
            video_image: None,
            brand: None,
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
            interaction: interaction::Interaction::default(),
            cursor_visible: None,
            waker_installed: false,
            mode: playback::Mode::default(),
            eof_armed: false,
            capture: capture::Capture::default(),
            decoder: decoder::Decoder::default(),
        }
    }

    /// 有 OpenGL 上下文时优先创建零拷贝内核，失败则回退软件渲染。
    fn load_backend(
        path: &std::path::Path,
        hardware: bool,
        gl: Option<std::sync::Arc<eframe::glow::Context>>,
    ) -> (
        Result<mpv::Mpv, String>,
        Option<std::sync::Arc<eframe::glow::Context>>,
    ) {
        if gl.is_some() {
            if let Ok(backend) = mpv::Mpv::with_opengl(path, hardware) {
                return (Ok(backend), gl);
            }
        }
        (mpv::Mpv::with_hwdec(path, hardware), None)
    }

    fn replace_backend(&mut self, path: PathBuf) -> Result<(), String> {
        // 旧内核的 OpenGL 渲染上下文必须在当前 GL 上下文中释放，先于新内核创建。
        self.backend = None;
        let gl = self
            .gl
            .take()
            .or_else(|| self.gl_video.as_ref().map(|video| video.gl().clone()));
        let (result, gl) = Self::load_backend(&path, self.decoder.hardware, gl);
        self.gl = gl;
        self.backend = Some(result?);
        self.waker_installed = false;
        self.decoder.gamma_adjusted = false;
        self.runtime = path;
        Ok(())
    }

    fn clear_video(&mut self) {
        self.texture = None;
        self.gl_ready = false;
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
        // 滤镜会改写新文件的 gamma 标签，换文件前移除，加载后按实际源重新判断。
        if self.decoder.gamma_adjusted {
            self.command(&["vf", "remove", "@system-gamma"]);
            self.decoder.gamma_adjusted = false;
        }
        if self.command(&["loadfile", &path.to_string_lossy(), "replace"]) {
            self.eof_armed = false;
            self.capture.reset_range();
            self.rotation = 0;
            self.command(&["set", "pause", "no"]);
            self.command(&["set", "speed", "1"]);
            self.speed = 1.0;
            self.rewinding = false;
            self.selected = Some(index);
            self.playing = true;
            self.clear_video();
            self.capture.frame = None;
            self.status = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
        }
    }

    fn advance_after_eof(&mut self) {
        if !self.playing {
            return;
        }
        match self.property("eof-reached").as_deref() {
            Some("no") => self.eof_armed = true,
            Some("yes") if self.eof_armed => {
                self.eof_armed = false;
                if let Some(index) = self.selected {
                    if let Some(next) = self.mode.next(index, self.playlist.len()) {
                        self.play(next);
                    }
                }
            }
            _ => {}
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
        } else {
            let active = self.rewinding || self.property("pause").as_deref() != Some("yes");
            self.rewinding = false;
            self.speed = 1.0;
            self.command(&["set", "speed", "1"]);
            self.command(&["set", "pause", if active { "yes" } else { "no" }]);
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
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 退出时 GL 上下文仍有效，按顺序释放 mpv 渲染上下文和纹理。
        self.backend = None;
        if let Some(video) = self.gl_video.take() {
            video.destroy();
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let active = self.playing
            && self.property("pause").as_deref() != Some("yes")
            && self.property("eof-reached").as_deref() != Some("yes");
        if !self.waker_installed {
            if let Some(backend) = &mut self.backend {
                let ctx = ctx.clone();
                // request_repaint() 每次会多画一帧；非零延迟只画一次，按视频帧率重绘。
                backend.set_waker(move || ctx.request_repaint_after(Duration::from_millis(1)));
                self.waker_installed = true;
            }
        }
        // 新帧由 mpv 回调触发重绘；定时重绘只用于刷新进度和状态。
        ctx.request_repaint_after(Duration::from_millis(repaint_interval_ms(active)));
        self.refresh_decoder();
        if self.brand.is_none() {
            if let Ok(image) = image::load_from_memory(include_bytes!("../resources/AppIcon.png")) {
                let image = image
                    .resize(192, 192, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
                self.brand = Some(ctx.load_texture(
                    "brand",
                    egui::ColorImage::from_rgba_unmultiplied([192, 192], image.as_raw()),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
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
        let opened = macos_open::take();
        if !opened.is_empty() {
            self.add(opened);
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.advance_after_eof();
        let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        let active = self.playing
            && self.property("pause").as_deref() != Some("yes")
            && self.property("eof-reached").as_deref() != Some("yes");
        let chrome = self.interaction.show_controls(
            ctx,
            fullscreen,
            active,
            self.notice.is_some()
                || self.capture.open
                || self.capture.image_open
                || self.decoder.open,
        );
        self.shortcuts(ctx);
        // send_viewport_cmd 会立即请求重绘，每帧发送会让界面按屏幕刷新率空转。
        if self.cursor_visible != Some(chrome) {
            self.cursor_visible = Some(chrome);
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(chrome));
        }
        if chrome {
            self.toolbar(ctx);
            self.controls(ctx);
        }
        self.playlist_panel(ctx);
        self.capture_window(ctx);
        self.decoder_window(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let available = ui.available_size();
                let (render_size, size) = video_layout::layout(
                    available,
                    self.decoder.source_size,
                    if self.decoder.retina {
                        ctx.pixels_per_point()
                    } else {
                        1.0
                    },
                    self.rotation % 180 == 90,
                );
                let resized = self.size != size;
                self.size = size;
                let opengl = self.backend.as_ref().is_some_and(mpv::Mpv::is_opengl);
                if opengl && self.gl_video.is_none() {
                    if let Some(gl) = self.gl.clone() {
                        match gl_video::GlVideo::new(gl, frame) {
                            Ok(video) => self.gl_video = Some(video),
                            Err(error) => self.status = error,
                        }
                    }
                }
                if opengl {
                    self.pixels = Vec::new();
                    if let (true, Some(backend), Some(video)) =
                        (self.playing, &self.backend, &mut self.gl_video)
                    {
                        let result = video.resize(size).and_then(|resized| {
                            backend.render_opengl(
                                video.fbo(),
                                size[0],
                                size[1],
                                resized || !self.gl_ready,
                            )
                        });
                        video.finish();
                        match result {
                            Ok(true) => self.gl_ready = true,
                            Ok(false) => {}
                            Err(error) => self.status = error,
                        }
                    }
                } else {
                    self.pixels.resize(size[0] * size[1] * 4, 0);
                }
                if self.playing && !opengl {
                    if let Some(backend) = &self.backend {
                        match backend.render(
                            size[0],
                            size[1],
                            &mut self.pixels,
                            resized || self.texture.is_none(),
                        ) {
                            Ok(true) => {
                                let image = match self.video_image.as_mut() {
                                    Some(existing)
                                        if existing.size == size
                                            && std::sync::Arc::strong_count(existing) == 1 =>
                                    {
                                        std::sync::Arc::get_mut(existing).map(|existing| {
                                            video_layout::update_opaque_frame(
                                                existing,
                                                size,
                                                &self.pixels,
                                            );
                                        });
                                        existing.clone()
                                    }
                                    _ => std::sync::Arc::new(video_layout::opaque_frame(
                                        size,
                                        &self.pixels,
                                    )),
                                };
                                self.video_image = Some(image.clone());
                                if self.capture.image_open {
                                    self.capture.frame = Some(image.clone());
                                }
                                let image = egui::ImageData::Color(image);
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
                let texture = match (&self.gl_video, &self.texture) {
                    (Some(video), _) if self.gl_ready => Some(egui::load::SizedTexture::new(
                        video.id,
                        egui::vec2(video.size[0] as f32, video.size[1] as f32),
                    )),
                    (_, Some(texture)) if !opengl => {
                        Some(egui::load::SizedTexture::from_handle(texture))
                    }
                    _ => None,
                };
                if let Some(texture) = texture {
                    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click());
                    if response.double_clicked() {
                        self.toggle_fullscreen(ctx);
                    }
                    let image_rect = egui::Rect::from_center_size(rect.center(), render_size);
                    egui::Image::new(texture)
                        .fit_to_exact_size(render_size)
                        .rotate((self.rotation as f32).to_radians(), egui::vec2(0.5, 0.5))
                        .paint_at(ui, image_rect);
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space((available.y - 230.0).max(0.0) * 0.5);
                        if let Some(brand) = &self.brand {
                            ui.add(
                                egui::Image::new(brand).fit_to_exact_size(egui::vec2(96.0, 96.0)),
                            );
                        }
                        ui.label(
                            egui::RichText::new("轻巧播放器")
                                .size(24.0)
                                .strong()
                                .color(egui::Color32::from_rgb(225, 233, 242)),
                        );
                        ui.label(
                            egui::RichText::new("将视频拖入窗口，或选择一个文件开始播放")
                                .size(13.0)
                                .color(egui::Color32::GRAY),
                        );
                        ui.add_space(12.0);
                        if ui
                            .add(
                                egui::Button::new("打开媒体文件").min_size(egui::vec2(150.0, 38.0)),
                            )
                            .clicked()
                        {
                            self.open();
                        }
                    });
                }
            });
    }
}

fn repaint_interval_ms(active: bool) -> u64 {
    if active {
        250
    } else {
        100
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
    fn repaint_interval_only_refreshes_status() {
        assert_eq!(repaint_interval_ms(false), 100);
        assert_eq!(repaint_interval_ms(true), 250);
    }

    #[test]
    #[ignore = "requires local libmpv and COMPACT_TEST_VIDEO fixture"]
    fn automatic_playback_modes() {
        let video = PathBuf::from(std::env::var_os("COMPACT_TEST_VIDEO").unwrap());
        let mut player = Player::new();
        player.playlist = vec![video.clone(), video];
        let mut pixels = vec![0; 320 * 180 * 4];
        for (mode, current, expected, reload) in [
            (playback::Mode::Sequential, 0, 1, true),
            (playback::Mode::Sequential, 1, 1, false),
            (playback::Mode::RepeatOne, 1, 1, true),
            (playback::Mode::RepeatAll, 1, 0, true),
        ] {
            player.mode = mode;
            player.play(current);
            player.command(&["set", "mute", "yes"]);
            player.command(&["set", "speed", "10"]);
            let start = std::time::Instant::now();
            loop {
                player
                    .backend
                    .as_ref()
                    .unwrap()
                    .render(320, 180, &mut pixels, false)
                    .unwrap();
                if player.property("eof-reached").as_deref() == Some("no") {
                    player.advance_after_eof();
                }
                if player.property("eof-reached").as_deref() == Some("yes") && player.eof_armed {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(10));
            }
            player.advance_after_eof();
            assert_eq!(player.selected, Some(expected), "{mode:?}");
            assert!(!player.eof_armed);
            if reload {
                while player.property("eof-reached").as_deref() != Some("no")
                    || player.number("time-pos") <= 0.0
                {
                    player
                        .backend
                        .as_ref()
                        .unwrap()
                        .render(320, 180, &mut pixels, false)
                        .unwrap();
                    assert!(start.elapsed() < Duration::from_secs(15));
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(player.number("time-pos") < 1.0);
            } else {
                for _ in 0..5 {
                    player.advance_after_eof();
                }
                assert_eq!(player.property("eof-reached").as_deref(), Some("yes"));
            }
            player.command(&["stop"]);
        }
    }

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
        player.play(0);
        player.toggle_play();
        assert_eq!(player.property("pause").as_deref(), Some("yes"));
        player.toggle_play();
        assert_eq!(player.property("pause").as_deref(), Some("no"));
        player.command(&["stop"]);
    }
}

fn main() -> eframe::Result {
    macos_open::install();
    eframe::run_native(
        "轻巧播放器",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1120.0, 700.0])
                .with_min_inner_size([960.0, 540.0]),
            ..Default::default()
        },
        Box::new(|creation| {
            ui::configure(&creation.egui_ctx);
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
            // COMPACT_PLAYER_RENDER=software 强制旧的软件渲染路径，便于对比和排查。
            let gl = creation
                .gl
                .clone()
                .filter(|_| std::env::var("COMPACT_PLAYER_RENDER").as_deref() != Ok("software"));
            let mut player = Player::create(gl);
            player.add(
                std::env::args_os()
                    .skip(1)
                    .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
                    .map(PathBuf::from)
                    .collect(),
            );
            Ok(Box::new(player))
        }),
    )
}
