# Intel 0.1.6 硬解测试版

基于 Intel 0.1.6，默认请求 `hwdec=videotoolbox-copy`，不支持的编码由 mpv 回退软件解码。probe 仍关闭硬解。
底部“解码”按钮显示实际状态，可查看编码、A/V 差值、丢帧并在播放中切换软硬解码。更换播放库沿用当前开关，重启恢复默认硬解。
暂停、结束和空闲时定时刷新降到 10Hz。播放时（`codex/opengl-zero-copy` 起）由 mpv 新帧回调按视频帧率重绘。

`codex/opengl-zero-copy` 分支起改为 OpenGL 零拷贝渲染（`hwdec=videotoolbox`），软件 RGBA 渲染保留为回退路径。Intel 机器上的稳定性和 CPU 占用尚未实测。

## 验证结果

- Intel 二进制在 Rosetta 下 15 项测试通过，包括 H.264 软硬解码往返切换、截图、导出及连播。
- 开发机 Intel 运行库下，H.264 1080p30 使用 videotoolbox-copy，MPEG-4 样片回退 no，均能渲染。
- 5 秒短基准硬解 user CPU 时间约 3.18 秒，软件约 4.08 秒，解码丢帧均为 0。初始化耗时与绘制帧数不同，且不含 egui 上传；不能代表旧 Intel 机器的改善幅度。
- 旧机温度、风扇及流畅度尚需实测，Rosetta 不能代替。

## 旧机器对照步骤

1. 插电、关闭其他重负载应用，使用同一 H.264 1080p 视频、窗口尺寸和正常倍速。
2. 点击底部“解码”，确认实际为 videotoolbox-copy，记录活动监视器 CPU、能耗及内存，观察口型同步。
3. 关闭硬解，等待十秒稳定后记录一分钟；再次开启硬解记录一分钟，重复三次。
4. 检查进度拖动、全屏、旋转、截图和自动连播，以及不支持的编码能否回退软件。

独立基准不含 UI：

```sh
cargo build --release --offline --bin check_performance
/usr/bin/time -p target/release/check_performance /path/libmpv.dylib /path/video.mp4 hardware 60
/usr/bin/time -p target/release/check_performance /path/libmpv.dylib /path/video.mp4 software 60
```

测试包名增加 hwdec-test；原正式版 DMG 保留。FFmpeg 仍为外部依赖。

## Retina 与视频有效区域修正

软件渲染尺寸原来按逻辑点计算，Retina 屏幕会把低分辨率纹理放大。现在按有效视频区域及物理像素计算，黑边由中央面板显示，渲染尺寸不超过源像素，像素预算上限 4K。
直接输出不透明 RGB，避免无意义的逐像素透明度预乘。低分辨率源不会因该变更产生更多细节。
解码设置可关闭 Retina 清晰显示以减小 CPU 压力；清晰模式增加像素数，未承诺总 CPU 下降。这仍不是 GPU 直绘或零拷贝，和 QuickTime 的性能差距尚未消除。
新测试包名为 hwdec-retina-test，保留上一份 hwdec-test 以供对照。

## 清晰度复核（2026-11-23）

此前把逻辑点渲染直接认定为用户清晰度问题的根因，证据不足。此次用本地“截取片段.mp4”（1080×1920 H.264）在暂停、精确定位 2 秒后，对照旧路径 900×600（含黑边）和新路径 675×1200（有效区域），旧图裁去黑边并用双线性放大到相同尺寸。目视未复现新路径静态画面变糊；新图细节略多，但不能代表用户 Intel 屏幕的实际呈现。

同一素材、同一定位，软件解码与请求硬解的输出 PSNR 约 68.87dB，差异很小；本次未采集实际 hwdec-current，不能据此断言 Intel 硬解没有清晰度回归。对照图片和临时诊断源在 /tmp/compact-quality-ab 与 /tmp/compact-quality-hw。

该竖屏例子的新路径有效视频像素数约为旧路径的 4 倍（总缓冲区约为旧路径的 1.5 倍），可能增加 CPU 负担。尚无旧 Intel 实机的显示截图、帧时间和与 QuickTime 的相同画面对照，因此清晰度回归及其根因仍未确认。本轮未修改播放器显示行为，也未重新发布安装包。
