// Beayy（Be Authentic. Your Yesterdays.）Tauri 2 外壳。
// 菜单栏标志＋菜单、主窗口（网页四页，app/ui/）、给网页的命令（清单见 COMMANDS.md）、后台定时任务。
// 采集（键鼠、界面文字、在场）在 beayy-capture 里，由这里启动；macOS 和 Windows 记的事件相同。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod amap;
mod storage;
mod daily;
mod data;
mod fonts;
mod health;
mod icons;
mod mcp;
mod summarizer;
mod sync;
mod update;
#[cfg(target_os = "macos")]
mod devhook;

use beayy_core::{data_root, fields, local_device, shadow_mode, EventLog};
use beayy_phone::PhoneSync;
use beayy_remote::Relay;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

const PRODUCT: &str = "Be Authentic. Your Yesterdays.";
const PHONE_EVERY: Duration = Duration::from_secs(300);
/// 手表对话、iPhone 停留点：每 2 分钟一轮（账号同步的列表接口便宜，停留点早点到）
const RELAY_EVERY: Duration = Duration::from_secs(120);

struct AppState {
    log: Arc<EventLog>,
    phone: Mutex<PhoneSync>,
    relay: Mutex<Relay>,
    /// 录音和停留点走账号同步的上一轮结果（设置「连接」和「数据」页显示）
    cloud: Mutex<String>,
    paused: Arc<AtomicBool>,
    /// 定时暂停到这个时刻自动继续；None＝一直暂停（或没暂停）
    paused_until: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// 今天的条数：(本地日期, 当时数出的文件里条数, 当时 log.count())
    today_base: Mutex<Option<(chrono::NaiveDate, u64, u64)>>,
}

/// 今天本机写了多少条事件：当天第一次问时数一遍今天的分段文件，之后加上本次运行新写的
fn events_today(state: &AppState) -> u64 {
    use chrono::{Local, TimeZone, Utc};
    let today = Local::now().date_naive();
    let mut g = state.today_base.lock().unwrap();
    if g.map(|(d, _, _)| d != today).unwrap_or(true) {
        let start = Local.from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
        let first = beayy_core::segment_key(start);
        let mut n = 0u64;
        if let Ok(it) = std::fs::read_dir(state.log.segments_dir()) {
            for seg in it.filter_map(|e| e.ok()) {
                if seg.file_name().to_string_lossy().as_ref() >= first.as_str() {
                    n += std::fs::read(seg.path().join("events.jsonl")).map(|b| b.iter().filter(|c| **c == b'\n').count() as u64).unwrap_or(0);
                }
            }
        }
        *g = Some((today, n, state.log.count()));
    }
    let (_, base, at) = g.unwrap();
    base + state.log.count().saturating_sub(at)
}

#[tauri::command]
fn get_status(state: tauri::State<'_, Arc<AppState>>) -> Value {
    json!({
        "permissions": beayy_capture::permissions(),
        "capturing": capturing(),
        "paused": state.paused.load(Ordering::Relaxed),
        "paused_until": state.paused_until.lock().unwrap().map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        "events_today": events_today(&state),
        "login_item": login_item(),
        "phone_sync": state.phone.lock().unwrap().last_summary.clone(),
        "relay_sync": state.relay.lock().unwrap().last_summary.clone(),
        "cloud_sync": state.cloud.lock().unwrap().clone(),
        "device": state.log.device,
        "version": env!("CARGO_PKG_VERSION"),
        "shadow": shadow_mode(),
        // 本机 MCP 服务就是这个程序加 --mcp（设置里给 AI 接入的命令）
        "exe": std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
    })
}

fn capturing() -> bool {
    #[cfg(target_os = "macos")]
    return beayy_capture::macos::capturing();
    #[cfg(target_os = "windows")]
    return beayy_capture::windows::capturing();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return true;
}

/// 暂停／继续。`minutes` 给了就是定时暂停，到点自动继续
#[tauri::command]
fn set_paused(paused: bool, minutes: Option<i64>, state: tauri::State<'_, Arc<AppState>>) {
    *state.paused_until.lock().unwrap() = match (paused, minutes) {
        (true, Some(m)) if m > 0 => Some(chrono::Utc::now() + chrono::Duration::minutes(m)),
        _ => None,
    };
    apply_paused(&state, paused);
}

