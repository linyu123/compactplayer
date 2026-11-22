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
struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}

#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
}

pub enum Notification {
    Loaded,
    Failed(String),
    Log(String),
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
    wait_event: unsafe extern "C" fn(*mut c_void, f64) -> *const Event,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    _library: Library,
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
        Self::create(path, true)
    }

    pub fn new(path: &Path) -> Result<Self, String> {
        Self::with_hwdec(path, true)
    }

    pub fn with_hwdec(path: &Path, hardware: bool) -> Result<Self, String> {
        Self::create_mode(path, false, hardware)
    }

    fn create(path: &Path, probe: bool) -> Result<Self, String> {
        Self::create_mode(path, probe, false)
    }

    fn create_mode(path: &Path, probe: bool, hardware: bool) -> Result<Self, String> {
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
                wait_event: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|e| e.to_string())?,
                error_string: *library
                    .get(b"mpv_error_string\0")
                    .map_err(|e| e.to_string())?,
                _library: library,
            };
            player.handle = create();
            if player.handle.is_null() {
                return Err("无法创建播放内核".into());
            }
            for (name, value) in [
                ("config", "no"),
                ("vo", if probe { "null" } else { "libmpv" }),
                ("hwdec", if hardware { "auto-copy" } else { "no" }),
                ("keep-open", "yes"),
                ("volume", "75"),
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
            let request_log = *player
                ._library
                .get::<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>(
                    b"mpv_request_log_messages\0",
                )
                .map_err(|e| e.to_string())?;
            check(request_log(player.handle, c"warn".as_ptr()))?;
            if probe {
                return Ok(player);
            }
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
        unsafe {
            let result = (self.command)(self.handle, pointers.as_ptr());
            if result < 0 {
                Err(self.error(result))
            } else {
                Ok(())
            }
        }
    }

    fn error(&self, code: c_int) -> String {
        unsafe {
            let ptr = (self.error_string)(code);
            let message = if ptr.is_null() {
                "unknown".into()
            } else {
                CStr::from_ptr(ptr).to_string_lossy()
            };
            format!("播放内核错误：{message}（{code}）")
        }
    }

    pub fn notifications(&self) -> Vec<Notification> {
        let mut notifications = Vec::new();
        // 非阻塞且限量读取，避免日志突发占满 UI 帧。
        for _ in 0..64 {
            unsafe {
                let event = (self.wait_event)(self.handle, 0.0);
                if event.is_null() || (*event).id == 0 {
                    break;
                }
                match (*event).id {
                    8 => notifications.push(Notification::Loaded),
                    7 if !(*event).data.is_null() => {
                        let end = &*(*event).data.cast::<EndFile>();
                        if end.reason == 4 {
                            notifications.push(Notification::Failed(self.error(end.error)));
                        }
                    }
                    2 if !(*event).data.is_null() => {
                        let log = &*(*event).data.cast::<LogMessage>();
                        if !log.text.is_null() && !log.prefix.is_null() {
                            notifications.push(Notification::Log(format!(
                                "{}: {}",
                                CStr::from_ptr(log.prefix).to_string_lossy(),
                                CStr::from_ptr(log.text).to_string_lossy().trim()
                            )));
                        }
                    }
                    _ => {}
                }
            }
        }
        notifications
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
