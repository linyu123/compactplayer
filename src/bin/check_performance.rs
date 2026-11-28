#[path = "../mpv.rs"]
mod mpv;

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 2 {
        return Err(
            "Usage: check_performance libmpv.dylib video [hardware|software] [seconds]".into(),
        );
    }
    let hardware = args.get(2).is_none_or(|s| s == "hardware");
    let seconds = args
        .get(3)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30)
        .clamp(1, 600);
    let backend = mpv::Mpv::with_hwdec(std::path::Path::new(&args[0]), hardware)?;
    backend.command(&["loadfile", &args[1]])?;
    let mut pixels = vec![0; 1920 * 1080 * 4];
    let start = std::time::Instant::now();
    let mut frames = 0;
    let mut render_ms = 0.0;
    let mut loaded = false;
    let mut maximum_avsync = 0.0_f64;
    while start.elapsed().as_secs() < seconds {
        let frame_start = std::time::Instant::now();
        if backend.render(1920, 1080, &mut pixels, false)? {
            frames += 1;
        }
        render_ms += frame_start.elapsed().as_secs_f64() * 1000.0;
        loaded |= backend.property("video-codec").is_some();
        if let Some(value) = backend
            .property("avsync")
            .and_then(|s| s.parse::<f64>().ok())
        {
            if value.is_finite() {
                maximum_avsync = maximum_avsync.max(value.abs());
            }
        }
        if loaded && backend.property("eof-reached").as_deref() == Some("yes") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    if !loaded || frames == 0 {
        return Err("未成功加载并渲染媒体".into());
    }
    println!(
        "{}",
        serde_json::json!({"arch":std::env::consts::ARCH,"requested_hwdec":hardware,"actual_hwdec":backend.property("hwdec-current"),"elapsed_seconds":start.elapsed().as_secs_f64(),"rendered_frames":frames,"render_call_ms_total":render_ms,"max_abs_avsync_seconds":maximum_avsync,"decoder_drops":backend.property("decoder-frame-drop-count"),"output_drops":backend.property("frame-drop-count"),"note":"软件渲染基准，不含 egui 纹理上传；CPU/温度需实机采集"})
    );
    Ok(())
}
