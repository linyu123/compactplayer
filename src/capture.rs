use crate::Player;
use eframe::egui;
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub struct Capture {
    pub open: bool,
    pub image_open: bool,
    image_message: String,
    pub frame: Option<std::sync::Arc<egui::ColorImage>>,
    pub start: f64,
    pub end: f64,
    start_text: String,
    end_text: String,
    selected_boundary: Option<bool>, // false = 开始，true = 结束
    precise: bool,
    remove_ad: bool,
    display: bool,
    ffmpeg: PathBuf,
    job: Option<Job>,
    message: String,
    progress: f32,
}

struct Job {
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<Event>,
    worker: Option<std::thread::JoinHandle<()>>,
}
enum Event {
    Progress(f32),
    Done(Result<PathBuf, String>),
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Default for Capture {
    fn default() -> Self {
        let bundled = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("ffmpeg")));
        let ffmpeg = bundled
            .into_iter()
            .chain([
                PathBuf::from("/opt/homebrew/bin/ffmpeg"),
                PathBuf::from("/usr/local/bin/ffmpeg"),
                PathBuf::from("/usr/bin/ffmpeg"),
            ])
            .find(|p| p.is_file())
            .unwrap_or_default();
        Self {
            open: false,
            image_open: false,
            image_message: String::new(),
            frame: None,
            start: 0.0,
            end: 0.0,
            start_text: timecode(0.0),
            end_text: timecode(0.0),
            selected_boundary: None,
            precise: false,
            remove_ad: false,
            display: false,
            ffmpeg,
            job: None,
            message: String::new(),
            progress: 0.0,
        }
    }
}

impl Capture {
    pub fn reset_range(&mut self) {
        self.start = 0.0;
        self.end = 0.0;
        self.start_text = timecode(0.0);
        self.end_text = timecode(0.0);
        self.selected_boundary = None;
    }

    pub fn set_from_slider(&mut self, position: f64) {
        if !self.open || self.job.is_some() || !position.is_finite() || position < 0.0 {
            return;
        }
        match self.selected_boundary {
            Some(false) => {
                self.start = position;
                self.start_text = timecode(position);
            }
            Some(true) => {
                self.end = position;
                self.end_text = timecode(position);
            }
            None => {}
        }
    }
}

fn timecode(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

fn parse_timecode(text: &str) -> Option<f64> {
    let parts = text.trim().split(':').collect::<Vec<_>>();
    let value = match parts.as_slice() {
        [seconds] => seconds.parse::<f64>().ok()?,
        [hours, minutes, seconds] => {
            let hours = hours.parse::<u32>().ok()? as f64;
            let minutes = minutes.parse::<u32>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            if minutes >= 60 || !(0.0..60.0).contains(&seconds) {
                return None;
            }
            hours * 3600.0 + minutes as f64 * 60.0 + seconds
        }
        _ => return None,
    };
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn valid_range(start: f64, end: f64, duration: f64) -> bool {
    start.is_finite()
        && end.is_finite()
        && duration.is_finite()
        && start >= 0.0
        && end > start
        && end <= duration
}

fn clip_args(
    input: &Path,
    start: f64,
    end: f64,
    precise: bool,
    output: &Path,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-n".into(),
        "-ss".into(),
        start.to_string().into(),
        "-i".into(),
        input.as_os_str().to_owned(),
        "-t".into(),
        (end - start).to_string().into(),
        "-map".into(),
        "0:v:0?".into(),
        "-map".into(),
        "0:a:0?".into(),
    ];
    let codec = if precise {
        vec![
            "-c:v", "libx264", "-preset", "fast", "-crf", "18", "-c:a", "aac", "-b:a", "192k",
        ]
    } else {
        vec!["-c", "copy"]
    };
    args.extend(codec.into_iter().map(Into::into));
    args.extend(
        [
            "-avoid_negative_ts",
            "disabled",
            "-use_editlist",
            "1",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(Into::into),
    );
    args.push(output.as_os_str().to_owned());
    args
}

fn remove_args(
    input: &Path,
    start: f64,
    end: f64,
    audio: bool,
    output: &Path,
) -> Vec<std::ffi::OsString> {
    let mut filter = format!("[0:v:0]split=2[vpre][vpost];[vpre]trim=end={start},setpts=PTS-STARTPTS[v0];[vpost]trim=start={end},setpts=PTS-STARTPTS[v1];");
    if audio {
        filter.push_str(&format!("[0:a:0]asplit=2[apre][apost];[apre]atrim=end={start},asetpts=PTS-STARTPTS[a0];[apost]atrim=start={end},asetpts=PTS-STARTPTS[a1];[v0][a0][v1][a1]concat=n=2:v=1:a=1[v][a]"));
    } else {
        filter.push_str("[v0][v1]concat=n=2:v=1:a=0[v]");
    }
    let mut args = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-n".into(),
        "-i".into(),
        input.as_os_str().to_owned(),
        "-filter_complex".into(),
        filter.into(),
        "-map".into(),
        "[v]".into(),
    ];
    if audio {
        args.extend(["-map", "[a]"].into_iter().map(Into::into));
    }
    args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(Into::into),
    );
    args.push(output.as_os_str().to_owned());
    args
}

