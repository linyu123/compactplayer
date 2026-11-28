use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

#[repr(C)]
struct Parameter {
    kind: c_int,
    data: *mut c_void,
}
type Free = unsafe extern "C" fn(*mut c_void);
type Command = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type Get = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type Render = unsafe extern "C" fn(*mut c_void, *mut Parameter) -> c_int;

#[repr(C)]
struct OpenGlInit {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    context: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    width: c_int,
    height: c_int,
    internal_format: c_int,
}

extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

// eframe 已在进程内加载 OpenGL.framework，直接从全局符号表取函数地址。
unsafe extern "C" fn gl_proc_address(_: *mut c_void, name: *const c_char) -> *mut c_void {
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
    dlsym(RTLD_DEFAULT, name)
}

pub struct Mpv {
    handle: *mut c_void,
    renderer: *mut c_void,
    command: Command,
    get: Get,
    free: Free,
    render: Render,
    update: unsafe extern "C" fn(*mut c_void) -> u64,
    render_free: Free,
    destroy: Free,
    set_update_callback:
        unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    // 回调数据需活到渲染上下文释放之后。
    waker: Option<Box<Box<dyn Fn() + Send + Sync>>>,
    opengl: bool,
    _library: Library,
}

unsafe extern "C" fn wake(data: *mut c_void) {
    (*(data as *const Box<dyn Fn() + Send + Sync>))();
}

impl Mpv {
    pub fn accepts_media(&self, path: &Path) -> bool {
        if self
            .command(&["loadfile", &path.to_string_lossy()])
            .is_err()
        {
            return false;
        }
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            if let Some(tracks) = self
                .property("track-list")
                .and_then(|value| serde_json::from_str::<Vec<serde_json::Value>>(&value).ok())
            {
                if tracks.iter().any(|track| {
                    track["type"] == "audio"
                        || (track["type"] == "video"
                            && track["image"] != true
                            && track["albumart"] != true)
                }) {
                    self.command(&["stop"]).ok();
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        self.command(&["stop"]).ok();
        false
    }

    pub fn probe(path: &Path) -> Result<Self, String> {
        Self::create(path, true, false, false)
    }

    #[allow(dead_code)]
    pub fn new(path: &Path) -> Result<Self, String> {
        Self::with_hwdec(path, true)
    }

    pub fn with_hwdec(path: &Path, hardware: bool) -> Result<Self, String> {
        Self::create(path, false, hardware, false)
    }

    /// OpenGL 渲染：硬解帧以 IOSurface 直接交给 GPU，无需拷回内存。
    /// 调用时当前线程必须持有 OpenGL 上下文。
    #[allow(dead_code)]
    pub fn with_opengl(path: &Path, hardware: bool) -> Result<Self, String> {
        Self::create(path, false, hardware, true)
    }

    #[allow(dead_code)]
    pub fn is_opengl(&self) -> bool {
        self.opengl
    }

    /// 当前渲染方式下的硬解选项：OpenGL 零拷贝，软件渲染需拷回内存。
    #[allow(dead_code)]
    pub fn hardware_decoder(&self) -> &'static str {
        if self.opengl {
            "videotoolbox"
        } else {
            "videotoolbox-copy"
        }
    }

    fn create(path: &Path, probe: bool, hardware: bool, opengl: bool) -> Result<Self, String> {
        unsafe {
            let library = Library::new(path).map_err(|error| error.to_string())?;
            let create = *library
                .get::<unsafe extern "C" fn() -> *mut c_void>(b"mpv_create\0")
                .map_err(|error| error.to_string())?;
            let initialize = *library
                .get::<unsafe extern "C" fn(*mut c_void) -> c_int>(b"mpv_initialize\0")
                .map_err(|error| error.to_string())?;
            let option = *library
                .get::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int>(
                    b"mpv_set_option_string\0",
                )
                .map_err(|error| error.to_string())?;
            let create_render = *library.get::<unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut Parameter) -> c_int>(b"mpv_render_context_create\0").map_err(|error| error.to_string())?;
            let mut player = Self {
                handle: std::ptr::null_mut(),
                renderer: std::ptr::null_mut(),
                command: *library
                    .get(b"mpv_command\0")
                    .map_err(|error| error.to_string())?,
                get: *library
                    .get(b"mpv_get_property_string\0")
                    .map_err(|error| error.to_string())?,
                free: *library
                    .get(b"mpv_free\0")
                    .map_err(|error| error.to_string())?,
                render: *library
                    .get(b"mpv_render_context_render\0")
                    .map_err(|error| error.to_string())?,
                update: *library
                    .get(b"mpv_render_context_update\0")
                    .map_err(|error| error.to_string())?,
                render_free: *library
                    .get(b"mpv_render_context_free\0")
                    .map_err(|error| error.to_string())?,
                set_update_callback: *library
                    .get(b"mpv_render_context_set_update_callback\0")
                    .map_err(|error| error.to_string())?,
                waker: None,
                destroy: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|error| error.to_string())?,
                opengl,
                _library: library,
            };
            player.handle = create();
            if player.handle.is_null() {
                return Err("无法创建播放内核".into());
            }
            for (name, value) in [
                ("config", "no"),
                ("vo", if probe { "null" } else { "libmpv" }),
                (
                    "hwdec",
                    match (hardware, opengl) {
                        (false, _) => "no",
                        (true, true) => "videotoolbox",
                        (true, false) => "videotoolbox-copy",
                    },
                ),
                ("keep-open", "yes"),
                ("volume", "75"),
                // egui 按 sRGB 合成；SDR 源的 gamma 由 Player::match_system_gamma 调整。
                ("target-trc", "srgb"),
            ] {
                check(option(
                    player.handle,
                    CString::new(name).unwrap().as_ptr(),
                    CString::new(value).unwrap().as_ptr(),
                ))?;
            }
            if probe {
                for (name, value) in [("ao", "null"), ("pause", "yes"), ("audio-display", "no")] {
                    check(option(
                        player.handle,
                        CString::new(name).unwrap().as_ptr(),
                        CString::new(value).unwrap().as_ptr(),
                    ))?;
                }
            }
            check(initialize(player.handle))?;
            if probe {
                return Ok(player);
            }
            let mut init = OpenGlInit {
                get_proc_address: gl_proc_address,
                context: std::ptr::null_mut(),
            };
            let mut parameters = [
                Parameter {
                    kind: 1,
                    data: if opengl {
                        c"opengl".as_ptr().cast_mut().cast()
                    } else {
                        c"sw".as_ptr().cast_mut().cast()
                    },
                },
                Parameter {
                    kind: if opengl { 2 } else { 0 },
                    data: (&mut init as *mut OpenGlInit).cast(),
                },
                Parameter {
                    kind: 0,
                    data: std::ptr::null_mut(),
                },
            ];
            check(create_render(
                &mut player.renderer,
                player.handle,
                parameters.as_mut_ptr(),
            ))?;
            Ok(player)
        }
    }

    /// mpv 有新画面时调用 `waker`（在 mpv 线程中，不能回调 mpv API），
    /// 界面据此按视频帧率重绘，而不是按屏幕刷新率轮询。
    #[allow(dead_code)]
    pub fn set_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        if self.renderer.is_null() {
            return;
        }
        let waker: Box<Box<dyn Fn() + Send + Sync>> = Box::new(Box::new(waker));
        unsafe {
            (self.set_update_callback)(
                self.renderer,
                Some(wake),
                &*waker as *const Box<dyn Fn() + Send + Sync> as *mut c_void,
            );
        }
        self.waker = Some(waker);
    }

    pub fn command(&self, values: &[&str]) -> Result<(), String> {
        let values = values
            .iter()
            .map(|value| CString::new(*value).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers = values
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        pointers.push(std::ptr::null());
        unsafe { check((self.command)(self.handle, pointers.as_ptr())) }
    }

    pub fn property(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        unsafe {
            let value = (self.get)(self.handle, name.as_ptr());
            if value.is_null() {
                return None;
            }
            let result = CStr::from_ptr(value).to_string_lossy().into_owned();
            (self.free)(value.cast());
            Some(result)
        }
    }

    pub fn render(
        &self,
        width: usize,
        height: usize,
        pixels: &mut [u8],
        force: bool,
    ) -> Result<bool, String> {
        if self.opengl {
            return Err("OpenGL 渲染模式不支持软件缓冲区".into());
        }
        if width == 0 || height == 0 || pixels.len() != width * height * 4 {
            return Err("视频缓冲区无效".into());
        }
        unsafe {
            if (self.update)(self.renderer) & 1 == 0 && !force {
                return Ok(false);
            }
            let mut size = [width as c_int, height as c_int];
            let mut stride = width * 4;
            let mut block: c_int = 0;
            let mut parameters = [
                Parameter {
                    kind: 17,
                    data: size.as_mut_ptr().cast(),
                },
                Parameter {
                    kind: 18,
                    data: c"rgba".as_ptr().cast_mut().cast(),
                },
                Parameter {
                    kind: 19,
                    data: (&mut stride as *mut usize).cast(),
                },
                Parameter {
                    kind: 20,
                    data: pixels.as_mut_ptr().cast(),
                },
                Parameter {
                    kind: 12,
                    data: (&mut block as *mut c_int).cast(),
                },
                Parameter {
                    kind: 0,
                    data: std::ptr::null_mut(),
                },
            ];
            check((self.render)(self.renderer, parameters.as_mut_ptr()))?;
            Ok(true)
        }
    }
}

