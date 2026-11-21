#[path = "../mpv.rs"]
mod mpv;

fn main() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 {
        return Err("Usage: check_render libmpv.dylib video".into());
    }
    let backend = mpv::Mpv::new(std::path::Path::new(&arguments[0]))?;
    backend.command(&["set", "mute", "yes"])?;
    backend.command(&["loadfile", &arguments[1]])?;
    let mut pixels = vec![0; 320 * 180 * 4];
    let start = std::time::Instant::now();
    let mut rendered = false;
    while start.elapsed() < std::time::Duration::from_secs(10) {
        backend.render(320, 180, &mut pixels, false)?;
        if pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 20 || pixel[1] > 20 || pixel[2] > 20)
        {
            rendered = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !rendered {
        return Err("No colored video frame within 10 seconds".into());
    }
    for (width, height) in [(180, 320), (320, 180), (180, 320), (320, 180)] {
        for _ in 0..10 {
            backend.render(width, height, &mut pixels, true)?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !pixels
            .chunks_exact(4)
            .any(|pixel| pixel[0] > 20 || pixel[1] > 20 || pixel[2] > 20)
        {
            return Err("No colored frame after rotation viewport change".into());
        }
    }
    backend.command(&["set", "pause", "yes"])?;
    if backend.property("pause").as_deref() != Some("yes") {
        return Err("Pause failed".into());
    }
    backend.command(&["seek", "1", "absolute+exact"])?;
    backend.command(&["set", "pause", "no"])?;
    let start = std::time::Instant::now();
    let mut sought = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        backend.render(320, 180, &mut pixels, false)?;
        if backend
            .property("time-pos")
            .and_then(|value| value.parse::<f64>().ok())
            .is_some_and(|value| value >= 1.0)
        {
            sought = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !sought {
        return Err("Seek failed".into());
    }
    backend.command(&["stop"])?;
    println!("PASS: colored frames rendered in landscape and portrait; pause, resume, seek and stop succeeded");
    Ok(())
}
