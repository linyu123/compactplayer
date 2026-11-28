use crate::Player;
use eframe::egui;
use std::time::{Duration, Instant};

pub struct Decoder {
    pub open: bool,
    pub hardware: bool,
    pub label: String,
    pub source_size: [f32; 2],
    pub fps: f64,
    pub retina: bool,
    pub gamma_adjusted: bool,
    stats: String,
    refresh: Instant,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            open: false,
            hardware: true,
            label: "解码：等待视频".into(),
            source_size: [1920.0, 1080.0],
            fps: 0.0,
            retina: true,
            gamma_adjusted: false,
            stats: String::new(),
            refresh: Instant::now() - Duration::from_secs(1),
        }
    }
}

/// 返回需要切换到的状态：Some(true) 添加滤镜，Some(false) 移除，None 不变。
/// 滤镜生效后 mpv 报告的 gamma 为 gamma2.0，此时保持已添加状态。
fn system_gamma_filter(gamma: &str, adjusted: bool) -> Option<bool> {
    let wanted = match gamma {
        "" => return None,
        "bt.1886" => true,
        "gamma2.0" => adjusted,
        _ => false,
    };
    (wanted != adjusted).then_some(wanted)
}

fn hardware_decoder_active(actual: &str) -> bool {
    actual.starts_with("videotoolbox")
}

fn effective_source_size(playing: bool, video: &str, width: f32, height: f32) -> [f32; 2] {
    if !playing || video.is_empty() {
        [1920.0, 1080.0]
    } else if width > 0.0 && height > 0.0 {
        [width, height]
    } else {
        [1920.0, 1080.0]
    }
}

impl Player {
    pub fn refresh_decoder(&mut self) {
        if self.decoder.refresh.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.decoder.refresh = Instant::now();
        let actual = self
            .property("hwdec-current")
            .unwrap_or_else(|| "no".into());
        let video = self.property("video-codec").unwrap_or_default();
        let width = self.number("video-out-params/dw") as f32;
        let height = self.number("video-out-params/dh") as f32;
        self.decoder.source_size = effective_source_size(self.playing, &video, width, height);
        self.match_system_gamma();
        let fps = self.number("container-fps");
        self.decoder.fps = if fps.is_finite() && fps > 0.0 {
            fps
        } else {
            0.0
        };
        self.decoder.label = if !self.playing || video.is_empty() {
            "解码：等待视频".into()
        } else if hardware_decoder_active(&actual) {
            "解码：VideoToolbox 硬解".into()
        } else if self.decoder.hardware {
            "解码：软件（硬解不可用）".into()
        } else {
            "解码：软件".into()
        };
        if self.decoder.open {
            let mut lines = vec![
                format!(
                    "轻巧播放器 {} / {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::ARCH
                ),
                format!("请求硬解：{}", self.decoder.hardware),
                format!("实际解码：{actual}"),
                format!("视频编码：{video}"),
                format!(
                    "画面输出：{}，{}×{}，最高 4K 像素预算",
                    if self
                        .backend
                        .as_ref()
                        .is_some_and(crate::mpv::Mpv::is_opengl)
                    {
                        "OpenGL 零拷贝纹理"
                    } else {
                        "软件 RGBA → 纹理"
                    },
                    self.size[0],
                    self.size[1]
                ),
            ];
            for (label, name) in [
                ("视频尺寸", "video-params"),
                ("源帧率", "container-fps"),
                ("A/V 差值（秒）", "avsync"),
                ("解码丢帧", "decoder-frame-drop-count"),
                ("输出丢帧", "frame-drop-count"),
            ] {
                lines.push(format!(
                    "{label}：{}",
                    self.property(name).unwrap_or_else(|| "暂无数据".into())
                ));
            }
            self.decoder.stats = lines.join("\n");
        }
    }

    /// macOS 系统播放器把未标注或 bt.1886 的 SDR 视频按约 2.0 的 gamma 显示，
    /// mpv 默认按 bt.1886 解释，画面明显偏暗。只调整这类源，HDR 等其他传输特性保持不变。
    fn match_system_gamma(&mut self) {
        let gamma = self.property("video-params/gamma").unwrap_or_default();
        let want = system_gamma_filter(&gamma, self.decoder.gamma_adjusted);
        if let Some(adjust) = want {
            let ok = if adjust {
                self.command(&["vf", "add", "@system-gamma:format=gamma=gamma2.0"])
            } else {
                self.command(&["vf", "remove", "@system-gamma"])
            };
            if ok {
                self.decoder.gamma_adjusted = adjust;
            }
        }
    }

