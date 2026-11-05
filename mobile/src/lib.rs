//! Beayy 手机版的 Rust 核心：和电脑同一套数据代码。iPhone app（app/ios）里的网页视图加载同一套 ui/，
//! 网页的 `invoke(命令, 参数)` 经 Swift 转到这里的 `beayy_call`，按命令名调到和电脑一样的函数，返回 JSON。
//! - 数据代码直接引用电脑外壳的源文件（#[path]），算法只有一份；只编不依赖桌面的部分（不采集、不读屏幕使用、没有菜单栏和更新）。
//! - 账号同步只下载：电脑的分段、摘要拉下来，界面照电脑那样算；录音、停留点经账号同步拿、手机自己也能算，
//!   写进带「-m」后缀的设备目录（读的时候按停留点 id、段 id、句子去重）。电脑关机时手机也看得到。
//! - 定位、手表录音中转、加密上传是原生 Swift；账号凭据读这里存的同一份（数据目录 sync/account）。
//! - 安卓（app/android）同一套：Kotlin 外壳经 JNI 调 `Core.call`，到这里的同一个 dispatch。安卓能直接读每个 app 的使用时长
//!   （UsageStatsManager），所以它是一台采集设备：设备名 android-…，记成和 iPhone 一样的 app.focus 事件，随账号同步上传给电脑。
//! 方案见 报告/方案_20261021_手机同一套代码.md、报告/方案_20261030_安卓.md

#[path = "../../src-tauri/src/data.rs"]
#[allow(dead_code)]
mod data;
#[path = "../../src-tauri/src/amap.rs"]
mod amap;
#[path = "../../src-tauri/src/sync.rs"]
#[allow(dead_code)]
mod sync;
#[path = "../../src-tauri/src/storage.rs"]
mod storage;
#[path = "../../src-tauri/src/health.rs"]
mod health;
#[path = "../../src-tauri/src/icons.rs"]
mod icons;
// 只用读的那几个（get_week、get_month）：手机不写总结，看电脑同步来的
#[path = "../../src-tauri/src/daily.rs"]
#[allow(dead_code)]
mod daily;

use beayy_core::{data_root, default_root, EventLog};
use beayy_remote::Relay;
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

/// 录音和停留点（账号同步）多久一轮
const CLOUD_EVERY: Duration = Duration::from_secs(120);

static CLOUD: Mutex<String> = Mutex::new(String::new());
static START: Once = Once::new();

/// 这台手机在同步里的设备名：iosapp-<8 位十六进制>（安卓 android-…），第一次启动生成存在数据目录。
/// 不能和电脑那边的 iphone-…（屏幕使用、停留点）同名，否则同步会以为那是本机的、不下载
fn device_id() -> String {
    let f = default_root().join("mobile_device");
    if let Ok(s) = std::fs::read_to_string(&f) {
        if !s.trim().is_empty() {
            return s.trim().to_string();
        }
    }
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let id = format!("{}-{:08x}", if cfg!(target_os = "android") { "android" } else { "iosapp" }, (t.subsec_nanos() ^ t.as_secs() as u32).wrapping_mul(2654435761));
    let _ = std::fs::create_dir_all(default_root());
    let _ = std::fs::write(&f, &id);
    id
}

/// 网页报错写进数据目录 logs/web.txt（手机上没有开发者工具，排查用）
fn web_log(text: &str) {
    let dir = default_root().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("web.txt")) {
        let _ = writeln!(f, "{} {}", chrono::Utc::now().to_rfc3339(), text.chars().take(2000).collect::<String>());
    }
}

/// 手机版的状态：没有采集、没有权限，只有同步和录音的进度
fn get_status() -> Value {
    json!({
        "mobile": true, "capturing": false, "paused": false, "permissions": {}, "events_today": 0,
        "device": beayy_core::local_device(), "version": env!("CARGO_PKG_VERSION"),
        "cloud_sync": CLOUD.lock().unwrap().clone(), "relay_sync": "", "login_item": false,
    })
}

