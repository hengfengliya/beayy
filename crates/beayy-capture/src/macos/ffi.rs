//! CoreFoundation、CoreGraphics（事件监听）、辅助功能（AX）的最小 FFI，外加一个自动释放的 CF 句柄。

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = CFTypeRef;
pub type CFRunLoopRef = CFTypeRef;
pub type CFRunLoopSourceRef = CFTypeRef;
pub type CFRunLoopTimerRef = CFTypeRef;
pub type CFMachPortRef = CFTypeRef;
pub type CGEventRef = CFTypeRef;
pub type AXUIElementRef = CFTypeRef;
pub type AXObserverRef = CFTypeRef;
pub type CFIndex = isize;
pub type CFTypeID = usize;
pub type Boolean = u8;
pub type AXError = i32;
pub type Pid = i32;

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kAXErrorSuccess: AXError = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CFRange {
    pub location: CFIndex,
    pub length: CFIndex,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub const kCFNumberSInt32Type: CFIndex = 3;
pub const kCFNumberFloat64Type: CFIndex = 6;
pub const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
pub const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> Boolean;
    pub fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    pub fn CFStringGetTypeID() -> CFTypeID;
    pub fn CFArrayGetTypeID() -> CFTypeID;
    pub fn CFURLGetTypeID() -> CFTypeID;
    pub fn CFStringCreateWithBytes(alloc: CFTypeRef, bytes: *const u8, len: CFIndex, enc: u32, external: Boolean) -> CFStringRef;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetBytes(
        s: CFStringRef,
        range: CFRange,
        enc: u32,
        loss: u8,
        external: Boolean,
        buf: *mut u8,
        max: CFIndex,
        used: *mut CFIndex,
    ) -> CFIndex;
    pub fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    pub fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> Boolean;
    pub fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    pub fn CFURLGetString(url: CFTypeRef) -> CFStringRef;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRemoveSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source: Boolean) -> i32;
    pub fn CFRunLoopWakeUp(rl: CFRunLoopRef);
    pub fn CFMachPortCreateRunLoopSource(alloc: CFTypeRef, port: CFMachPortRef, order: CFIndex) -> CFRunLoopSourceRef;
    pub fn CFAbsoluteTimeGetCurrent() -> f64;
    pub fn CFRunLoopTimerCreate(
        alloc: CFTypeRef,
        fire: f64,
        interval: f64,
        flags: u64,
        order: CFIndex,
        callout: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
        ctx: *mut c_void,
    ) -> CFRunLoopTimerRef;
    pub fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
}

// CGEventType
pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventScrollWheel: u32 = 22;
pub const kCGEventOtherMouseDown: u32 = 25;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;
// CGEventField
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGEventSourceUnixProcessID: u32 = 41;
pub const kCGEventSourceUserData: u32 = 42;
pub const kCGEventSourceStateID: u32 = 45;
// CGEventFlags
pub const kCGEventFlagMaskShift: u64 = 0x0002_0000;
pub const kCGEventFlagMaskControl: u64 = 0x0004_0000;
pub const kCGEventFlagMaskAlternate: u64 = 0x0008_0000;
pub const kCGEventFlagMaskCommand: u64 = 0x0010_0000;

pub type TapCallback = extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGRequestListenEventAccess() -> bool;
    /// tap 1＝kCGSessionEventTap，place 0＝头部插入，options 1＝只监听
    pub fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCallback, info: *mut c_void) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(e: CGEventRef) -> u64;
    pub fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    pub fn CGEventKeyboardGetUnicodeString(e: CGEventRef, max: usize, actual: *mut usize, buf: *mut u16);
    /// state 0＝combinedSessionState，type !0＝任意输入
    pub fn CGEventSourceSecondsSinceLastEventType(state: i32, ty: u32) -> f64;
    pub static kCGWindowOwnerPID: CFStringRef;
    pub static kCGWindowBounds: CFStringRef;
    pub static kCGWindowAlpha: CFStringRef;
    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    pub fn CGRectMakeWithDictionaryRepresentation(dict: CFTypeRef, rect: *mut CGRect) -> bool;
}

pub type AXObserverCallback = extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(opts: CFTypeRef) -> bool;
    pub fn AXUIElementCreateApplication(pid: Pid) -> AXUIElementRef;
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> AXError;
    pub fn AXUIElementCopyElementAtPosition(app: AXUIElementRef, x: f32, y: f32, out: *mut AXUIElementRef) -> AXError;
    pub fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut Pid) -> AXError;
    pub fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, seconds: f32) -> AXError;
    pub fn AXObserverCreate(pid: Pid, cb: AXObserverCallback, out: *mut AXObserverRef) -> AXError;
    pub fn AXObserverAddNotification(obs: AXObserverRef, el: AXUIElementRef, name: CFStringRef, refcon: *mut c_void) -> AXError;
    pub fn AXObserverGetRunLoopSource(obs: AXObserverRef) -> CFRunLoopSourceRef;
}

/// 持有一个 CF 对象（Create／Copy 规则拿到的），离开作用域时释放
pub struct CF(CFTypeRef);

// CF 对象的引用计数是线程安全的；AX 元素可以跨线程使用
unsafe impl Send for CF {}
unsafe impl Sync for CF {}

impl CF {
    /// 接管一个 +1 的引用；空指针返回 None
    pub fn owned(p: CFTypeRef) -> Option<CF> {
        // 不能写 then_some(CF(p))：参数会先构造，空指针随后被丢弃时就成了 CFRelease(NULL)，直接崩溃
        (!p.is_null()).then(|| CF(p))
    }
    /// 借来的引用（Get 规则）：先 retain 再持有
    pub fn retained(p: CFTypeRef) -> Option<CF> {
        (!p.is_null()).then(|| CF(unsafe { CFRetain(p) }))
    }
    pub fn ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Clone for CF {
    fn clone(&self) -> Self {
        CF(unsafe { CFRetain(self.0) })
    }
}

impl Drop for CF {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

/// Rust 字符串 → CFString
pub fn cfstr(s: &str) -> CF {
    CF(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, kCFStringEncodingUTF8, 0) })
}

/// CFString → Rust 字符串；不是 CFString 返回 None
pub fn string_of(p: CFTypeRef) -> Option<String> {
    if p.is_null() || unsafe { CFGetTypeID(p) != CFStringGetTypeID() } {
        return None;
    }
    unsafe {
        let len = CFStringGetLength(p);
        let range = CFRange { location: 0, length: len };
        let mut need: CFIndex = 0;
        CFStringGetBytes(p, range, kCFStringEncodingUTF8, b'?', 0, std::ptr::null_mut(), 0, &mut need);
        let mut buf = vec![0u8; need as usize];
        let mut used: CFIndex = 0;
        CFStringGetBytes(p, range, kCFStringEncodingUTF8, b'?', 0, buf.as_mut_ptr(), need, &mut used);
        buf.truncate(used as usize);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}