fn fast_remove_args(
    input: &Path,
    start: f64,
    end: f64,
    list: &Path,
    output: &Path,
) -> Result<Vec<std::ffi::OsString>, String> {
    let input = input
        .canonicalize()
        .map_err(|e| format!("源文件不可读取：{e}"))?;
    let name = input.to_str().ok_or("快速拼接暂不支持非 UTF-8 文件路径")?;
    if name.contains(['\n', '\r']) {
        return Err("快速拼接不支持文件名中的换行符".into());
    }
    let name = name.replace('\'', "'\\''");
    let content = format!("ffconcat version 1.0\nfile '{name}'\ninpoint 0\noutpoint {start}\nfile '{name}'\ninpoint {end}\n");
    std::fs::write(list, content).map_err(|e| e.to_string())?;
    let mut args = [
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-n",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
    ]
    .into_iter()
    .map(Into::into)
    .collect::<Vec<std::ffi::OsString>>();
    args.push(list.as_os_str().to_owned());
    args.extend(
        [
            "-map",
            "0:v:0?",
            "-map",
            "0:a:0?",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "disabled",
            "-use_editlist",
            "1",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
        ]
        .into_iter()
        .map(Into::into),
    );
    args.push(output.as_os_str().to_owned());
    Ok(args)
}

#[cfg(test)]
fn export(
    ffmpeg: PathBuf,
    input: PathBuf,
    output: PathBuf,
    start: f64,
    end: f64,
    precise: bool,
) -> Result<Job, String> {
    export_with_removal(ffmpeg, input, output, start, end, precise, None)
}