impl Mpv {
    /// 渲染到 OpenGL framebuffer。mpv 默认（不翻转）输出第 0 行即画面顶部，
    /// 与 egui 纹理坐标一致。
    #[allow(dead_code)]
    pub fn render_opengl(
        &self,
        fbo: u32,
        width: usize,
        height: usize,
        force: bool,
    ) -> Result<bool, String> {
        if !self.opengl || width == 0 || height == 0 {
            return Err("OpenGL 渲染参数无效".into());
        }
        unsafe {
            if (self.update)(self.renderer) & 1 == 0 && !force {
                return Ok(false);
            }
            let mut target = OpenGlFbo {
                fbo: fbo as c_int,
                width: width as c_int,
                height: height as c_int,
                internal_format: 0,
            };
            let mut flip: c_int = 0;
            let mut block: c_int = 0;
            let mut parameters = [
                Parameter {
                    kind: 3,
                    data: (&mut target as *mut OpenGlFbo).cast(),
                },
                Parameter {
                    kind: 4,
                    data: (&mut flip as *mut c_int).cast(),
                },
                Parameter {
                    kind: 12,
                    data: (&mut block as *mut c_int).cast(),
                },
                Parameter {
                    kind: 0,
                    data: std::ptr::null_mut(),
                },
            ];
            check((self.render)(self.renderer, parameters.as_mut_ptr()))?;
            Ok(true)
        }
    }
}

fn check(result: c_int) -> Result<(), String> {
    if result < 0 {
        Err(format!("播放内核错误，错误码：{result}"))
    } else {
        Ok(())
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe {
            if !self.renderer.is_null() {
                (self.render_free)(self.renderer);
            }
            self.waker = None;
            if !self.handle.is_null() {
                (self.destroy)(self.handle);
            }
        }
    }
}
