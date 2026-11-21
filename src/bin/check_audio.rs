#[path = "../mpv.rs"]
mod mpv;

fn main() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 {
        return Err("用法：check_audio libmpv.dylib 媒体文件".into());
    }
    let backend = mpv::Mpv::new(std::path::Path::new(&arguments[0]))?;
    backend.command(&["set", "volume", "25"])?;
    backend.command(&["loadfile", &arguments[1]])?;
    let mut pixels = vec![0; 320 * 180 * 4];
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(4) {
        backend.render(320, 180, &mut pixels, false)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for property in [
        "track-list",
        "aid",
        "audio-codec-name",
        "current-ao",
        "audio-device",
        "audio-params",
        "audio-out-params",
        "mute",
        "volume",
        "time-pos",
    ] {
        println!(
            "{property}: {}",
            backend
                .property(property)
                .unwrap_or_else(|| "不可用".into())
        );
    }
    let output = backend.property("current-ao");
    backend.command(&["stop"])?;
    if output.as_deref().map_or(true, |value| value == "null") {
        return Err("没有有效音频输出".into());
    }
    println!("音频输出初始化成功；实际听感需在输出设备确认。");
    Ok(())
}
