use libloading::Library;
use std::ffi::{c_char, c_int, c_void, CStr, CString};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Usage: check_libmpv /path/to/libmpv.dylib")?;
    unsafe {
        let library = Library::new(path)?;
        let create = library.get::<unsafe extern "C" fn() -> *mut c_void>(b"mpv_create\0")?;
        let initialize =
            library.get::<unsafe extern "C" fn(*mut c_void) -> c_int>(b"mpv_initialize\0")?;
        let destroy =
            library.get::<unsafe extern "C" fn(*mut c_void)>(b"mpv_terminate_destroy\0")?;
        let option =
            library
                .get::<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int>(
                    b"mpv_set_option_string\0",
                )?;
        let get = library.get::<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char>(
            b"mpv_get_property_string\0",
        )?;
        let free = library.get::<unsafe extern "C" fn(*mut c_void)>(b"mpv_free\0")?;
        let handle = create();
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        for (name, value) in [
            ("config", "no"),
            ("vo", "null"),
            ("ao", "null"),
            ("terminal", "no"),
        ] {
            let name = CString::new(name)?;
            let value = CString::new(value)?;
            if option(handle, name.as_ptr(), value.as_ptr()) < 0 {
                destroy(handle);
                return Err("Failed to configure libmpv".into());
            }
        }
        let result = initialize(handle);
        if result < 0 {
            destroy(handle);
            return Err(format!("mpv_initialize failed: {result}").into());
        }
        let version = get(handle, c"mpv-version".as_ptr());
        if version.is_null() {
            destroy(handle);
            return Err("Cannot read mpv version".into());
        }
        println!("Initialized {}", CStr::from_ptr(version).to_string_lossy());
        free(version.cast());
        if let Some(media) = std::env::args().nth(2) {
            let command = library
                .get::<unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int>(
                    b"mpv_command\0",
                )?;
            let load = CString::new("loadfile")?;
            let media = CString::new(media)?;
            let arguments = [load.as_ptr(), media.as_ptr(), std::ptr::null()];
            if command(handle, arguments.as_ptr()) < 0 {
                destroy(handle);
                return Err("Cannot load test media".into());
            }
            let start = std::time::Instant::now();
            let mut progressed = false;
            while start.elapsed() < std::time::Duration::from_secs(10) {
                let position = get(handle, c"time-pos".as_ptr());
                if !position.is_null() {
                    let seconds = CStr::from_ptr(position)
                        .to_string_lossy()
                        .parse::<f64>()
                        .unwrap_or(0.0);
                    free(position.cast());
                    if seconds > 0.1 {
                        progressed = true;
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if !progressed {
                destroy(handle);
                return Err("Playback did not advance within 10 seconds".into());
            }
            println!("Headless playback advanced successfully");
        }
        destroy(handle);
    }
    Ok(())
}