    pub fn decoder_window(&mut self, ctx: &egui::Context) {
        let mut open = self.decoder.open;
        egui::Window::new("解码设置 · Intel 优化测试版")
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                let mut hardware = self.decoder.hardware;
                if ui
                    .checkbox(&mut hardware, "优先使用 VideoToolbox 硬件解码")
                    .changed()
                {
                    let option = match (&self.backend, hardware) {
                        (Some(backend), true) => backend.hardware_decoder(),
                        _ => "no",
                    };
                    if self.command(&["set", "hwdec", option]) {
                        self.decoder.hardware = hardware;
                        self.decoder.refresh = Instant::now() - Duration::from_secs(1);
                    }
                }
                ui.label("可在播放中切换对比；切换可能短暂停顿。不支持的编码自动回退软件。");
                ui.label("旧 Intel 优先测试 H.264；HEVC、VP9、AV1 的硬件支持取决于机器。");
                ui.label("设置仅在本次运行有效。若画面异常，请关闭硬解后重试。");
                ui.checkbox(
                    &mut self.decoder.retina,
                    "Retina 清晰显示（关闭可降低渲染分辨率）",
                );
                ui.label(
                    "清晰模式渲染更多像素，软件输出下可能增加 CPU；不在 CPU 上放大低分辨率源。",
                );
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(&self.decoder.stats).monospace());
                    });
                if ui.button("复制诊断信息").clicked() {
                    ctx.copy_text(self.decoder.stats.clone());
                }
            });
        self.decoder.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_videotoolbox_status_variants() {
        assert!(hardware_decoder_active("videotoolbox-copy"));
        assert!(hardware_decoder_active("videotoolbox"));
        assert!(!hardware_decoder_active("no"));
        assert!(!hardware_decoder_active(""));
    }

    #[test]
    fn adjusts_only_sdr_bt1886_gamma() {
        assert_eq!(system_gamma_filter("bt.1886", false), Some(true));
        assert_eq!(system_gamma_filter("bt.1886", true), None);
        assert_eq!(system_gamma_filter("gamma2.0", true), None);
        assert_eq!(system_gamma_filter("pq", false), None);
        assert_eq!(system_gamma_filter("pq", true), Some(false));
        assert_eq!(system_gamma_filter("hlg", true), Some(false));
        assert_eq!(system_gamma_filter("", true), None);
    }

    #[test]
    fn resets_source_size_when_video_is_inactive() {
        assert_eq!(
            effective_source_size(false, "h264", 1080.0, 1920.0),
            [1920.0, 1080.0]
        );
        assert_eq!(
            effective_source_size(true, "", 1080.0, 1920.0),
            [1920.0, 1080.0]
        );
        assert_eq!(
            effective_source_size(true, "h264", 1080.0, 1920.0),
            [1080.0, 1920.0]
        );
        assert_eq!(
            effective_source_size(true, "h264", 0.0, 0.0),
            [1920.0, 1080.0]
        );
    }

    #[test]
    #[ignore = "requires libmpv and COMPACT_H264_VIDEO"]
    fn switches_hardware_and_software_without_losing_playback() {
        let mut player = Player::new();
        player.playlist.push(std::path::PathBuf::from(
            std::env::var_os("COMPACT_H264_VIDEO").unwrap(),
        ));
        player.play(0);
        player.command(&["set", "mute", "yes"]);
        let mut pixels = vec![0; 320 * 180 * 4];
        for (option, expected) in [
            ("videotoolbox-copy", "videotoolbox-copy"),
            ("no", "no"),
            ("videotoolbox-copy", "videotoolbox-copy"),
        ] {
            assert!(player.command(&["set", "hwdec", option]));
            let start = Instant::now();
            loop {
                player
                    .backend
                    .as_ref()
                    .unwrap()
                    .render(320, 180, &mut pixels, true)
                    .unwrap();
                if match player.property("hwdec-current") {
                    Some(actual) if expected == "no" => actual == "no",
                    Some(actual) => hardware_decoder_active(&actual),
                    None => false,
                } && pixels
                    .chunks_exact(4)
                    .any(|p| p[0] > 20 || p[1] > 20 || p[2] > 20)
                {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "实际解码：{:?}",
                    player.property("hwdec-current")
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        player.command(&["stop"]);
    }
}