fn login_item() -> bool {
    #[cfg(target_os = "macos")]
    return beayy_capture::macos::login_item_enabled();
    #[cfg(target_os = "windows")]
    return beayy_capture::windows::login_item_enabled();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return false;
}

/// 退出 app（左下角菜单）
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// 开机自动启动
#[tauri::command]
fn set_login_item(on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return beayy_capture::macos::set_login_item(on);
    #[cfg(target_os = "windows")]
    return beayy_capture::windows::set_login_item(on);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return if on { Err("这个系统上还不支持".into()) } else { Ok(()) };
}

/// macOS、Windows 上由采集线程发现变化，先把正在打的字落盘再记 app.paused／app.resumed；别的平台这里直接记
fn apply_paused(state: &AppState, paused: bool) {
    if state.paused.swap(paused, Ordering::Relaxed) == paused {
        return;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    state.log.write(if paused { "app.paused" } else { "app.resumed" }, fields! {}, None);
}

/// 并跑版自检：页面把自己的状态写到影子目录 snapshots/<name>.txt（正式版不响应）
#[tauri::command]
fn dev_report(name: String, text: String) {
    if !shadow_mode() {
        return;
    }
    let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':')).collect();
    let dir = data_root().join("snapshots");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{name}.txt")), text);
}

#[tauri::command]
fn open_permission(kind: String) {
    // 屏幕使用时间（iCloud 跨设备共享）不是权限，但也在系统设置里：「怎么设置」弹层用
    if kind == "screentime" {
        let _ = std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.Screen-Time-Settings.extension").spawn();
        return;
    }
    beayy_capture::open_permission(&kind);
}

/// 「现在读取」：iPhone 屏幕使用时间不等 5 分钟，马上读一次
static PHONE_NOW: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn phone_read_now() {
    PHONE_NOW.store(true, Ordering::Relaxed);
}

#[tauri::command]
fn open_data_dir() {
    let _ = std::process::Command::new("/usr/bin/open").arg(data_root()).spawn();
}

fn open_main(app: &AppHandle) {
    if !shadow_mode() {
        update::check_soon();
    }
    // 主窗口开着时进 Dock、能 ⌘Tab；关掉后退回只在菜单栏（见 run 里的 Destroyed）
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title(if shadow_mode() { "Beayy 测试版" } else { PRODUCT })
        .inner_size(1180.0, 760.0)
        .min_inner_size(760.0, 520.0);
    // 墨线约定：标题栏并进侧栏顶部，系统窗口按钮叠在侧栏左上角（页面让出 72 像素）
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    let _ = builder.build();
}

fn permission_line() -> String {
    let p = beayy_capture::permissions();
    let mark = |b: bool| if b { "✓" } else { "✗" };
    format!(
        "辅助功能 {} · 输入监控 {} · 完全磁盘访问 {}",
        mark(p.accessibility),
        mark(p.input_monitoring),
        mark(p.full_disk_access)
    )
}

/// 菜单栏图标旁的「!」：缺权限或监听没装上
fn tray_ok() -> bool {
    let p = beayy_capture::permissions();
    p.accessibility && p.input_monitoring && p.full_disk_access && capturing()
}

/// 退出前把正在打的字落盘、记 app.quit。菜单「退出」和系统发来的退出（关机、osascript quit）都走 RunEvent::Exit
fn on_exit(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    beayy_capture::macos::shutdown();
    #[cfg(target_os = "windows")]
    beayy_capture::windows::shutdown();
    if let Some(s) = app.try_state::<Arc<AppState>>() {
        s.log.write("app.quit", fields! {}, None);
    }
}

/// 同一个数据目录只跑一个 app：两个一起采会把每条记录写两遍（10-17 更新重启时出过一次）。
/// 测试版写影子目录，锁也在影子目录，照样能和正式版同时开。
/// 更新重启时旧进程可能还没退完，拿不到锁先等一会儿（最多 10 秒），还拿不到才算真有一个在跑。
fn single_instance() -> Result<Option<std::fs::File>, ()> {
    let root = data_root();
    let _ = std::fs::create_dir_all(&root);
    // 锁文件建不了、系统不支持锁：不拦，照常启动（Ok(None)）
    let Ok(f) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join("app.lock")) else {
        return Ok(None);
    };
    for _ in 0..40 {
        match f.try_lock() {
            Ok(()) => return Ok(Some(f)),
            Err(std::fs::TryLockError::WouldBlock) => {
                // Windows 上点快捷方式（带 --open）时已经有一个在跑（比如开机自启的）：留个请求，让它打开主窗口
                #[cfg(target_os = "windows")]
                if std::env::args().any(|a| a == "--open") {
                    let _ = std::fs::write(root.join("open.request"), "");
                }
                std::thread::sleep(Duration::from_millis(250))
            }
            Err(_) => return Ok(None),
        }
    }
    Err(())
}

