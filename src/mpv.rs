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
    _library: Library,
}

impl Mpv {
    pub fn new(path: &Path) -> Result<Self, String> {
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
                destroy: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|error| error.to_string())?,
                _library: library,
            };
            player.handle = create();
            if player.handle.is_null() {
                return Err("无法创建播放内核".into());
            }
            for (name, value) in [
                ("config", "no"),
                ("vo", "libmpv"),
                ("hwdec", "no"),
                ("keep-open", "yes"),
                ("volume", "75"),
            ] {
                check(option(
                    player.handle,
                    CString::new(name).unwrap().as_ptr(),
                    CString::new(value).unwrap().as_ptr(),
                ))?;
            }
            check(initialize(player.handle))?;
            let mut parameters = [
                Parameter {
                    kind: 1,
                    data: c"sw".as_ptr().cast_mut().cast(),
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
            if !self.handle.is_null() {
                (self.destroy)(self.handle);
            }
        }
    }
}
