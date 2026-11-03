//! Windows 实现。10-16 起记的和 macOS 一样全，事件名和字段一致（见 app/INTERNAL.md 的事件表和对照）：
//! 键鼠（点击带元素、滚动、快捷键、打字与发送、人还是程序）、界面文字、切 app 与窗口变化、在场与心跳、
//! 睡眠唤醒锁屏熄屏、不记录的 app、暂停。
//!
//! - `hooks`：消息线程——低级键鼠钩子、前台切换、电源和会话通知；回调只收原始字段
//! - `sys`：进程和窗口（前台、程序路径和显示名、窗口标题、空闲）
//! - `uia`：UI Automation——焦点输入框的值、点中的元素、界面文字遍历
//! - `capture`：采集线程（判断逻辑同 macOS，共用 `common`）和 UIA 工作线程
//!
//! 已知限制：以管理员身份运行的窗口受 UIPI 限制，Beayy 不提权运行，读不到它的界面、也收不到它前台时的键鼠；
//! 部分 UWP、游戏窗口的 UIA 信息很少；分不出注入键鼠的是哪个进程（actor 只有人／程序两类）。

mod capture;
mod hooks;
mod sys;
mod uia;

use crate::PermissionStatus;
use beayy_core::EventLog;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub use sys::idle_seconds;

/// 键鼠事件的来源标记：是不是程序注入的、注入方是不是更低权限、注入方留的附加信息
#[derive(Clone, Copy)]
pub(crate) struct ActorRaw {
    pub injected: bool,
    pub lower_il: bool,
    pub extra: usize,
}

/// 发给采集线程的消息
pub(crate) enum Ev {
    Key { vk: u32, mods: u8, actor: ActorRaw },
    Mouse { button: &'static str, x: i32, y: i32, actor: ActorRaw },
    Scroll { actor: ActorRaw },
    /// 前台窗口换了（窗口句柄）
    Foreground(sys::Hwnd),
    /// 睡眠、唤醒、锁屏、解锁、熄屏、亮屏（system.sleep 等事件名）
    System(&'static str),
    /// 钩子装上／装失败／重装
    Hooks { ok: bool, error: String, reinstall: bool },
    /// 安排一次界面文字读取（工作线程记完点击后）
    ScheduleUi(&'static str, f64),
    /// 退出前把正在打的字落盘，完成后回一声
    Quit(Sender<()>),
}

static HANDLE: OnceLock<Mutex<Sender<Ev>>> = OnceLock::new();
static SHUT: AtomicBool = AtomicBool::new(false);

pub fn permissions() -> PermissionStatus {
    // Windows 没有对应的系统授权
    PermissionStatus { accessibility: true, input_monitoring: true, full_disk_access: true }
}

pub fn open_permission(_kind: &str) {}

/// 键鼠钩子装上了没有
pub fn capturing() -> bool {
    hooks::HOOKS_OK.load(Ordering::Relaxed)
}

/// 启动采集。`paused` 由界面和菜单改，采集线程自己发现变化、记暂停事件。
pub fn start(log: Arc<EventLog>, paused: Arc<AtomicBool>) {
    let tx = capture::spawn(log, paused);
    hooks::spawn(tx.clone());
    let _ = HANDLE.set(Mutex::new(tx));
}

/// 退出前调用：把正在打的字落盘（最多等 1 秒）。关机时消息线程也会调，只做一次
pub fn shutdown() {
    if SHUT.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(h) = HANDLE.get() else { return };
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(h) = h.lock() {
        let _ = h.send(Ev::Quit(tx));
    }
    let _ = rx.recv_timeout(Duration::from_secs(1));
}