fn export_with_removal(
    ffmpeg: PathBuf,
    input: PathBuf,
    output: PathBuf,
    start: f64,
    end: f64,
    precise: bool,
    removal: Option<(f64, bool)>,
) -> Result<Job, String> {
    if output.exists() || input == output {
        return Err("目标文件已存在，请选择新文件名；不会覆盖原文件。".into());
    }
    let parent = output.parent().ok_or("无效的导出目录")?;
    // 独占临时目录，取消和失败时只清理由本任务创建的文件。
    let folder = parent.join(format!(
        ".compact-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir(&folder).map_err(|e| e.to_string())?;
    let temp = folder.join("clip.mp4");
    let args = if let Some((duration, audio)) = removal {
        if !valid_range(start, end, duration) || start <= 0.0 || end >= duration {
            let _ = std::fs::remove_dir_all(&folder);
            return Err("中间广告范围必须在视频内部，并保留前后两段内容".into());
        }
        if precise {
            remove_args(&input, start, end, audio, &temp)
        } else {
            match fast_remove_args(&input, start, end, &folder.join("segments.ffconcat"), &temp) {
                Ok(args) => args,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&folder);
                    return Err(error);
                }
            }
        }
    } else {
        clip_args(&input, start, end, precise, &temp)
    };
    let output_duration = removal
        .map(|(duration, _)| duration - (end - start))
        .unwrap_or(end - start);
    let child = Command::new(&ffmpeg)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&folder);
            return Err(format!("无法启动 FFmpeg：{error}"));
        }
    };
    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancellation = cancel.clone();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let worker = std::thread::spawn(move || {
        let progress_sender = sender.clone();
        let progress = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(value) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|s| s.parse::<f64>().ok())
                {
                    let _ = progress_sender.send(Event::Progress(
                        (value / 1_000_000.0 / output_duration).clamp(0.0, 0.99) as f32,
                    ));
                }
            }
        });
        let errors = std::thread::spawn(move || {
            let mut tail = Vec::new();
            let mut reader = BufReader::new(stderr);
            let mut block = [0; 4096];
            while let Ok(n) = reader.read(&mut block) {
                if n == 0 {
                    break;
                }
                tail.extend_from_slice(&block[..n]);
                if tail.len() > 16384 {
                    tail.drain(..tail.len() - 16384);
                }
            }
            String::from_utf8_lossy(&tail).into_owned()
        });
        let result = loop {
            if cancellation.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                break Err("已取消导出".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(format!("FFmpeg 导出失败：{status}"))
                    };
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error.to_string());
                }
            }
        };
        let _ = progress.join();
        let stderr = errors.join().unwrap_or_default();
        let result = result
            .and_then(|_| {
                if cancellation.load(Ordering::Relaxed) {
                    return Err("已取消导出".into());
                }
                // hard_link 原子拒绝覆盖已存在目标，临时目录与输出处于同一文件系统。
                std::fs::hard_link(&temp, &output)
                    .map_err(|e| format!("保存失败（目标可能已存在）：{e}"))?;
                Ok(output)
            })
            .map_err(|error| {
                if stderr.is_empty() {
                    error
                } else {
                    format!("{error}\n{stderr}")
                }
            });
        let _ = std::fs::remove_dir_all(folder);
        let _ = sender.send(Event::Done(result));
    });
    Ok(Job {
        cancel,
        receiver,
        worker: Some(worker),
    })
}