/// 启动后台：账号同步（只下载）、录音和停留点。只跑一次
fn start() {
    START.call_once(|| {
        std::env::set_var("BEAYY_DEVICE", device_id());
        let root = data_root();
        // 同步日志不放进 devices/：iPhone 不是一台采集设备，放进去设备页会把它当「这台」、去查电脑那些权限。
        // 安卓是（app 使用记录写在 devices/android-…/，同步上传）
        if !cfg!(target_os = "android") {
            let _ = std::fs::remove_dir_all(root.join("devices").join(beayy_core::local_device()));
        }
        sync::start(Arc::new(EventLog::new(root.join("mobile"), beayy_core::local_device())));
        std::thread::spawn(move || {
            let mut relay = Relay::default();
            relay.device_suffix = "-m".into();
            loop {
                let msg = match beayy_remote::cloud::round(&mut relay, &root, Duration::from_secs(90)) {
                    Ok(n) => n.summary(),
                    Err(e) => e,
                };
                *CLOUD.lock().unwrap() = msg;
                std::thread::sleep(CLOUD_EVERY);
            }
        });
    });
}

// ---------- 安卓：app 使用记录 ----------

/// 安卓外壳读到的前台切换（UsageStatsManager 的 ACTIVITY_RESUMED／PAUSED），记成和 iPhone 一样的 app.focus 事件：
/// biome_time 是 Unix 秒，另带 app_name（安卓包名查不到名字，电脑那边靠它显示）。
/// 外壳每次从上次读到的地方往后读；这里再按「读到哪了」（数据目录 android_cursor）挡一次重复。返回记了几条
fn android_usage(events: Vec<Value>) -> Result<usize, String> {
    let root = data_root();
    let f = root.join("android_cursor");
    let mut cursor: f64 = std::fs::read_to_string(&f).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    let log = EventLog::new(root.clone(), beayy_core::local_device());
    let mut n = 0;
    for e in events {
        let (Some(t), Some(pkg)) = (e["t"].as_f64(), e["package"].as_str()) else { continue };
        if t <= cursor {
            continue;
        }
        let Some(at) = chrono::DateTime::<chrono::Utc>::from_timestamp_micros((t * 1e6) as i64) else { continue };
        log.write(
            "app.focus",
            beayy_core::fields! {
                "state" => if e["start"].as_bool().unwrap_or(false) { "start" } else { "end" },
                "bundle_id" => pkg,
                "app_name" => e["name"].as_str().unwrap_or(""),
                "biome_time" => t,
                "source" => "android:UsageStats",
                "recorded_at" => beayy_core::iso(chrono::Utc::now()),
            },
            Some(at),
        );
        cursor = t;
        n += 1;
    }
    if n > 0 {
        std::fs::write(&f, cursor.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(n)
}

/// 外壳问「上次读到哪了」：没有记录时给 0（外壳就从 7 天前读起）
fn android_cursor() -> f64 {
    std::fs::read_to_string(data_root().join("android_cursor")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0)
}

// ---------- 网页的命令 ----------

/// 取一个参数：网页按 Tauri 的习惯传驼峰（placeId），也认下划线
fn arg<T: DeserializeOwned>(a: &Map<String, Value>, snake: &str) -> Result<T, String> {
    let camel: String = snake.split('_').enumerate().map(|(i, p)| if i == 0 { p.to_string() } else { p[..1].to_uppercase() + &p[1..] }).collect();
    let v = a.get(&camel).or_else(|| a.get(snake)).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| format!("参数 {snake} 不对：{e}"))
}

fn ok<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

/// 命令名 → 和电脑同一个函数。返回 Ok(结果) 或 Err(错误文字)
fn dispatch(cmd: &str, a: &Map<String, Value>) -> Result<Value, String> {
    use pollster::block_on as wait;
    match cmd {
        "get_status" => ok(get_status()),
        "web_log" => { web_log(&arg::<String>(a, "text")?); ok(()) }
        "list_days" => ok(data::list_days()),
        "get_day" => ok(data::get_day(arg(a, "date")?)),
        "get_range" => ok(data::get_range(arg(a, "from")?, arg(a, "to")?)?),
        "get_week" => ok(daily::get_week(arg(a, "date")?)?),
        "get_month" => ok(daily::get_month(arg(a, "date")?)?),
        "llm_usage" => ok(data::llm_usage()),
        "get_rules" => ok(data::get_rules()),
        "save_rules" => ok(data::save_rules(arg(a, "rules")?)?),
        "get_settings" => ok(data::get_settings()),
        "save_settings" => ok(data::save_settings(arg(a, "settings")?)?),
        "storage_stats" => ok(wait(storage::storage_stats())),
        "llm_status" => ok(wait(data::llm_status())),
        "test_model" => ok(wait(data::test_model())?),
        "delete_since" => ok(data::delete_since(arg(a, "minutes")?, arg(a, "today")?)?),
        "list_devices" => ok(data::list_devices()),
        "rename_device" => ok(data::rename_device(arg(a, "id")?, arg(a, "name")?)?),
        "recent_events" => ok(data::recent_events(arg(a, "device")?, arg(a, "before")?, arg(a, "limit")?, arg(a, "types")?)?),
        "set_place_label" => ok(data::set_place_label(arg(a, "lat")?, arg(a, "lon")?, arg(a, "label")?, arg(a, "kind")?)?),
        "set_place_pick" => ok(data::set_place_pick(arg(a, "id")?, arg(a, "shop")?, arg(a, "kind")?, arg(a, "lat")?, arg(a, "lon")?)?),
        "clear_place_pick" => ok(data::clear_place_pick(arg(a, "id")?)?),
        "list_place_labels" => ok(data::list_place_labels()),
        "place_stats" => ok(data::place_stats(arg(a, "days")?)),
        "edit_place_label" => ok(data::edit_place_label(arg(a, "index")?, arg(a, "label")?, arg(a, "kind")?, arg(a, "r")?, arg(a, "address")?)?),
        "get_profile" => ok(data::get_profile()),
        "save_profile" => ok(wait(data::save_profile(arg(a, "name")?, arg(a, "avatar")?))?),
        "list_projects" => ok(data::list_projects()),
        "merge_project" => ok(data::merge_project(arg(a, "from")?, arg(a, "into")?)?),
        "set_project_domain" => ok(data::set_project_domain(arg(a, "project")?, arg(a, "domain")?)?),
        "set_amap_key" => ok(amap::set_amap_key(arg(a, "key")?)?),
        "amap_js_config" => ok(wait(amap::amap_js_config())),
        "set_amap_js" => ok(amap::set_amap_js(arg(a, "key")?, arg(a, "code")?)?),
        "amap_around" => ok(amap::amap_around(arg(a, "lat")?, arg(a, "lon")?, arg(a, "radius")?)?),
        "amap_search" => ok(amap::amap_search(arg(a, "text")?, arg(a, "lat")?, arg(a, "lon")?)?),
        "amap_map" => ok(amap::amap_map(arg(a, "points")?, arg(a, "width")?, arg(a, "height")?, arg(a, "selected")?, arg(a, "often")?)?),
        "get_icons" => ok(wait(icons::get_icons(arg(a, "ids")?, arg(a, "phone")?))),
        "sync_status" => ok(sync::sync_status()),
        "sync_now" => ok(wait(sync::sync_now())?),
        "account_signup" => ok(wait(sync::account_signup(arg(a, "invite")?, arg(a, "email")?, arg(a, "password")?))?),
        "signup_info" => ok(wait(sync::signup_info())),
        "account_login" => ok(wait(sync::account_login(arg(a, "email")?, arg(a, "password")?))?),
        "account_claim" => ok(wait(sync::account_claim(arg(a, "email")?, arg(a, "password")?))?),
        "account_recover" => ok(wait(sync::account_recover(arg(a, "email")?, arg(a, "code")?, arg(a, "password")?))?),
        "account_password" => ok(wait(sync::account_password(arg(a, "password")?))?),
        "account_info" => ok(wait(sync::account_info())?),
        "account_remove_device" => ok(wait(sync::account_remove_device(arg(a, "id")?))?),
        "account_logout" => ok(wait(sync::account_logout())),
        "account_recovery_code" => ok(sync::account_recovery_code()),
        // 不要 Beayy 账号的同步：WebDAV、iCloud（手机上没有任选文件夹，iCloud 就是 iCloud 容器里的同步文件夹）
        "webdav_state" => ok(wait(sync::webdav_state(arg(a, "url")?, arg(a, "user")?, arg(a, "pass")?))),
        "webdav_create" => ok(wait(sync::webdav_create(arg(a, "url")?, arg(a, "user")?, arg(a, "pass")?, arg(a, "password")?))?),
        "webdav_join" => ok(wait(sync::webdav_join(arg(a, "url")?, arg(a, "user")?, arg(a, "pass")?, arg(a, "password")?))?),
        "webdav_recover" => ok(wait(sync::webdav_recover(arg(a, "url")?, arg(a, "user")?, arg(a, "pass")?, arg(a, "code")?, arg(a, "password")?))?),
        "icloud_state" => ok(wait(sync::icloud_state())),
        "icloud_create" => ok(wait(sync::icloud_create(arg(a, "password")?))?),
        "icloud_join" => ok(wait(sync::icloud_join(arg(a, "password")?))?),
        "icloud_recover" => ok(wait(sync::icloud_recover(arg(a, "code")?, arg(a, "password")?))?),
        "android_usage" => ok(android_usage(arg(a, "events")?)?),
        "android_cursor" => ok(android_cursor()),
        _ => Err(format!("手机版没有这个功能：{cmd}")),
    }
}

/// 网页调一个命令：cmd 是命令名，args 是 JSON 对象；返回 `{"ok": 结果}` 或 `{"err": "…"}`（用 beayy_free 释放）。
/// 会阻塞（联网的命令要几秒），Swift 在后台队列里调
///
/// # Safety
/// cmd、args 要是有效的、以 0 结尾的 UTF-8 字符串
#[no_mangle]
pub unsafe extern "C" fn beayy_call(cmd: *const c_char, args: *const c_char) -> *mut c_char {
    start();
    let cmd = CStr::from_ptr(cmd).to_string_lossy().to_string();
    let args: Map<String, Value> = serde_json::from_str(&CStr::from_ptr(args).to_string_lossy()).unwrap_or_default();
    let out = match std::panic::catch_unwind(|| dispatch(&cmd, &args)) {
        Ok(Ok(v)) => json!({ "ok": v }),
        Ok(Err(e)) => json!({ "err": e }),
        Err(_) => json!({ "err": format!("{cmd} 出错了") }),
    };
    CString::new(out.to_string()).unwrap_or_default().into_raw()
}

/// 启动后台同步（app 启动时调一次；beayy_call 也会顺带启动）
#[no_mangle]
pub extern "C" fn beayy_start() {
    start();
}

/// 释放 beayy_call 返回的字符串
///
/// # Safety
/// p 必须是 beayy_call 返回的、还没释放过的指针
#[no_mangle]
pub unsafe extern "C" fn beayy_free(p: *mut c_char) {
    if !p.is_null() {
        drop(CString::from_raw(p));
    }
}

/// 安卓的 JNI 入口（com.fengli.beayy.Core）：init 先把 HOME 指到 app 的私有目录（数据根＝HOME/Library/Application Support/Beayy，
/// 和其他端一样的相对位置），再启动后台同步；call 同 beayy_call
#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    #[no_mangle]
    pub extern "system" fn Java_com_fengli_beayy_Core_init(mut env: JNIEnv, _: JClass, home: JString) {
        if let Ok(h) = env.get_string(&home) {
            std::env::set_var("HOME", String::from(h));
        }
        super::start();
    }

    #[no_mangle]
    pub extern "system" fn Java_com_fengli_beayy_Core_call(mut env: JNIEnv, _: JClass, cmd: JString, args: JString) -> jstring {
        let cmd: String = env.get_string(&cmd).map(String::from).unwrap_or_default();
        let args: String = env.get_string(&args).map(String::from).unwrap_or_else(|_| "{}".into());
        let (c, a) = (std::ffi::CString::new(cmd).unwrap_or_default(), std::ffi::CString::new(args).unwrap_or_default());
        let out = unsafe {
            let p = super::beayy_call(c.as_ptr(), a.as_ptr());
            let s = std::ffi::CStr::from_ptr(p).to_string_lossy().to_string();
            super::beayy_free(p);
            s
        };
        env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }
}
