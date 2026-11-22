use crate::{mpv, timestamp, Player};
use eframe::egui::{self, Color32, RichText, Vec2};

const ACCENT: Color32 = Color32::from_rgb(76, 200, 234);
const MUTED: Color32 = Color32::from_rgb(139, 151, 169);
const PANEL: Color32 = Color32::from_rgb(23, 28, 36);

pub fn configure(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = Color32::from_rgb(28, 34, 44);
    style.visuals.extreme_bg_color = Color32::from_rgb(15, 19, 25);
    style.visuals.selection.bg_fill = Color32::from_rgb(30, 74, 91);
    style.visuals.selection.stroke.color = ACCENT;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(35, 42, 53);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(35, 42, 53);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(49, 62, 77);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(35, 100, 121);
    style.visuals.widgets.noninteractive.bg_stroke.color = Color32::from_rgb(43, 51, 63);
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    ctx.set_style(style);
}

impl Player {
    pub fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(56.0)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(
                        egui::RichText::new("轻巧播放器")
                            .strong()
                            .color(egui::Color32::LIGHT_BLUE),
                    );
                    if ui.button("打开").clicked() {
                        self.open();
                    }
                    if ui
                        .button(self.mode.label())
                        .on_hover_text("点击切换：顺序播放 → 单曲循环 → 列表循环")
                        .clicked()
                    {
                        self.mode.cycle();
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
                    let fullscreen =
                        ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
                    let playlist_label = if fullscreen {
                        "全屏隐藏列表"
                    } else if self.show_playlist {
                        "隐藏列表"
                    } else {
                        "显示列表"
                    };
                    if ui
                        .add_enabled(!fullscreen, egui::Button::new(playlist_label))
                        .on_disabled_hover_text("退出全屏后恢复播放列表原来的状态")
                        .clicked()
                    {
                        self.show_playlist = !self.show_playlist;
                    }
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
                    if ui
                        .button(if fullscreen { "退出全屏" } else { "全屏" })
                        .on_hover_text("双击画面或按 F 切换全屏，Esc 退出")
                        .clicked()
                    {
                        self.toggle_fullscreen(ctx);
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
    }

    pub fn controls(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("controls")
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().button_padding = Vec2::new(7.0, 5.0);
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                let mut position = self.number("time-pos");
                let duration = self.number("duration");
                ui.spacing_mut().slider_width = ui.available_width();
                if ui
                    .add_enabled(
                        self.playing && duration > 0.0,
                        egui::Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false),
                    )
                    .changed()
                {
                    self.command(&["seek", &position.to_string(), "absolute+exact"]);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.selected.is_some(), egui::Button::new("上一项"))
                        .clicked()
                    {
                        if let Some(index) = self.selected {
                            self.play(index.saturating_sub(1));
                        }
                    }
                    if ui
                        .add_enabled(self.playing, egui::Button::new("后退10秒"))
                        .clicked()
                    {
                        self.seek(-10.0);
                    }
                    if ui
                        .add_enabled(self.playing, egui::Button::new("快退"))
                        .on_hover_text("连续点击切换 2、5、10、30、60 倍")
                        .clicked()
                    {
                        self.shuttle(true);
                    }
                    let ended = self.property("eof-reached").as_deref() == Some("yes");
                    let paused = self.property("pause").as_deref() == Some("yes");
                    let label = if ended {
                        "重播"
                    } else if !self.playing || (paused && !self.rewinding) {
                        "播放"
                    } else {
                        "停止"
                    };
                    if ui
                        .add_enabled(
                            self.selected.is_some(),
                            egui::Button::new(
                                RichText::new(label)
                                    .strong()
                                    .color(Color32::from_rgb(12, 29, 36)),
                            )
                            .fill(ACCENT)
                            .min_size(Vec2::new(64.0, 32.0)),
                        )
                        .on_hover_text("播放中点击停止并保留当前位置，再次点击继续；播完后点击重播")
                        .clicked()
                    {
                        self.toggle_play();
                    }
                    if ui
                        .add_enabled(self.playing, egui::Button::new("快进"))
                        .on_hover_text("连续点击切换 2、5、10、30、60 倍")
                        .clicked()
                    {
                        self.shuttle(false);
                    }
                    if ui
                        .add_enabled(self.playing, egui::Button::new("前进10秒"))
                        .clicked()
                    {
                        self.seek(10.0);
                    }
                    if ui
                        .add_enabled(self.selected.is_some(), egui::Button::new("下一项"))
                        .clicked()
                    {
                        if let Some(index) = self.selected {
                            self.play(index + 1);
                        }
                    }
                    ui.label(
                        RichText::new(format!(
                            "{}{}×",
                            if self.rewinding { "倒退" } else { "" },
                            self.speed
                        ))
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                timestamp(position),
                                timestamp(duration)
                            ))
                            .monospace()
                            .size(12.0)
                            .color(MUTED),
                        );
                        ui.spacing_mut().slider_width = 65.0;
                        if ui
                            .add(egui::Slider::new(&mut self.volume, 0.0..=100.0).suffix("%"))
                            .on_hover_text("音量")
                            .changed()
                        {
                            self.command(&["set", "volume", &self.volume.to_string()]);
                        }
                        ui.label(RichText::new("音量").size(12.0).color(MUTED));
                        if self.intake.is_some() {
                            ui.spinner().on_hover_text("正在扫描同目录视频");
                        }
                    });
                });
                if self.status.contains("错误")
                    || self.status.contains("失败")
                    || self.status.contains("不可用")
                {
                    ui.label(
                        RichText::new(&self.status)
                            .small()
                            .color(Color32::LIGHT_RED),
                    );
                }
            });
    }

    pub fn playlist_panel(&mut self, ctx: &egui::Context) {
        // 全屏时不绘制侧栏，保留窗口模式下的列表显示偏好。
        if ctx.input(|input| input.viewport().fullscreen.unwrap_or(false)) {
            return;
        }
        if !self.show_playlist {
            egui::SidePanel::right("playlist_collapsed")
                .exact_width(32.0)
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(19, 24, 31))
                        .inner_margin(4.0),
                )
                .show(ctx, |ui| {
                    if ui
                        .add_sized([24.0, 32.0], egui::Button::new("‹").small())
                        .on_hover_text("展开播放列表")
                        .clicked()
                    {
                        self.show_playlist = true;
                    }
                });
            return;
        }
        egui::SidePanel::right("playlist")
            .default_width(252.0)
            .min_width(210.0)
            .max_width(420.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(19, 24, 31))
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("播放列表").strong().size(15.0));
                    ui.label(
                        RichText::new(format!("{} 项", self.playlist.len()))
                            .small()
                            .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("›").on_hover_text("收起播放列表").clicked() {
                            self.show_playlist = false;
                        }
                        if ui
                            .add_enabled(
                                !self.playlist.is_empty(),
                                egui::Button::new("清空").small(),
                            )
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
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                if self.playlist.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("列表还是空的").color(MUTED));
                    ui.label(
                        RichText::new("打开文件或将视频拖入窗口\n同目录视频会自动添加")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                let mut selected = None;
                let mut removed = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, path) in self.playlist.iter().enumerate() {
                        let active = self.selected == Some(index);
                        let label = format!(
                            "{}  {}",
                            if active {
                                "▶".into()
                            } else {
                                format!("{:02}", index + 1)
                            },
                            path.file_name().unwrap_or_default().to_string_lossy()
                        );
                        let response = ui
                            .add_sized(
                                [ui.available_width(), 38.0],
                                egui::Button::new(RichText::new(label).color(if active {
                                    ACCENT
                                } else {
                                    Color32::from_rgb(198, 206, 218)
                                }))
                                .selected(active)
                                .truncate(),
                            )
                            .on_hover_text(path.display().to_string());
                        if response.clicked() {
                            selected = Some(index);
                        }
                        response.context_menu(|ui| {
                            if ui.button("从播放列表删除").clicked() {
                                removed = Some(index);
                                ui.close_menu();
                            }
                        });
                    }
                });
                if let Some(index) = removed {
                    let current = self.selected == Some(index);
                    if !current || self.backend.is_none() || self.command(&["stop"]) {
                        self.playlist.remove(index);
                        if current {
                            self.selected = None;
                            self.playing = false;
                            self.rewinding = false;
                            self.texture = None;
                            self.status = "当前播放项已从列表删除".into();
                        } else if let Some(selected) = self.selected {
                            if index < selected {
                                self.selected = Some(selected - 1);
                            }
                        }
                    }
                } else if let Some(index) = selected {
                    self.play(index);
                }
            });
    }
}