impl Player {
    fn capture_filename(&self, screenshot: bool) -> String {
        let stem = self
            .selected
            .and_then(|index| self.playlist.get(index))
            .and_then(|path| path.file_stem())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "视频".into());
        let safe_time = |seconds| timecode(seconds).replace(':', "-");
        if screenshot {
            format!("{stem}_截图_{}.png", safe_time(self.number("time-pos")))
        } else if self.capture.remove_ad {
            format!("{stem}_去广告.mp4")
        } else {
            format!(
                "{stem}_截取_{}_{}.mp4",
                safe_time(self.capture.start),
                safe_time(self.capture.end)
            )
        }
    }

    fn save_capture(&mut self, path: &Path, display: bool) -> Result<(), String> {
        let frame = self
            .capture
            .frame
            .as_ref()
            .ok_or("尚无可用视频画面，请等待画面加载后重试")?;
        let mut pixels = Vec::with_capacity(frame.pixels.len() * 4);
        for pixel in &frame.pixels {
            let [r, g, b, _] = pixel.to_array();
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
        let image = image::RgbaImage::from_raw(frame.size[0] as u32, frame.size[1] as u32, pixels)
            .ok_or("视频缓冲尺寸无效")?;
        let image = match if display { self.rotation } else { 0 } {
            90 => image::imageops::rotate90(&image),
            180 => image::imageops::rotate180(&image),
            270 => image::imageops::rotate270(&image),
            _ => image,
        };
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let image = image::open(path).map_err(|e| format!("未能读取截图结果：{e}"))?;
        if image.width() == 0 || image.height() == 0 {
            return Err("截图尺寸无效".into());
        }
        Ok(())
    }

    pub fn capture_window(&mut self, ctx: &egui::Context) {
        let mut done = None;
        if let Some(job) = &self.capture.job {
            while let Ok(event) = job.receiver.try_recv() {
                match event {
                    Event::Progress(value) => self.capture.progress = value,
                    Event::Done(result) => done = Some(result),
                }
            }
        }
        if let Some(result) = done {
            self.capture.job = None;
            self.capture.message = match result {
                Ok(path) => {
                    self.capture.progress = 1.0;
                    format!("导出成功：{}", path.display())
                }
                Err(error) => error,
            };
        }
        let mut open = self.capture.open;
        let mut image_open = self.capture.image_open;
        egui::Window::new("图片截取")
            .open(&mut image_open)
            .default_width(480.0)
            .show(ctx, |ui| {
                let duration = self.number("duration");
                let has_video = self.playing && self.number("video-params/w") > 0.0;
                ui.label(format!(
                    "当前位置：{} / {}",
                    timecode(self.number("time-pos")),
                    timecode(duration)
                ));
                ui.add_enabled_ui(has_video, |ui| {
                    ui.horizontal(|ui| {
                        for (label, command) in
                            [("前一帧", "frame-back-step"), ("后一帧", "frame-step")]
                        {
                            if ui.button(label).clicked() {
                                self.rewinding = false;
                                self.speed = 1.0;
                                self.command(&["set", "speed", "1"]);
                                self.command(&["set", "pause", "yes"]);
                                self.command(&[command]);
                            }
                        }
                        ui.checkbox(&mut self.capture.display, "应用当前旋转方向");
                    });
                    if ui.button("保存当前帧（PNG）…").clicked() {
                        self.command(&["set", "pause", "yes"]);
                        // 在进入系统保存对话框前冻结实际已经显示的帧。
                        // OpenGL 模式下画面只在显存中，仅此时读回一次。
                        if let (Some(video), true) = (&self.gl_video, self.gl_ready) {
                            self.capture.frame = Some(std::sync::Arc::new(video.read()));
                        }
                        let frozen = self.capture.frame.clone();
                        let rotation = self.rotation;
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(self.capture_filename(true))
                            .add_filter("PNG 图片", &["png"])
                            .save_file()
                        {
                            if path.exists() {
                                self.capture.image_message = "文件已存在，请使用新文件名".into();
                            } else {
                                self.capture.frame = frozen;
                                self.rotation = rotation;
                                self.capture.image_message =
                                    match self.save_capture(&path, self.capture.display) {
                                        Ok(()) => format!("截图已保存：{}", path.display()),
                                        Err(error) => format!("截图失败：{error}"),
                                    };
                            }
                        }
                    }
                });
                ui.label(&self.capture.image_message);
            });
        self.capture.image_open = image_open;
        egui::Window::new("视频截取")
            .open(&mut open)
            .default_width(540.0)
            .show(ctx, |ui| {
                let duration = self.number("duration");
                ui.label(format!(
                    "当前位置：{} / {}",
                    timecode(self.number("time-pos")),
                    timecode(duration)
                ));
                ui.add_enabled_ui(self.capture.job.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.capture.remove_ad, false, "保留选中片段");
                        ui.selectable_value(&mut self.capture.remove_ad, true, "删除中间广告");
                    });
                });
                ui.label(if self.capture.remove_ad {
                    "标记广告开始和结束，导出时保留前后内容并拼接。"
                } else {
                    "标记需要保留的片段开始和结束。"
                });
                ui.label("先点击开始或结束输入框，再拖动播放器进度条；选中项实时更新。");
                ui.add_enabled_ui(self.playing && self.capture.job.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        for end in [false, true] {
                            let selected = self.capture.selected_boundary == Some(end);
                            ui.label(match (self.capture.remove_ad, end) {
                                (true, false) => "广告开始",
                                (true, true) => "广告结束",
                                (false, false) => "开始",
                                (false, true) => "结束",
                            });
                            let text = if end {
                                &mut self.capture.end_text
                            } else {
                                &mut self.capture.start_text
                            };
                            let response = ui
                                .scope(|ui| {
                                    if selected {
                                        ui.visuals_mut().extreme_bg_color =
                                            egui::Color32::from_rgb(25, 65, 80);
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(text)
                                            .desired_width(125.0)
                                            .font(egui::TextStyle::Monospace),
                                    )
                                })
                                .inner;
                            if response.clicked() || response.gained_focus() {
                                self.capture.selected_boundary = Some(end);
                            }
                            if response.changed() {
                                let text = if end {
                                    &self.capture.end_text
                                } else {
                                    &self.capture.start_text
                                };
                                if let Some(value) = parse_timecode(text) {
                                    if end {
                                        self.capture.end = value;
                                    } else {
                                        self.capture.start = value;
                                    }
                                }
                            }
                        }
                    });
                    ui.label(match self.capture.selected_boundary {
                        Some(false) => "正在设置开始时间：拖动下方播放进度条",
                        Some(true) => "正在设置结束时间：拖动下方播放进度条",
                        None => "点击上方输入框选择要调整的时间，也可直接输入时:分:秒.毫秒",
                    });
                    ui.horizontal(|ui| {
                        for (label, position) in [
                            (
                                if self.capture.remove_ad {
                                    "预览广告前2秒"
                                } else {
                                    "预览开始"
                                },
                                if self.capture.remove_ad {
                                    (self.capture.start - 2.0).max(0.0)
                                } else {
                                    self.capture.start
                                },
                            ),
                            (
                                if self.capture.remove_ad {
                                    "预览广告结束后"
                                } else {
                                    "预览结束前2秒"
                                },
                                if self.capture.remove_ad {
                                    self.capture.end
                                } else {
                                    (self.capture.end - 2.0).max(self.capture.start)
                                },
                            ),
                        ] {
                            if ui.button(label).clicked() {
                                self.command(&["seek", &position.to_string(), "absolute+exact"]);
                                self.command(&["set", "pause", "no"]);
                            }
                        }
                    });
                });
                ui.label(format!(
                    "保留时长：{}",
                    timecode(if self.capture.remove_ad {
                        (duration - (self.capture.end - self.capture.start)).max(0.0)
                    } else {
                        (self.capture.end - self.capture.start).max(0.0)
                    })
                ));
                ui.add_enabled_ui(self.capture.job.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.capture.precise, false, "快速复制（默认）");
                        ui.selectable_value(&mut self.capture.precise, true, "精确重编码");
                    });
                });
                if self.capture.remove_ad && !self.capture.precise {
                    ui.label("快速去广告不重新编码；关键帧附近可能残留广告或短暂画面异常，实际时长可能略有偏差。");
                }
                ui.label(
                    "快速模式切点受关键帧限制。导出保留首个视频和音轨，不包含字幕及界面旋转。",
                );
                if !self.capture.ffmpeg.is_file() {
                    ui.colored_label(egui::Color32::YELLOW, "导出需要 FFmpeg，请选择可执行文件。");
                }
                if ui.button("选择 FFmpeg…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("选择 ffmpeg 可执行文件")
                        .pick_file()
                    {
                        self.capture.ffmpeg = path;
                    }
                }
                let range_ok = valid_range(self.capture.start, self.capture.end, duration)
                    && parse_timecode(&self.capture.start_text).is_some()
                    && parse_timecode(&self.capture.end_text).is_some()
                    && (!self.capture.remove_ad
                        || (self.capture.start > 0.0
                            && self.capture.end < duration
                            && self.number("video-params/w") > 0.0));
                if !range_ok {
                    ui.label(if self.capture.remove_ad {
                        "请选择视频内部的广告范围：0 < 广告开始 < 广告结束 < 视频时长。"
                    } else {
                        "请设置有效范围：0 ≤ 开始 < 结束 ≤ 视频时长。"
                    });
                }
                if ui
                    .add_enabled(
                        range_ok && self.capture.job.is_none() && self.capture.ffmpeg.is_file(),
                        egui::Button::new(if self.capture.remove_ad {
                            "去掉广告并导出（MP4）…"
                        } else {
                            "导出保留片段（MP4）…"
                        }),
                    )
                    .clicked()
                {
                    if let Some(input) = self.selected.and_then(|i| self.playlist.get(i)).cloned() {
                        if let Some(output) = rfd::FileDialog::new()
                            .set_file_name(self.capture_filename(false))
                            .add_filter("MP4 视频", &["mp4"])
                            .save_file()
                        {
                            let removal = self
                                .capture
                                .remove_ad
                                .then(|| (duration, self.property("audio-codec").is_some()));
                            match export_with_removal(
                                self.capture.ffmpeg.clone(),
                                input,
                                output,
                                self.capture.start,
                                self.capture.end,
                                self.capture.precise,
                                removal,
                            ) {
                                Ok(job) => {
                                    self.capture.job = Some(job);
                                    self.capture.progress = 0.0;
                                    self.capture.message = "正在后台导出…".into();
                                }
                                Err(error) => self.capture.message = error,
                            }
                        }
                    }
                }
                if let Some(job) = &self.capture.job {
                    ui.add(egui::ProgressBar::new(self.capture.progress).show_percentage());
                    if ui.button("取消导出").clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                }
                ui.label(&self.capture.message);
            });
        self.capture.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires FFmpeg and ffprobe"]
    fn quick_copy_has_no_leading_video_gap_with_b_frames() {
        let ffmpeg = Capture::default().ffmpeg;
        let folder =
            std::env::temp_dir().join(format!("compact-start-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let input = folder.join("source.mp4");
        let generated = Command::new(&ffmpeg)
            .args([
                "-v",
                "error",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440",
                "-t",
                "3",
                "-c:v",
                "libx264",
                "-bf",
                "2",
                "-c:a",
                "aac",
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert!(generated.status.success());
        let output = folder.join("clip.mp4");
        let result = Command::new(&ffmpeg)
            .args(clip_args(&input, 0.5, 2.5, false, &output))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let probe = Command::new(ffmpeg.with_file_name("ffprobe"))
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=start_time",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(&output)
            .output()
            .unwrap();
        let start: f64 = String::from_utf8_lossy(&probe.stdout)
            .trim()
            .parse()
            .unwrap();
        assert!(start.abs() < 0.002, "视频起点不是零：{start}");
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    #[ignore = "requires local FFmpeg and ffprobe"]
    fn fast_ad_removal_copies_with_and_without_audio() {
        let ffmpeg = Capture::default().ffmpeg;
        let folder = std::env::temp_dir().join(format!("compact-fast-ad-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for audio in [false, true] {
            let input = folder.join(format!("中文 ' input-{audio}.mp4"));
            let mut command = Command::new(&ffmpeg);
            command.args([
                "-v",
                "error",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=25",
            ]);
            if audio {
                command.args(["-f", "lavfi", "-i", "sine=frequency=440"]);
            }
            let generated = command
                .args([
                    "-t",
                    "3",
                    "-c:v",
                    "libx264",
                    "-g",
                    "25",
                    "-keyint_min",
                    "25",
                    "-sc_threshold",
                    "0",
                    "-bf",
                    "0",
                    "-c:a",
                    "aac",
                ])
                .arg(&input)
                .output()
                .unwrap();
            assert!(
                generated.status.success(),
                "{}",
                String::from_utf8_lossy(&generated.stderr)
            );
            let output = folder.join(format!("去广告-{audio}.mp4"));
            let job = export_with_removal(
                ffmpeg.clone(),
                input,
                output.clone(),
                1.0,
                2.0,
                false,
                Some((3.0, audio)),
            )
            .unwrap();
            loop {
                if let Event::Done(result) =
                    job.receiver.recv_timeout(Duration::from_secs(30)).unwrap()
                {
                    assert!(result.is_ok(), "{result:?}");
                    break;
                }
            }
            let probe = Command::new(ffmpeg.with_file_name("ffprobe"))
                .args([
                    "-v",
                    "error",
                    "-show_format",
                    "-show_streams",
                    "-of",
                    "json",
                ])
                .arg(&output)
                .output()
                .unwrap();
            let data: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let duration: f64 = data["format"]["duration"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!((duration - 2.0).abs() < 0.2, "{duration}");
            let streams = data["streams"].as_array().unwrap();
            assert_eq!(streams.iter().any(|s| s["codec_type"] == "audio"), audio);
            assert!(streams.iter().any(|s| s["codec_name"] == "h264"));
            let decoded = Command::new(&ffmpeg)
                .args(["-v", "error", "-i"])
                .arg(&output)
                .args(["-f", "null", "-"])
                .output()
                .unwrap();
            assert!(
                decoded.status.success() && decoded.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&decoded.stderr)
            );
        }
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    #[ignore = "requires FFmpeg and COMPACT_TEST_VIDEO"]
    fn removes_middle_and_keeps_surrounding_audio_video() {
        let ffmpeg = Capture::default().ffmpeg;
        let input = PathBuf::from(std::env::var_os("COMPACT_TEST_VIDEO").unwrap());
        let probe = Command::new(ffmpeg.with_file_name("ffprobe"))
            .args([
                "-v",
                "error",
                "-show_format",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&input)
            .output()
            .unwrap();
        let source: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let duration: f64 = source["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let audio = source["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["codec_type"] == "audio");
        let folder =
            std::env::temp_dir().join(format!("compact-remove-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let output = folder.join("去广告.mp4");
        let job = export_with_removal(
            ffmpeg.clone(),
            input,
            output.clone(),
            0.5,
            1.5,
            true,
            Some((duration, audio)),
        )
        .unwrap();
        loop {
            if let Event::Done(result) = job.receiver.recv_timeout(Duration::from_secs(30)).unwrap()
            {
                assert!(result.is_ok(), "{result:?}");
                break;
            }
        }
        let probe = Command::new(ffmpeg.with_file_name("ffprobe"))
            .args([
                "-v",
                "error",
                "-show_format",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(&output)
            .output()
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let actual: f64 = result["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (actual - (duration - 1.0)).abs() < 0.2,
            "时长异常：{actual}"
        );
        let tracks = result["streams"].as_array().unwrap();
        assert!(tracks.iter().any(|s| s["codec_type"] == "video"));
        assert_eq!(tracks.iter().any(|s| s["codec_type"] == "audio"), audio);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn slider_only_changes_selected_boundary_while_panel_open() {
        let mut capture = Capture::default();
        capture.open = true;
        capture.set_from_slider(10.0);
        assert_eq!(capture.start, 0.0);
        capture.selected_boundary = Some(false);
        capture.set_from_slider(5130.5);
        assert_eq!(capture.start_text, "01:25:30.500");
        assert_eq!(capture.end, 0.0);
        capture.selected_boundary = Some(true);
        capture.set_from_slider(6000.0);
        assert_eq!(capture.end_text, "01:40:00.000");
        assert_eq!(capture.start, 5130.5);
        capture.open = false;
        capture.set_from_slider(1.0);
        assert_eq!(capture.end, 6000.0);
        capture.reset_range();
        assert_eq!(capture.selected_boundary, None);
        assert_eq!(capture.start_text, "00:00:00.000");
    }
    #[test]
    #[ignore = "requires libmpv and colored COMPACT_TEST_VIDEO"]
    fn screenshots_have_visible_pixels_when_paused() {
        let mut player = Player::new();
        player.playlist.push(PathBuf::from(
            std::env::var_os("COMPACT_TEST_VIDEO").unwrap(),
        ));
        player.play(0);
        player.size = [320, 180];
        player.pixels = vec![0; 320 * 180 * 4];
        let start = std::time::Instant::now();
        loop {
            player
                .backend
                .as_ref()
                .unwrap()
                .render(320, 180, &mut player.pixels, true)
                .unwrap();
            if player
                .pixels
                .chunks_exact(4)
                .any(|p| p[0] > 30 || p[1] > 30 || p[2] > 30)
            {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        player.command(&["set", "pause", "yes"]);
        player.capture.frame = Some(std::sync::Arc::new(
            egui::ColorImage::from_rgba_unmultiplied(player.size, &player.pixels),
        ));
        let ctx = egui::Context::default();
        player.texture = Some(ctx.load_texture(
            "test",
            egui::ColorImage::example(),
            egui::TextureOptions::LINEAR,
        ));
        for display in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "compact-screenshot-{}-{display}.png",
                std::process::id()
            ));
            // 复现无新帧时空缓冲的情况，截图需重新填充。
            if display {
                player.pixels.fill(0);
                player.rotation = 90;
            }
            player.texture = Some(ctx.load_texture(
                "test",
                egui::ColorImage::example(),
                egui::TextureOptions::LINEAR,
            ));
            player.save_capture(&path, display).unwrap();
            let image = image::open(&path).unwrap().to_rgba8();
            assert!(
                image
                    .pixels()
                    .any(|p| p[3] > 0 && (p[0] > 30 || p[1] > 30 || p[2] > 30)),
                "截图为空：{path:?}"
            );
            if display {
                assert_eq!(image.dimensions(), (180, 320));
                assert!(image.pixels().all(|p| p[3] == 255));
            }
            if display {
                if let Some(qa_path) = std::env::var_os("COMPACT_CAPTURE_QA") {
                    std::fs::copy(&path, qa_path).unwrap();
                }
            }
            std::fs::remove_file(path).unwrap();
        }
        player.command(&["stop"]);
    }
    #[test]
    #[ignore = "requires FFmpeg and COMPACT_TEST_VIDEO"]
    fn exports_and_cancels_without_overwriting() {
        let ffmpeg = Capture::default().ffmpeg;
        let input = PathBuf::from(std::env::var_os("COMPACT_TEST_VIDEO").unwrap());
        let folder =
            std::env::temp_dir().join(format!("compact-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for precise in [false, true] {
            let output = folder.join(format!("片段 {precise}.mp4"));
            let job = export(
                ffmpeg.clone(),
                input.clone(),
                output.clone(),
                0.5,
                1.5,
                precise,
            )
            .unwrap();
            loop {
                if let Event::Done(result) =
                    job.receiver.recv_timeout(Duration::from_secs(30)).unwrap()
                {
                    assert!(result.is_ok(), "{result:?}");
                    break;
                }
            }
            assert!(std::fs::metadata(&output).unwrap().len() > 100);
            if precise {
                let probe = Command::new(ffmpeg.with_file_name("ffprobe"))
                    .args([
                        "-v",
                        "error",
                        "-show_entries",
                        "format=duration",
                        "-of",
                        "default=nw=1:nk=1",
                    ])
                    .arg(&output)
                    .output()
                    .unwrap();
                let duration: f64 = String::from_utf8_lossy(&probe.stdout)
                    .trim()
                    .parse()
                    .unwrap();
                assert!(
                    (duration - 1.0).abs() < 0.15,
                    "精确导出时长异常：{duration}"
                );
            }
            assert!(export(ffmpeg.clone(), input.clone(), output, 0.5, 1.5, precise).is_err());
        }
        let output = folder.join("cancel.mp4");
        let job = export(ffmpeg, input, output.clone(), 0.0, 600.0, true).unwrap();
        job.cancel.store(true, Ordering::Relaxed);
        loop {
            if let Event::Done(result) = job.receiver.recv_timeout(Duration::from_secs(30)).unwrap()
            {
                assert!(result.is_err());
                break;
            }
        }
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn ranges_and_safe_arguments() {
        assert_eq!(timecode(5130.5), "01:25:30.500");
        assert_eq!(parse_timecode("01:25:30.500"), Some(5130.5));
        assert_eq!(timecode(59.9996), "00:01:00.000");
        assert_eq!(parse_timecode("00:60:00"), None);
        assert_eq!(parse_timecode("00:00:60"), None);
        assert_eq!(parse_timecode("-1"), None);
        assert!(valid_range(1.0, 2.0, 3.0));
        for (a, b) in [(2.0, 1.0), (-1.0, 2.0), (0.0, 4.0), (f64::NAN, 2.0)] {
            assert!(!valid_range(a, b, 3.0));
        }
        let args = clip_args(
            Path::new("/tmp/中文 ' file.mp4"),
            1.0,
            2.0,
            true,
            Path::new("/tmp/out.mp4"),
        );
        assert!(args.contains(&"/tmp/中文 ' file.mp4".into()));
        assert!(args.contains(&"libx264".into()));
    }
}
