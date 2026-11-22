# 0.1.5 性能与兼容性评估

## 当前实现

- 软件渲染调用 `mpv_render_context_render` 输出 RGBA，再经 `ColorImage` 上传 egui 纹理；缓冲上限 1920×1080。输出尺寸上限不改变源文件的解码分辨率。
- 自动硬件解码使用 `hwdec=auto-copy`，允许复制回系统内存以兼容当前软件渲染。诊断窗口可以切换为 `no`，实际结果查看 `hwdec-current`。
- 暂停、未播放时将定时刷新从约 60 次/秒降到 10 次/秒；轨道和统计每 500ms 更新。没有去掉视频渲染所需的定时检查。
- 硬解未必降低总 CPU：色彩转换、缩放、RGBA 拷贝和纹理上传仍存在。不能仅凭配置开关宣称性能改善。

## GPU 直绘评估

当前 `mpv.rs` 的 render context 使用 `sw`；不能在运行中将此上下文当作 OpenGL 上下文使用。
后续独立实现 `opengl` render context，利用 eframe 的 Glow 上下文与绘制回调，在正确线程和当前上下文中绘制到 FBO，再以纹理显示。
需要处理函数地址加载、帧完成通知、上下文销毁顺序、GL 状态恢复、HiDPI、旋转与窗口缩放，并保留软件回退。
本轮未启用 GPU 直绘，不以硬件解码代替 GPU 渲染验收。应在 ARM 和 Intel 真机完成长时间测试后再决定默认路径。

## 可重复基准

开发机短测：Apple Silicon，6 秒合成 H.264 1080p30 + AAC，分别运行 3 秒。
auto-copy 实际为 videotoolbox-copy，软件模式实际为 no；两者解码丢帧均为 0，输出丢帧均为 1。
硬解渲染调用累计约 1169ms，软件约 1145ms。样本很短、帧数不同，且不含 UI 上传，不能据此判断总 CPU 改善或长期稳定性。

使用同一文件、相同分辨率与窗口大小，电源接通，关闭后台重任务。至少分别测试 H.264 1080p、H.264 4K、HEVC 4K、多音轨和外挂字幕，每项播放十分钟；软硬解码交替测试并留出冷却时间。

```sh
cargo build --offline --release --bin check_performance
/usr/bin/time -l target/release/check_performance /path/libmpv.dylib /path/video.mp4 software 60
/usr/bin/time -l target/release/check_performance /path/libmpv.dylib /path/video.mp4 hardware 60
```

程序报告实际硬解、帧数、渲染调用累计耗时、丢帧、采样到的最大绝对 A/V 差值。此基准不含 egui 上传和窗口合成，不能代替应用总 CPU 测量；采样差值不能独立证明感知音画同步。

Intel 机器：MacBookPro11,4 / i7-4770HQ / 16GB / macOS 12.7.1。在活动监视器记录应用 CPU、内存、能耗；如已有温度工具，同时记录温度、风扇及环境温度，避免使用无权限或不可用的温度读数替代测量。
手动检查对白口型、快进后同步、字幕延迟正负方向、全屏、旋转、暂停、连续播放、睡眠唤醒。
本开发环境的 Rosetta 测试只能验证 Intel 二进制功能，不能代表旧 Intel 硬件的速度、温度或解码能力。

## 错误与日志

`~/Library/Logs/CompactPlayer/player-<pid>.log` 保存启动版本/架构、播放库、打开的路径、解码器、命令失败和 mpv 警告。
后台有界队列写盘，每份日志超过 5MiB 轮转，启动时清理旧 player 日志，仅保留最近五份历史文件。日志包含本地文件路径，分享前请检查。
Rust panic 单独同步写入 `panic-<pid>.log`。原生 SIGSEGV/abort 不保证触发 Rust hook，检查 macOS 的 `~/Library/Logs/DiagnosticReports/`。
损坏或不存在的文件通过 libmpv 非阻塞事件显示错误提示；不会预扫描每个文件。
