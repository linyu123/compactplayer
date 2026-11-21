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

## 界面布局

56px 顶部工具栏，功能按钮全部展开，保持直接操作。
底部按钮合并为一排，快进、快退和前后跳转均可直接点击，时长位于最右侧。
播放键统一承担播放/停止/重播：停止保留当前位置，再点击继续播放；播放结束后从头重播。
移除独立停止和静音按钮，音量仍可通过滑块调整。
右侧列表支持当前项高亮、长文件名省略及完整路径悬停提示。
空白页显示应用图标；底部显示倍速和音量，异常时显示错误提示。
最小窗口调整为 960×540，默认 1120×700。

## 应用打包

主干独立应用打包：`bash scripts/package-app.sh`，输出 `dist/CompactPlayer-0.1.1-main-arm64.dmg`。
此包适用于 Apple Silicon、macOS 14+，包含播放运行库和图标，使用本地临时签名，尚未公证。

## 应用图标

图标采用深蓝圆角底、青蓝渐变播放键及进度弧线，源设计为 `scripts/generate-icon.swift`。
运行 `bash scripts/generate-icon.sh` 生成 `resources/AppIcon.png` 和完整尺寸的 `resources/AppIcon.icns`。
开发 app 的启动脚本会复制图标并通过 Info.plist 配置，重新打包时也需要复制到 Contents/Resources/。

## 基础能力迭代

基础播放改进、旋转功能和应用图标已纳入主干。
工具栏“旋转 90°”每次顺时针旋转，按 0° → 90° → 180° → 270° → 0° 循环。
旋转由界面纹理层完成，横竖方向切换时调整渲染尺寸，绕过当前 libmpv 软件旋转的裁剪断言崩溃。
只改变播放显示，不修改原视频文件；打开或重播视频时恢复默认方向，保留文件自身方向元数据。

- 播放结束后，点击播放或按空格会从头重新播放。
- 左右方向键跳转 5 秒，底部按钮前进/后退 10 秒；跳转不会超出媒体范围。
- 连续点击快进或快退，切换 2、5、10、30、60 倍，点击播放恢复正常速度。
  快进使用 libmpv 倍速；快退采用无声跳帧预览，不是逐帧反向解码，也不保证与 QuickTime 完全一致。
- 添加文件仅按常见音视频扩展名快速筛选，不再逐个打开文件预检。显式选择的媒体立即开始加载播放。
  图片、字幕等不支持的扩展名弹窗提示；伪装成视频的损坏文件由播放内核在实际加载时处理。
- 文件路径规范化后去重，重复选择已有项会播放该项，不重复入列。
- 打开视频时，后台扫描同目录（不递归）的常见视频扩展名并加入列表，不预检解码能力。
  显式选择项优先，其他视频按文件名自然排序（第2集排在第10集前）；不自动添加同目录音频。
- 扫描不影响当前播放，也不在完成后重新启动播放。可以继续添加文件；新一轮扫描替换旧结果。
  清空会丢弃尚未完成的扫描结果，避免列表重新填入。

验证：`cargo test --bin compact-player`。
真实内核回归测试需提供超过一秒、短于十秒的视频：
`COMPACT_TEST_VIDEO=/path/test.mp4 cargo test --bin compact-player detects_invalid_media_and_replays_after_eof -- --ignored`。
