// 辅助功能读取的小工具。从 Swift 版 AXUtil.swift 移植，那份改编自 OpenChronicle（resources/mac-ax-watcher.swift）。
// Copyright (c) 2026 Einsia. MIT License，全文见 app/THIRD_PARTY_NOTICES.md。

use super::ffi::*;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
pub use crate::common::{merge, num, truncate};
use serde_json::{json, Map, Value};

/// 读元素的一个属性，原样返回 CF 对象
pub fn attr(el: &CF, name: &str) -> Option<CF> {
    let key = cfstr(name);
    let mut out: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(el.ptr(), key.ptr(), &mut out) };
    if err != kAXErrorSuccess {
        return None;
    }
    CF::owned(out)
}

pub fn ax_string(el: &CF, name: &str) -> Option<String> {
    attr(el, name).and_then(|v| string_of(v.ptr()))
}

pub fn role(el: &CF) -> Option<String> {
    ax_string(el, "AXRole")
}

pub fn subrole(el: &CF) -> Option<String> {
    ax_string(el, "AXSubrole")
}

/// 密码框
pub fn is_secure(el: &CF) -> bool {
    subrole(el).as_deref() == Some("AXSecureTextField")
}

pub fn app_element(pid: Pid) -> Option<CF> {
    CF::owned(unsafe { AXUIElementCreateApplication(pid) })
}

pub fn focused_element(app: &CF) -> Option<CF> {
    attr(app, "AXFocusedUIElement")
}

pub fn focused_window(app: &CF) -> Option<CF> {
    attr(app, "AXFocusedWindow")
}

pub fn is_focused_secure(app: &CF) -> bool {
    focused_element(app).map(|e| is_secure(&e)).unwrap_or(false)
}

/// 读元素的值；密码框返回 [REDACTED]
pub fn element_value(el: &CF, max: usize) -> String {
    if is_secure(el) {
        return "[REDACTED]".into();
    }
    truncate(&ax_string(el, "AXValue").unwrap_or_default(), max)
}

pub fn describe(el: &CF) -> Map<String, Value> {
    let s = |k: &str| truncate(&ax_string(el, k).unwrap_or_default(), 200);
    let mut m = Map::new();
    m.insert("role".into(), json!(role(el).unwrap_or_default()));
    m.insert("subrole".into(), json!(subrole(el).unwrap_or_default()));
    m.insert("title".into(), json!(s("AXTitle")));
    m.insert("description".into(), json!(s("AXDescription")));
    m.insert("identifier".into(), json!(s("AXIdentifier")));
    m.insert("value".into(), json!(element_value(el, 500)));
    m
}

pub fn element_pid(el: &CF) -> Pid {
    let mut pid: Pid = 0;
    unsafe { AXUIElementGetPid(el.ptr(), &mut pid) };
    pid
}

/// 进程的 app 名和 bundle id
pub fn app_names(pid: Pid) -> (String, String) {
    objc2::rc::autoreleasepool(|_| {
        match NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            Some(app) => (
                app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
                app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
            ),
            None => (String::new(), String::new()),
        }
    })
}

pub fn app_info(pid: Pid) -> Map<String, Value> {
    let (name, bundle) = app_names(pid);
    let mut m = Map::new();
    m.insert("pid".into(), json!(pid));
    m.insert("app_name".into(), json!(name));
    m.insert("bundle_id".into(), json!(bundle));
    m
}

/// 屏幕上这个点最上层的窗口属于哪个进程（按窗口服务器的前后顺序找第一个盖住它的可见窗口）。
/// 只问窗口服务器，不碰任何 app 的界面对象，哪个线程调都安全。找不到返回 None。
pub fn window_owner_at(x: f64, y: f64) -> Option<Pid> {
    let list = CF::owned(unsafe { CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, 0) })?;
    let n = unsafe { CFArrayGetCount(list.ptr()) };
    for i in 0..n {
        let d = unsafe { CFArrayGetValueAtIndex(list.ptr(), i) };
        if d.is_null() {
            continue;
        }
        let mut alpha = 1.0f64;
        let a = unsafe { CFDictionaryGetValue(d, kCGWindowAlpha) };
        if !a.is_null() {
            unsafe { CFNumberGetValue(a, kCFNumberFloat64Type, &mut alpha as *mut f64 as *mut _) };
        }
        if alpha <= 0.0 {
            continue;
        }
        let b = unsafe { CFDictionaryGetValue(d, kCGWindowBounds) };
        let mut r = CGRect::default();
        if b.is_null() || !unsafe { CGRectMakeWithDictionaryRepresentation(b, &mut r) } {
            continue;
        }
        if x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h {
            continue;
        }
        let p = unsafe { CFDictionaryGetValue(d, kCGWindowOwnerPID) };
        let mut pid: i32 = 0;
        if p.is_null() || unsafe { CFNumberGetValue(p, kCFNumberSInt32Type, &mut pid as *mut i32 as *mut _) } == 0 {
            return None;
        }
        return Some(pid as Pid);
    }
    None
}

/// 前台 app 的进程号和 bundle id
pub fn frontmost() -> Option<(Pid, String)> {
    objc2::rc::autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        Some((app.processIdentifier(), app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default()))
    })
}

pub fn window_title(pid: Pid) -> String {
    let Some(app) = app_element(pid) else { return String::new() };
    if is_focused_secure(&app) {
        return "[REDACTED]".into();
    }
    focused_window(&app).and_then(|w| ax_string(&w, "AXTitle")).unwrap_or_default()
}