fn main() {
    // 调试用：beayy --dump list_days｜get_day YYYY-MM-DD｜get_settings｜get_rules，打印命令结果后退出（不启动界面）
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|a| a == "--summarize-once").unwrap_or(false) {
        // 调试用：立刻跑一次 10 分钟摘要调度（同定时任务，含锁文件），完成后退出
        summarizer::run_once(&EventLog::new(data_root(), local_device()));
        return;
    }
    if args.get(1).map(|a| a == "--mcp").unwrap_or(false) {
        // 本机 MCP 服务（标准输入输出），给本机 AI 读记录
        mcp::serve();
        return;
    }
    if let Some(code) = sync::cli(&args) {
        std::process::exit(code);
    }
    if args.get(1).map(|a| a == "--daily").unwrap_or(false) && args.get(2).map(|a| a == "catch-up").unwrap_or(false) {
        // 调试用：跑一遍定时的补写（最近 7 天的日总结、最近两周的周总结），直到没有要写的
        daily::catch_up_all();
        return;
    }
    if args.get(1).map(|a| a == "--weekly").unwrap_or(false) {
        // 调试用：写某天所在那周的周总结（默认上周）并打印
        let d = args.get(2).and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or_else(|| chrono::Local::now().date_naive() - chrono::Duration::days(7));
        match daily::generate_week(daily::monday_of(d)) {
            Ok(md) => println!("{md}"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if args.get(1).map(|a| a == "--monthly").unwrap_or(false) {
        // 调试用：写某天所在那个月的月总结（默认上个月）并打印
        let d = args.get(2).and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or_else(|| daily::first_of(chrono::Local::now().date_naive()) - chrono::Duration::days(1));
        match daily::generate_month(d) {
            Ok(md) => println!("{md}"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if args.get(1).map(|a| a == "--daily").unwrap_or(false) {
        // 调试用：写某天的日总结（默认今天）并打印
        let d = args.get(2).and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or_else(|| chrono::Local::now().date_naive());
        match daily::generate(d) {
            Ok(md) => println!("{md}"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if args.get(1).map(|a| a == "--summary-tag").unwrap_or(false) {
        // 一次性：给还没有「类型＋项目」标签的旧摘要补上，再生成缺的小时摘要
        let sm = beayy_summary::Summarizer::new(beayy_core::default_root());
        let n = sm.tag_backfill(|l| println!("{l}"));
        sm.update_aliases(|l| println!("{l}"));
        let m = sm.rollups(|l| println!("{l}"));
        println!("补标签 {n} 段，小时摘要 {m} 段");
        return;
    }
    if args.get(1).map(|a| a == "--retag-dry").unwrap_or(false) {
        // 调试用：按现在的类型说明把最近 N 天（默认 14）的 10 分钟摘要重新打一遍标签，每段一行 JSON 打印新旧对比，不写文件
        let days = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(14);
        let sm = beayy_summary::Summarizer::new(beayy_core::default_root());
        for (name, active, old, new) in sm.retag_dry(days, 6) {
            println!("{}", serde_json::json!({"name": name, "active_sec": active, "old": old, "new": new}));
        }
        return;
    }
    if args.get(1).map(|a| a == "--summary-redo-idle").unwrap_or(false) {
        // 一次性：已写过的「整段不在电脑前」摘要换成固定版（不调模型）
        let n = beayy_summary::Summarizer::new(beayy_core::default_root()).redo_idle();
        println!("换掉 {n} 段");
        return;
    }
    if args.get(1).map(|a| a == "--summary-try").unwrap_or(false) {
        // 调试用：用现在的提示词给分段问一次模型，打印结果和用量，不写文件
        let sm = beayy_summary::Summarizer::new(beayy_core::default_root());
        for name in &args[2..] {
            let out = sm.try_segment(name).map(|(j, u)| json!({"summary": j, "usage": u})).unwrap_or_else(|e| json!({"error": e}));
            println!("===== {name} =====\n{}\n", serde_json::to_string_pretty(&out).unwrap());
        }
        return;
    }
    if args.get(1).map(|a| a == "--summary-pack").unwrap_or(false) {
        // 调试用：打印某分段拼好的摘要包（不调模型）
        let sm = beayy_summary::Summarizer::new(beayy_core::default_root());
        for name in &args[2..] {
            println!("===== {name} =====\n{}\n", sm.dry_run(name));
        }
        return;
    }
    if args.get(1).map(|a| a == "--relay-pull").unwrap_or(false) {
        // 调试用：从录音服务器拉一轮手表对话和停留点
        let mut r = Relay::default();
        match r.pull(&data_root()) {
            Ok(n) => println!("拉了 {} 天，新增 {} 条（{}）", n.days, n.added, r.last_summary),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    // 调试和迁移：beayy --sync-put <设备> <块名> <文件>，把本地文件加密后放到同步服务；--sync-delete <设备> <块名>；beayy --ticket-file <文件>，把转写票据写进文件（1 小时有效，不打印）
    if args.get(1).map(|a| a == "--sync-put").unwrap_or(false) {
        let (d, n, f) = (args.get(2).cloned().unwrap_or_default(), args.get(3).cloned().unwrap_or_default(), args.get(4).cloned().unwrap_or_default());
        let r = std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| beayy_sync::put_named(&data_root(), &d, &n, &b));
        println!("{}", r.map(|_| "已传".to_string()).unwrap_or_else(|e| e));
        return;
    }
    if args.get(1).map(|a| a == "--sync-delete").unwrap_or(false) {
        let (d, n) = (args.get(2).cloned().unwrap_or_default(), args.get(3).cloned().unwrap_or_default());
        println!("{}", beayy_sync::delete_named(&data_root(), &d, &n).map(|_| "已删".to_string()).unwrap_or_else(|e| e));
        return;
    }
    if args.get(1).map(|a| a == "--ticket-file").unwrap_or(false) {
        let r = beayy_sync::ticket(&data_root()).and_then(|(t, _)| std::fs::write(args.get(2).cloned().unwrap_or_default(), t).map_err(|e| e.to_string()));
        println!("{}", r.map(|_| "已写".to_string()).unwrap_or_else(|e| e));
        return;
    }
    if args.get(1).map(|a| a == "--relay-import").unwrap_or(false) {
        // 调试用：beayy --relay-import transcripts|places <文件>，把本地 jsonl 当成服务器上的数据写进去
        let (kind, file) = (args.get(2).cloned().unwrap_or_default(), args.get(3).cloned().unwrap_or_default());
        match Relay::default().import_file(&data_root(), &kind, std::path::Path::new(&file)) {
            Ok(n) => println!("新增 {n} 条"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if args.get(1).map(|a| a == "--set-relay-token").unwrap_or(false) {
        // 从标准输入读录音服务器口令存进钥匙串（不经命令行参数，免得留在 shell 历史里）
        let mut t = String::new();
        let _ = std::io::stdin().read_line(&mut t);
        match beayy_core::secret_set(beayy_remote::SECRET, t.trim()) {
            Ok(()) => println!("已存"),
            Err(e) => eprintln!("{e}"),
        }
        return;
    }
    if args.get(1).map(|a| a == "--dump").unwrap_or(false) {
        let out = match args.get(2).map(String::as_str) {
            Some("list_days") => json!(data::list_days()),
            Some("get_day") => data::get_day(args.get(3).cloned().unwrap_or_default()),
            Some("get_settings") => data::get_settings(),
            Some("get_rules") => data::get_rules(),
            Some("list_devices") => json!(data::list_devices()),
            Some("list_projects") => json!(data::list_projects()),
            Some("get_profile") => data::get_profile(),
            Some("recent_events") => data::recent_events(args.get(3).cloned().unwrap_or_default(), None, Some(5), None).unwrap_or_else(|e| json!({ "error": e })),
            Some("place_stats") => data::place_stats(None),
            Some("get_range") => data::get_range(args.get(3).cloned().unwrap_or_default(), args.get(4).cloned().unwrap_or_default()).unwrap_or_else(|e| json!({ "error": e })),
            Some("get_week") => daily::get_week(args.get(3).cloned().unwrap_or_default()).unwrap_or_else(|e| json!({ "error": e })),
            Some("get_month") => daily::get_month(args.get(3).cloned().unwrap_or_default()).unwrap_or_else(|e| json!({ "error": e })),
            Some("llm_usage") => data::llm_usage(),
            Some("cloud_round") => match beayy_remote::cloud::round(&mut Relay::default(), &data_root(), Duration::from_secs(600)) {
                Ok(n) => json!({"added": n.added, "places": n.places, "health": n.health, "transcribed": n.transcribed, "fetched": n.fetched, "deleted": n.deleted, "waiting": n.waiting, "errors": n.errors}),
                Err(e) => json!({"error": e}),
            },
            Some("storage_stats") => tauri::async_runtime::block_on(storage::storage_stats()),
            Some("test_model") => json!(tauri::async_runtime::block_on(data::test_model()).map(|_| "通了".to_string()).unwrap_or_else(|e| e)),
            Some("map_around") => amap::amap_around(args.get(3).and_then(|x| x.parse().ok()).unwrap_or(32.0475), args.get(4).and_then(|x| x.parse().ok()).unwrap_or(118.9215), Some(120)).map(|v| json!(v.iter().take(6).map(|x| json!([x["name"], x["type"], x["distance"]])).collect::<Vec<_>>())).unwrap_or_else(|e| json!({ "error": e })),
            Some("map_admin") => json!(amap::admin(32.0475, 118.9215, 20.0, true)),
            Some("amap_search") => amap::amap_search(args.get(3).cloned().unwrap_or_default(), Some(32.0475), Some(118.9215)).map(|v| json!(v.len())).unwrap_or_else(|e| json!({ "error": e })),
            Some("amap_js") => tauri::async_runtime::block_on(amap::amap_js_config()).as_object().map(|o| json!(o.keys().collect::<Vec<_>>())).unwrap_or(json!(null)),
            Some("sync_usage") => beayy_sync::usage(&beayy_core::data_root()).unwrap_or_else(|e| json!({ "error": e })),
            Some("ticket") => beayy_sync::ticket(&beayy_core::data_root()).map(|(t, exp)| json!({ "len": t.len(), "expires": exp })).unwrap_or_else(|e| json!({ "error": e })),
            _ => json!({"error": "用法：--dump list_days|get_day 日期|get_range 起 止|get_week 日期|get_month 日期|llm_usage|get_settings|get_rules|place_stats"}),
        };
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return;
    }
    // 锁要活到进程结束（run 不返回），进程退出时系统自动放掉
    let Ok(_instance) = single_instance() else {
        eprintln!("Beayy 已经在运行，这个就不开了");
        return;
    };
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_status, phone_read_now, set_paused, set_login_item, quit_app, open_permission, open_data_dir, dev_report,
            data::list_days, data::get_day, data::get_rules, data::save_rules,
            data::get_settings, data::save_settings, storage::storage_stats, data::llm_status, data::set_api_key, data::set_relay_token, data::delete_since, data::list_devices, data::rename_device, data::set_place_label, data::recent_events, data::set_place_pick, data::clear_place_pick, data::list_place_labels, data::place_stats, data::edit_place_label, amap::set_amap_key, amap::amap_around, amap::amap_search, amap::amap_map, amap::amap_js_config, amap::set_amap_js, icons::get_icons, daily::generate_daily, daily::daily_status, daily::get_week, daily::generate_weekly, daily::get_month, daily::generate_monthly, data::get_range, data::llm_usage, data::test_model, fonts::list_fonts,
            data::get_profile, data::save_profile, data::list_projects, data::merge_project, data::set_project_domain,
            sync::sync_status, sync::sync_now,
            sync::account_signup, sync::account_login, sync::account_claim, sync::account_recover, sync::account_password,
            sync::account_info, sync::account_remove_device, sync::account_logout, sync::account_recovery_code, sync::signup_info, sync::folder_state, sync::folder_create, sync::folder_join, sync::folder_recover, sync::pick_folder,
            update::update_status, update::update_check, update::update_apply
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let root = data_root();
            let state = Arc::new(AppState { log: Arc::new(EventLog::new(root.clone(), local_device())), phone: Mutex::new(PhoneSync::default()), relay: Mutex::new(Relay::default()), cloud: Mutex::new(String::new()), paused: Arc::new(AtomicBool::new(false)), paused_until: Mutex::new(None), today_base: Mutex::new(None) });
            #[allow(unused_mut)]
            let mut launch = fields! {
                "version" => env!("CARGO_PKG_VERSION"),
                "impl" => "rust",
                "permissions" => beayy_capture::permissions(),
                "bundle_path" => std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
                "shadow" => shadow_mode(),
            };
            #[cfg(target_os = "macos")]
            {
                if let Some(b) = beayy_capture::macos::bundle_path() {
                    launch.insert("bundle_path".into(), json!(b));
                }
                launch.extend(beayy_capture::macos::launch_fields());
            }
            state.log.write("app.launch", launch, None);
            app.manage(state.clone());

            // 采集：键鼠、打字与发送、界面文字、切 app、在场（macOS 在主线程调，系统通知要挂在主线程上；Windows 自己起消息线程）
            #[cfg(target_os = "macos")]
            beayy_capture::macos::start(state.log.clone(), state.paused.clone());
            #[cfg(target_os = "windows")]
            beayy_capture::windows::start(state.log.clone(), state.paused.clone());

            // 10 分钟摘要：每个整 10 分过后 1 分钟调起 Python 脚本（并跑版也跑，脚本跳过已写过的分段，锁文件防重入）
            summarizer::start(state.log.clone(), state.paused.clone());
            // 日总结：每天 21:30 写当天，次日 0:15 重写前一天
            daily::start(state.paused.clone());
            // 多设备同步：每 5 分钟一轮（没开同步就什么都不做；测试版不同步）
            sync::start(state.log.clone());
            // 自动更新：启动 1 分钟后查，之后每小时、打开主窗口时也查；下好了由用户点「重启更新」（测试版不查）
            if !shadow_mode() {
                update::start();
            }

            // 菜单栏：标志模板图（系统按深浅色菜单栏自动反色）＋菜单
            let title = MenuItem::with_id(app, "title", if shadow_mode() { "Beayy 测试版（数据写影子目录，不算正式记录）".to_string() } else { PRODUCT.into() }, false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "打开 Beayy", true, None::<&str>)?;
            let perm = MenuItem::with_id(app, "perm", permission_line(), true, None::<&str>)?;
            let tap = MenuItem::with_id(app, "tap", "键鼠监听：启动中", false, None::<&str>)?;
            let phone = MenuItem::with_id(app, "phone", "iPhone：读取中", false, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "暂停记录", true, None::<&str>)?;
            let data = MenuItem::with_id(app, "data", "打开数据目录", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "开机自动启动", cfg!(any(target_os = "macos", target_os = "windows")), login_item(), None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&title, &open, &sep1, &perm, &tap, &phone, &sep2, &pause, &data, &login, &sep3, &quit_item])?;
            let icon = Image::from_bytes(include_bytes!("../../Resources/MenuBarIcon@2x.png"))?;
            TrayIconBuilder::with_id("main")
                .icon(icon)
                .icon_as_template(true)
                .tooltip(if shadow_mode() { "Beayy 测试版" } else { PRODUCT })
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, e| match e.id.as_ref() {
                    "open" => open_main(app),
                    "perm" => {
                        let p = beayy_capture::permissions();
                        let kind = if !p.accessibility {
                            "accessibility"
                        } else if !p.input_monitoring {
                            "input_monitoring"
                        } else {
                            "full_disk_access"
                        };
                        beayy_capture::open_permission(kind);
                    }
                    "data" => open_data_dir(),
                    "pause" => {
                        if let Some(s) = app.try_state::<Arc<AppState>>() {
                            let p = !s.paused.load(Ordering::Relaxed);
                            *s.paused_until.lock().unwrap() = None;
                            apply_paused(&s, p);
                        }
                    }
                    "login" => {
                        if let Err(err) = set_login_item(!login_item()) {
                            if let Some(s) = app.try_state::<Arc<AppState>>() {
                                s.log.write("login_item.error", fields! {"error" => err}, None);
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // 后台：iPhone 每 5 分钟同步一次（完全磁盘访问刚给上时立刻同步）；菜单上的状态每 5 秒刷新
            let st = state.clone();
            let phone_item = phone.clone();
            let phone_now = Arc::new(AtomicBool::new(false));
            let phone_now2 = phone_now.clone();
            std::thread::spawn(move || loop {
                let notes = st.phone.lock().unwrap().sync(&root);
                for n in notes.iter().filter(|n| n.added > 0) {
                    st.log.write(
                        "phone.sync",
                        fields! {"phone_device" => n.device, "added" => n.added, "total" => n.total, "latest" => n.latest, "source_uuid" => n.uuid},
                        None,
                    );
                }
                let _ = phone_item.set_text(format!("iPhone：{}", st.phone.lock().unwrap().last_summary));
                let mut waited = Duration::ZERO;
                while waited < PHONE_EVERY && !phone_now2.swap(false, Ordering::Relaxed) && !PHONE_NOW.swap(false, Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(1));
                    waited += Duration::from_secs(1);
                }
            });
            // 后台：手表对话、iPhone 停留点每 2 分钟一轮（走账号同步；没登录就什么都不做）。旧录音服务器（v1）10-29 下掉，不再拉
            let st = state.clone();
            let relay_root = data_root();
            std::thread::spawn(move || {
                // 走账号同步的录音和停留点：自己一个写入器
                let mut cloud = Relay::default();
                loop {
                    match beayy_remote::cloud::round(&mut cloud, &relay_root, Duration::from_secs(8 * 60)) {
                        Ok(n) => {
                            if n.added > 0 || !n.errors.is_empty() {
                                st.log.write("cloud.sync", fields! {"added" => n.added, "transcribed" => n.transcribed, "errors" => n.errors.join("；")}, None);
                            }
                            *st.cloud.lock().unwrap() = n.summary();
                        }
                        Err(e) => *st.cloud.lock().unwrap() = e,
                    }
                    std::thread::sleep(RELAY_EVERY);
                }
            });
            let st = state.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut fda = beayy_capture::permissions().full_disk_access;
                loop {
                    let p = beayy_capture::permissions();
                    if p.full_disk_access && !fda {
                        phone_now.store(true, Ordering::Relaxed);
                    }
                    fda = p.full_disk_access;
                    let _ = perm.set_text(permission_line());
                    let _ = tap.set_text(if capturing() { "键鼠监听：运行中" } else { "键鼠监听：未启动" });
                    // 定时暂停到点：自动继续
                    let due = st.paused_until.lock().unwrap().map(|t| t <= chrono::Utc::now()).unwrap_or(false);
                    if due {
                        *st.paused_until.lock().unwrap() = None;
                        apply_paused(&st, false);
                    }
                    let paused = st.paused.load(Ordering::Relaxed);
                    let _ = pause.set_text(if paused { "继续记录" } else { "暂停记录" });
                    let _ = login.set_checked(login_item());
                    if let Some(tray) = handle.tray_by_id("main") {
                        let _ = tray.set_title(if tray_ok() { None } else { Some("!") });
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            });
            // 并跑版另开自检钩子（见 devhook.rs）
            // Windows 没有 Reopen：另一个进程留下 open.request（见 single_instance）就打开主窗口
            #[cfg(target_os = "windows")]
            {
                let req = data_root().join("open.request");
                let _ = std::fs::remove_file(&req);
                let handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(500));
                    if std::fs::remove_file(&req).is_ok() {
                        let h = handle.clone();
                        let _ = handle.run_on_main_thread(move || open_main(&h));
                    }
                });
            }
            // 带 --open 启动、或还没做过首次引导：直接打开主窗口
            if std::env::args().any(|a| a == "--open") || data::needs_onboarding() {
                open_main(app.handle());
            }
            #[cfg(target_os = "macos")]
            if shadow_mode() {
                devhook::start(app.handle().clone(), data_root().join("snapshots"), open_main);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("启动 Beayy 失败")
        .run(|_app, event| match event {
            RunEvent::Exit => on_exit(_app),
            // 关掉主窗口不退出，菜单栏常驻
            RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
            // 已在运行时再 open 一次（open -a Beayy-rs）：打开主窗口
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => open_main(_app),
            #[cfg(target_os = "macos")]
            // 要在窗口真正关掉之前切：Regular 状态下关掉最后一个窗口，系统会把整个 app 退掉（菜单栏也没了）
            RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { .. }, .. } if label == "main" => {
                let _ = _app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            _ => {}
        });
}
