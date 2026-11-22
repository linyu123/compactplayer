//! 接收 Finder 的原生 open-documents AppleEvent，不替换 winit 的应用代理。
#[cfg(target_os = "macos")]
mod native {
    use objc2::{
        msg_send,
        runtime::{AnyClass, AnyObject, ClassBuilder, Sel},
        sel,
    };
    use std::{
        ffi::{c_char, CStr},
        path::PathBuf,
        sync::Mutex,
    };

    static FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    unsafe fn register_handler(handler: *const AnyObject) {
        let manager: *mut AnyObject = msg_send![
            AnyClass::get("NSAppleEventManager").unwrap(),
            sharedAppleEventManager
        ];
        let _: () = msg_send![manager, setEventHandler: handler andSelector: sel!(handleOpenDocuments:withReplyEvent:) forEventClass: u32::from_be_bytes(*b"aevt") andEventID: u32::from_be_bytes(*b"odoc")];
    }

    unsafe extern "C" fn will_finish(this: &AnyObject, _selector: Sel, _notification: &AnyObject) {
        register_handler(this);
    }

    unsafe extern "C" fn open_documents(
        _this: &AnyObject,
        _selector: Sel,
        event: &AnyObject,
        _reply: &AnyObject,
    ) {
        // keyDirectObject ('----') 包含 alias/file URL 描述符数组，索引从 1 开始。
        let list: *mut AnyObject =
            msg_send![event, paramDescriptorForKeyword: u32::from_be_bytes(*b"----")];
        if list.is_null() {
            return;
        }
        let count: isize = msg_send![list, numberOfItems];
        let mut paths = Vec::new();
        for index in 1..=count {
            let item: *mut AnyObject = msg_send![list, descriptorAtIndex: index];
            if item.is_null() {
                continue;
            }
            let url: *mut AnyObject = msg_send![item, fileURLValue];
            if url.is_null() {
                continue;
            }
            let path: *mut AnyObject = msg_send![url, path];
            if path.is_null() {
                continue;
            }
            let utf8: *const c_char = msg_send![path, UTF8String];
            if !utf8.is_null() {
                paths.push(PathBuf::from(
                    CStr::from_ptr(utf8).to_string_lossy().into_owned(),
                ));
            }
        }
        if let Ok(mut files) = FILES.lock() {
            #[cfg(debug_assertions)]
            eprintln!("Finder 打开文件：{paths:?}");
            files.extend(paths);
        }
    }

    pub fn install() {
        unsafe {
            // 启动前注册，并在 will-finish-launching 时重设，避免被 AppKit 初始化覆盖。
            let _: *mut AnyObject =
                msg_send![AnyClass::get("NSApplication").unwrap(), sharedApplication];
            let class = if let Some(class) = AnyClass::get("CompactPlayerOpenDocuments") {
                class
            } else {
                let mut builder = ClassBuilder::new(
                    "CompactPlayerOpenDocuments",
                    AnyClass::get("NSObject").unwrap(),
                )
                .unwrap();
                builder.add_method(
                    sel!(handleOpenDocuments:withReplyEvent:),
                    open_documents as unsafe extern "C" fn(_, _, _, _),
                );
                builder.add_method(
                    sel!(willFinishLaunching:),
                    will_finish as unsafe extern "C" fn(_, _, _),
                );
                builder.register()
            };
            let handler: *mut AnyObject = msg_send![class, new];
            register_handler(handler);
            let center: *mut AnyObject = msg_send![
                AnyClass::get("NSNotificationCenter").unwrap(),
                defaultCenter
            ];
            let name: *mut AnyObject = msg_send![AnyClass::get("NSString").unwrap(), stringWithUTF8String: c"NSApplicationWillFinishLaunchingNotification".as_ptr()];
            let _: () = msg_send![center, addObserver: handler selector: sel!(willFinishLaunching:) name: name object: std::ptr::null::<AnyObject>()];
            // handler 持有的 +1 引用保留至进程结束，确保原生回调始终有效。
        }
    }

    pub fn take() -> Vec<PathBuf> {
        FILES
            .lock()
            .map(|mut files| std::mem::take(&mut *files))
            .unwrap_or_default()
    }
}

#[cfg(target_os = "macos")]
pub use native::{install, take};

#[cfg(not(target_os = "macos"))]
pub fn install() {}
#[cfg(not(target_os = "macos"))]
pub fn take() -> Vec<std::path::PathBuf> {
    Vec::new()
}
