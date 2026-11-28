use eframe::egui;
use eframe::egui_glow;
use eframe::glow::{self, HasContext};
use std::sync::Arc;

/// mpv 通过 OpenGL 直接渲染到此纹理，egui 以原生纹理显示，画面不经过 CPU。
pub struct GlVideo {
    gl: Arc<glow::Context>,
    texture: glow::Texture,
    fbo: glow::Framebuffer,
    pub id: egui::TextureId,
    pub size: [usize; 2],
    srgb: bool,
}

impl GlVideo {
    pub fn new(gl: Arc<glow::Context>, frame: &mut eframe::Frame) -> Result<Self, String> {
        unsafe {
            let texture = gl.create_texture()?;
            let fbo = gl.create_framebuffer()?;
            // 与 egui_glow 的判断保持一致：启用 sRGB 纹理时，着色器会把采样值重新编码为 gamma，
            // 因此视频纹理也必须是 sRGB 格式，否则画面会发灰。
            let srgb = egui_glow::ShaderVersion::get(&gl) == egui_glow::ShaderVersion::Es300
                || gl
                    .supported_extensions()
                    .iter()
                    .any(|extension| extension.contains("sRGB"));
            let mut video = Self {
                id: frame.register_native_glow_texture(texture),
                gl,
                texture,
                fbo,
                size: [0, 0],
                srgb,
            };
            video.resize([16, 16])?;
            Ok(video)
        }
    }

    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }

    pub fn fbo(&self) -> u32 {
        self.fbo.0.get()
    }

    /// 调整纹理尺寸，返回是否发生变化。
    pub fn resize(&mut self, size: [usize; 2]) -> Result<bool, String> {
        if self.size == size || size[0] == 0 || size[1] == 0 {
            return Ok(false);
        }
        let gl = &self.gl;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            for (name, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                if self.srgb {
                    glow::SRGB8_ALPHA8
                } else {
                    glow::RGBA8
                } as i32,
                size[0] as i32,
                size[1] as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(self.texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("视频帧缓冲不可用：0x{status:x}"));
            }
        }
        self.size = size;
        Ok(true)
    }

    /// mpv 渲染后恢复 egui 使用的默认帧缓冲。
    pub fn finish(&self) {
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    /// 读回当前画面（第 0 行为画面顶部），仅在截图时调用。
    pub fn read(&self) -> egui::ColorImage {
        let [width, height] = self.size;
        let mut pixels = vec![0; width * height * 4];
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut pixels),
            );
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        crate::video_layout::opaque_frame(self.size, &pixels)
    }

    pub fn destroy(&self) {
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.texture);
        }
    }
}
