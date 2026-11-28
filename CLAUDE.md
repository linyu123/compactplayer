# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概况

Compact Player（应用名“轻巧播放器”）：macOS 上的 Rust 视频播放器原型，eframe/egui 界面 + 进程内 libmpv。界面文案、README、提交信息均使用中文。本项目为独立实现，不得引入 PotPlayer 的代码、图标或品牌素材。

## 常用命令

```bash
bash scripts/install-libmpv.sh [arm64|x86_64]   # 下载并校验 libmpv 到被忽略的 runtime/，运行前必须执行
cargo run                                       # 启动 GUI；可追加 -- /absolute/path/video.mkv
bash scripts/run-app.sh /path/video.mkv         # 生成开发用 .app 外壳启动（依赖项目内 runtime/）
bash scripts/package-app.sh [arm64|x86_64]      # Release 应用 + DMG，输出到 dist/

cargo test --bin compact-player                 # 单元测试（无需外部依赖）
cargo test --bin compact-player <test_name>     # 运行单个测试
```

很多测试标记为 `#[ignore]`，需要真实 libmpv、FFmpeg/ffprobe 或测试视频，用 `-- --ignored` 运行并设置环境变量：

- `COMPACT_TEST_VIDEO`：1–10 秒、含彩色画面的视频（播放、截图、导出测试）
- `COMPACT_H264_VIDEO`：H.264 视频（`decoder.rs` 软硬解切换测试）
- 例：`COMPACT_TEST_VIDEO=/path/test.mp4 cargo test --offline --bin compact-player exports_and_cancels_without_overwriting -- --ignored`

`src/bin/` 下是不带 UI 的诊断程序，第一个参数为 libmpv.dylib 路径：`check_libmpv`、`check_render`、`check_audio`、`check_performance`（`<dylib> <video> hardware|software <秒数>`，配合 `/usr/bin/time -p` 做软硬解 CPU 对照）。它们通过 `#[path = "../mpv.rs"]` 直接复用 `mpv.rs`，修改 `Mpv` 的公开接口时要同步检查这些二进制。

构建/打包脚本使用 `--offline`，新增依赖需先联网 fetch。

## 架构

- **`mpv.rs`**：用 `libloading` 动态加载 libmpv 的 C API（不链接），封装 `command`、`property`、`render`（软件 RGBA）和 `render_opengl`（渲染到 FBO）。`with_opengl` 创建 OpenGL 渲染上下文并使用 `hwdec=videotoolbox` 零拷贝；`with_hwdec` 为软件渲染，硬解为 `videotoolbox-copy`。切换硬解时用 `hardware_decoder()` 取当前模式对应的选项。`probe` 模式使用 `vo=null` 且关闭硬解，用于检测媒体。不支持的编码由 mpv 回退软件解码。
- **`main.rs`**：`Player` 结构体持有全部状态并实现 `eframe::App`。其他模块大多通过 `impl Player { ... }` 为它追加方法（`ui.rs` 的工具栏/控制栏/播放列表、`interaction.rs` 的快捷键与全屏控制栏自动隐藏、`decoder.rs` 的解码状态窗口、`capture.rs` 的截图/导出窗口），而不是独立组件。
- **渲染流程**：每帧 `update` 中 `video_layout::layout` 按有效视频区域和物理像素（Retina）计算渲染尺寸，不超过源像素，上限 4K；默认路径（`gl_video.rs`）：mpv 在 `update` 中直接渲染到 `GlVideo` 的 FBO 纹理，该纹理通过 `register_native_glow_texture` 交给 egui 显示；纹理格式需与 egui_glow 的 sRGB 判断一致，否则颜色发灰。截图时才用 `GlVideo::read` 读回。mpv 渲染上下文必须在 GL 上下文有效时释放（`on_exit`、切换内核时先释放旧内核）。回退路径：`backend.render` 填充 `pixels`，`opaque_frame` 转为不透明 RGB 后上传为 egui 纹理；`COMPACT_PLAYER_RENDER=software` 强制回退。旋转在纹理层完成，没有使用 mpv 的旋转（会触发 libmpv 裁剪断言崩溃）。播放时约 60Hz 重绘，暂停/空闲时降到 10Hz。`src/bin/` 诊断程序和 `#[ignore]` 测试只走软件渲染路径。
- **libmpv 路径解析**（`Player::new`）：`COMPACT_PLAYER_LIBMPV` 环境变量 → 应用包内 `Contents/Frameworks/libmpv.dylib` → 项目 `runtime/libmpv-libs_v0.7.3_macos-{arm64|amd64}-video-default/`。运行中可通过“播放内核”按钮切换。
- **`media.rs`**：按扩展名筛选媒体，扫描同目录视频（不递归），路径规范化去重，按文件名自然排序。扫描在后台线程运行，结果经 `mpsc` 通道（`Player::intake`）回到 UI 线程。
- **`capture.rs`**：截图直接保存当前显示的缓冲区（含字幕）。片段导出和删除中间广告通过子进程调用外部 FFmpeg（自动查找 Homebrew 路径或由用户选择），后台线程读取进度并支持取消，取消或失败时清理临时文件，不覆盖已有文件。FFmpeg 不打包进应用。
- **`macos_open.rs`**：通过 objc2 注册 Finder “打开方式”事件，`take()` 在每帧取出待打开的路径。
- **`playback.rs`**：顺序播放 / 单曲循环 / 列表循环模式，决定播放结束后的下一项。

## 分支与版本

不同功能线分在不同分支（如 `codex/intel-macos12`、`codex/intel-hwdec-0.1.6`、`release-0.1.x`），Intel 分支会从主干同步功能。Intel 包面向 macOS 12+（`MACOSX_DEPLOYMENT_TARGET=12.0`，需 `rustup target add x86_64-apple-darwin`），Apple Silicon 包面向 macOS 14+。在 Apple Silicon 上交叉编译的 Intel 包只能在 Rosetta 下验证，不能替代 Intel 真机测试。版本号同时写在 `Cargo.toml` 和 `scripts/package-app.sh` 的 DMG 文件名中。当前分支的硬解测试说明和验证记录见 `docs/intel-hwdec-0.1.6.md`。
