use eframe::egui;

/// 软件渲染只输出视频区域。黑边由黑色中央面板提供，不上传黑边像素。
pub fn layout(
    available: egui::Vec2,
    source: [f32; 2],
    pixels_per_point: f32,
    rotated: bool,
) -> (egui::Vec2, [usize; 2]) {
    let bounds = if rotated {
        egui::vec2(available.y, available.x)
    } else {
        available
    };
    let source = egui::vec2(source[0].max(1.0), source[1].max(1.0));
    let fit = (bounds.x / source.x).min(bounds.y / source.y).max(0.001);
    let display = source * fit;
    // 限制在原视频像素和 4K 像素预算内，避免在 CPU 上放大低分辨率源。
    let scale = (fit * pixels_per_point.max(1.0))
        .min(1.0)
        .min((8_294_400.0 / (source.x * source.y)).sqrt());
    (
        display,
        [
            (source.x * scale).round().max(1.0) as usize,
            (source.y * scale).round().max(1.0) as usize,
        ],
    )
}

pub fn update_opaque_frame(image: &mut egui::ColorImage, size: [usize; 2], pixels: &[u8]) {
    assert_eq!(image.size, size);
    assert_eq!(pixels.len(), size[0] * size[1] * 4);
    for (target, source) in image.pixels.iter_mut().zip(pixels.chunks_exact(4)) {
        *target = egui::Color32::from_rgb(source[0], source[1], source[2]);
    }
}

pub fn opaque_frame(size: [usize; 2], pixels: &[u8]) -> egui::ColorImage {
    assert_eq!(pixels.len(), size[0] * size[1] * 4);
    // 视频画面已合成字幕，不需要 egui 对每个像素做透明度预乘。
    egui::ColorImage {
        size,
        pixels: pixels
            .chunks_exact(4)
            .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retina_portrait_and_rotation_preserve_source_pixels() {
        let (display, size) = layout(egui::vec2(900.0, 600.0), [1080.0, 1920.0], 2.0, false);
        assert_eq!(display, egui::vec2(337.5, 600.0));
        assert_eq!(size, [675, 1200]);
        let (_, size) = layout(egui::vec2(900.0, 600.0), [1920.0, 1080.0], 2.0, false);
        assert_eq!(size, [1800, 1013]);
        let (display, size) = layout(egui::vec2(600.0, 900.0), [1920.0, 1080.0], 2.0, true);
        assert_eq!(display, egui::vec2(900.0, 506.25));
        assert_eq!(size, [1800, 1013]);
        assert_eq!(
            layout(egui::vec2(3000.0, 2000.0), [640.0, 360.0], 2.0, false).1,
            [640, 360]
        );
        let (_, size) = layout(egui::vec2(8000.0, 8000.0), [7680.0, 4320.0], 2.0, false);
        assert_eq!(size, [3840, 2160]);
    }
    #[test]
    fn opaque_conversion_preserves_rgb_even_when_alpha_is_undefined() {
        assert_eq!(
            opaque_frame([1, 1], &[10, 20, 30, 0]).pixels[0].to_array(),
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn reuses_color_buffer_and_updates_rgb_pixels() {
        let mut image = opaque_frame([1, 1], &[10, 20, 30, 0]);
        update_opaque_frame(&mut image, [1, 1], &[200, 150, 100, 255]);
        assert_eq!(image.pixels[0].to_array(), [200, 150, 100, 255]);
    }
}
