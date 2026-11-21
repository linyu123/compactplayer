# Compact Player

面向 macOS 的 Rust 播放器原型，采用接近 PotPlayer 的紧凑深色布局。

## libmpv 集成进度

已验证 media-kit/libmpv-darwin-build v0.7.3 的 Apple Silicon video-default
运行库可在 macOS 14.2 加载，报告 mpv 0.36.0。可执行
`bash scripts/install-libmpv.sh` 下载、校验 SHA-256、修复同目录动态库搜索路径并做本地临时签名。
运行库保存在被 Git 忽略的 runtime/，不会修改 Homebrew 或系统目录。
该构建是第三方发行，版本较旧，正式分发前需要更新并整理许可证及源码提供要求。

检查命令：`cargo run --bin check_libmpv -- runtime/libmpv-libs_v0.7.3_macos-arm64-video-default/libmpv.dylib`。
可追加媒体路径验证无画面、无音频输出的播放进度。
GUI 已切换为进程内 libmpv，通过软件渲染 API 将 RGBA 帧上传到 egui 纹理，视频显示在主窗口。

## 运行

需要 Rust，先执行 `bash scripts/install-libmpv.sh`，然后运行 `cargo run`。
无需安装 mpv 命令行程序或 Homebrew。可用 `cargo run -- /absolute/path/video.mkv` 直接打开视频。
macOS 桌面启动可用 `bash scripts/run-app.sh /absolute/path/video.mkv`，它生成开发用 app 外壳并启动。
该外壳仍依赖当前项目的 runtime/ 路径，不是可分发安装包。
默认使用项目 runtime/ 中的运行库，也可通过 `COMPACT_PLAYER_LIBMPV` 环境变量指定完整动态库路径。
顶部 Runtime 按钮可切换 libmpv.dylib；同目录需要包含配套依赖库，选择仅在本次运行有效。

## 当前功能

- 打开多个媒体文件、拖入文件、播放列表选择及上一项/下一项。
- 通过 libmpv C API 控制播放暂停、停止、进度跳转和音量。
- 显示播放进度和时长，加载外部字幕、切换音轨和视频全屏。
- 静音切换及音频输出状态提示；切换音轨只在实际音轨间轮换，不会切到关闭状态。
- 空格暂停，左右方向键跳转五秒；快捷键在控制窗口有焦点时生效。
- 关闭窗口时释放渲染上下文及 libmpv 实例。

当前采用软件解码和软件渲染，渲染缓冲上限为 1920×1080，会有 CPU 拷贝开销。
尚未实现 GPU 渲染、硬件解码、自动连播、断点续播或独立应用打包。
渲染及控制检查：`cargo run --bin check_render -- /path/libmpv.dylib /path/test-video.mp4`。
此检查需要含彩色画面且时长超过一秒的测试视频。

本项目为独立实现，不包含 PotPlayer 的代码、图标或品牌素材。
