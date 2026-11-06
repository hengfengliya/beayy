//! 多设备同步的调度和给网页的命令。同步本身在 beayy-sync（方案见 报告/方案_20261016_多设备同步.md）。
//! 每 5 分钟一轮；测试版（--shadow）不同步，免得把影子目录传上去。

use beayy_core::{data_root, fields, local_device, shadow_mode, EventLog};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(300);

static RUNNING: AtomicBool = AtomicBool::new(false);
static NOW: OnceLock<Mutex<bool>> = OnceLock::new();

/// 跑一轮（同一时间只跑一轮），记一条 sync.run
fn run(log: Option<&EventLog>) -> Option<beayy_sync::Report> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return None;
    }
    let t = std::time::Instant::now();
    // 手机版只下载：它不采集电脑那种分段，自己算出的东西也不能当成某台设备的分段传上去
    #[cfg(target_os = "ios")]
    let r = beayy_sync::sync_download_only(&data_root(), &local_device());
    #[cfg(not(target_os = "ios"))]
    let r = beayy_sync::sync_once(&data_root(), &local_device());
    RUNNING.store(false, Ordering::SeqCst);
    if let (Some(r), Some(log)) = (&r, log) {
        if r.uploaded + r.downloaded > 0 || !r.errors.is_empty() {
            log.write(
                "sync.run",
                fields! {"uploaded" => r.uploaded, "downloaded" => r.downloaded, "errors" => r.errors.len(), "seconds" => t.elapsed().as_secs()},
                None,
            );
        }
    }
    r
}

pub fn start(log: Arc<EventLog>) {
    if shadow_mode() {
        return;
    }
    std::thread::spawn(move || loop {
        run(Some(&log));
        pull_profile();
        // 等 5 分钟，期间页面点了「立即同步」就提前跑
        let mut waited = Duration::ZERO;
        while waited < EVERY {
            std::thread::sleep(Duration::from_secs(1));
            waited += Duration::from_secs(1);
            let flag = NOW.get_or_init(|| Mutex::new(false));
            if std::mem::take(&mut *flag.lock().unwrap()) {
                break;
            }
        }
    });
}

/// 头像和名称：账号上的比本机新（在别的设备上改过）就拉下来；本机的更新（登录账号以前设的、
/// 当时只存在本机）就推上去，别的设备下一轮就拿到
fn pull_profile() {
    let root = data_root();
    if beayy_sync::service(&root).and_then(|a| a.email).is_none() {
        return;
    }
    let v = if beayy_sync::load(&root).is_some() { beayy_sync::account(&root) } else { beayy_sync::service_account(&root) };
    if let Ok(v) = v {
        let p = &v["profile"];
        let updated = p["updated"].as_i64().unwrap_or(0);
        let mine = crate::data::profile_updated();
        if updated > mine {
            let _ = crate::data::write_profile(p["name"].as_str().unwrap_or(""), p["avatar"].as_str(), updated);
        } else if mine > updated {
            let s = crate::data::read_settings_raw();
            let name = s.get("profile_name").and_then(Value::as_str).unwrap_or("").to_string();
            let avatar = s.get("profile_avatar").and_then(Value::as_str).map(String::from);
            if name.is_empty() && avatar.is_none() {
                return;
            }
            if let Ok(at) = beayy_sync::set_profile(&root, &name, avatar.as_deref()) {
                if at > 0 {
                    let _ = crate::data::write_profile(&name, avatar.as_deref(), at);
                }
            }
        }
    }
}

#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn sync_status() -> Value {
    let mut v = beayy_sync::status(&data_root());
    v["running"] = json!(RUNNING.load(Ordering::SeqCst));
    v["shadow"] = json!(shadow_mode());
    v
}



/// 立即同步一轮，返回结果
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn sync_now() -> Result<Value, String> {
    match run(None) {
        Some(r) => Ok(json!({"uploaded": r.uploaded, "downloaded": r.downloaded, "errors": r.errors})),
        None => Err("没开同步，或者正在同步".into()),
    }
}



// ---------- 账号（邮箱＋主密码） ----------

/// 这台设备在账号里显示的名字
fn device_name() -> String {
    let d = local_device();
    match d.split_once('-') {
        Some(("mac", h)) => format!("Mac · {h}"),
        Some(("win", h)) => format!("Windows · {h}"),
        Some(("iosapp", _)) => "iPhone（Beayy）".into(),
        Some(("android", _)) => "Android（Beayy）".into(),
        _ => d,
    }
}

fn kick() {
    *NOW.get_or_init(|| Mutex::new(false)).lock().unwrap() = true;
}

/// 注册、登录用的同步服务器：设置里的 sync_server（自建的），没填用官方的
pub fn server() -> String {
    crate::data::read_settings_raw().get("sync_server").and_then(|v| v.as_str()).map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| beayy_sync::DEFAULT_SERVER.to_string())
}

/// 设置里选的同步方式：server（官方或自建服务器，默认）、folder（同步文件夹，设置里的 sync_folder）、webdav、icloud。
/// 只记页面上选的是哪个；真正在用的看 sync_status 的 mode（账号里存的）
pub fn mode() -> String {
    crate::data::read_settings_raw().get("sync_mode").and_then(|v| v.as_str()).filter(|s| ["folder", "webdav", "icloud"].contains(s)).unwrap_or("server").to_string()
}

pub fn folder() -> String {
    crate::data::read_settings_raw().get("sync_folder").and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// 注册框用：这台服务器要不要邀请码、每个账号能存多少
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn signup_info() -> Value {
    beayy_sync::signup_info(&server())
}

/// 设置里选的同步文件夹：路径、在不在、里面开没开过同步（账号页据此给「新开」还是「加入」）
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn folder_state() -> Value {
    beayy_sync::folder_state(&folder())
}

/// 第一台设备在同步文件夹里新开同步，返回恢复码
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn folder_create(password: String) -> Result<String, String> {
    let code = beayy_sync::folder_create(&data_root(), &folder(), &password)?;
    kick();
    Ok(code)
}

/// 别的设备加入同步文件夹里已有的同步
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn folder_join(password: String) -> Result<(), String> {
    beayy_sync::folder_join(&data_root(), &folder(), &password)?;
    kick();
    Ok(())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn folder_recover(code: String, password: String) -> Result<(), String> {
    beayy_sync::folder_recover(&data_root(), &folder(), &code, &password)?;
    kick();
    Ok(())
}

// ---------- WebDAV（坚果云、NAS、Nextcloud）：地址、用户名、密码只存进账号（钥匙串），不进设置 ----------

/// 连不连得上、里面开没开过同步：{ok, vault, error?}
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn webdav_state(url: String, user: String, pass: String) -> Value {
    beayy_sync::webdav_state(&url, &user, &pass)
}

/// 第一台设备在 WebDAV 里新开同步，返回恢复码
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn webdav_create(url: String, user: String, pass: String, password: String) -> Result<String, String> {
    let code = beayy_sync::webdav_create(&data_root(), &url, &user, &pass, &password)?;
    kick();
    Ok(code)
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn webdav_join(url: String, user: String, pass: String, password: String) -> Result<(), String> {
    beayy_sync::webdav_join(&data_root(), &url, &user, &pass, &password)?;
    kick();
    Ok(())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn webdav_recover(url: String, user: String, pass: String, code: String, password: String) -> Result<(), String> {
    beayy_sync::webdav_recover(&data_root(), &url, &user, &pass, &code, &password)?;
    kick();
    Ok(())
}

// ---------- iCloud（Mac＋iPhone，不要 Beayy 账号）：iCloud 容器里的同步文件夹，见 beayy_sync::icloud_dir ----------

/// 这台设备能不能用 iCloud、里面开没开过同步：{available, path?, vault, error?}
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn icloud_state() -> Value {
    beayy_sync::icloud_state()
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn icloud_create(password: String) -> Result<String, String> {
    let code = beayy_sync::icloud_create(&data_root(), &password)?;
    kick();
    Ok(code)
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn icloud_join(password: String) -> Result<(), String> {
    beayy_sync::icloud_join(&data_root(), &password)?;
    kick();
    Ok(())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn icloud_recover(code: String, password: String) -> Result<(), String> {
    beayy_sync::icloud_recover(&data_root(), &code, &password)?;
    kick();
    Ok(())
}

/// 选一个文件夹（系统的选文件夹窗口）：macOS 用 AppleScript，Windows 用 PowerShell；取消返回 None
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn pick_folder() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    let o = std::process::Command::new("/usr/bin/osascript").args(["-e", &format!("POSIX path of (choose folder with prompt \"{}\")", beayy_core::i18n::tr("选一个同步文件夹"))]).output();
    #[cfg(target_os = "windows")]
    let o = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"])
        .output();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let o: std::io::Result<std::process::Output> = Err(std::io::Error::other("这个系统没有选文件夹窗口，直接填路径"));
    let o = o.map_err(|e| e.to_string())?;
    let p = String::from_utf8_lossy(&o.stdout).trim().trim_end_matches('/').to_string();
    Ok(Some(p).filter(|p| o.status.success() && !p.is_empty()))
}

/// 注册新账号，返回恢复码
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_signup(invite: String, email: String, password: String) -> Result<String, String> {
    let code = beayy_sync::signup(&data_root(), &server(), invite.trim(), &email, &password, &local_device(), &device_name())?;
    kick();
    Ok(code)
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_login(email: String, password: String) -> Result<(), String> {
    beayy_sync::login(&data_root(), &server(), &email, &password, &local_device(), &device_name())?;
    kick();
    Ok(())
}

/// 老账号（配对码时期）设邮箱和主密码，返回恢复码
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_claim(email: String, password: String) -> Result<String, String> {
    beayy_sync::claim(&data_root(), &email, &password, &local_device(), &device_name())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_recover(email: String, code: String, password: String) -> Result<(), String> {
    beayy_sync::recover(&data_root(), &server(), &email, &code, &password, &local_device(), &device_name())?;
    kick();
    Ok(())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_password(password: String) -> Result<(), String> {
    beayy_sync::change_password(&data_root(), &password)
}

/// 账号信息：邮箱、登录着的设备（标出哪台是这台）
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_info() -> Result<Value, String> {
    beayy_sync::account(&data_root())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_remove_device(id: String) -> Result<(), String> {
    beayy_sync::remove_session(&data_root(), &id)
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_logout() {
    beayy_sync::logout(&data_root());
}

// ---------- 只为官方服务（模型、转写、地图）登的账号：同步用 iCloud、网盘、文件夹或不同步时 ----------

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_service_login(email: String, password: String) -> Result<(), String> {
    beayy_sync::service_login(&data_root(), &server(), &email, &password, &local_device(), &device_name())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_service_signup(invite: String, email: String, password: String) -> Result<String, String> {
    beayy_sync::service_signup(&data_root(), &server(), invite.trim(), &email, &password, &local_device(), &device_name())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_service_recover(email: String, code: String, password: String) -> Result<(), String> {
    beayy_sync::service_recover(&data_root(), &server(), &email, &code, &password, &local_device(), &device_name())
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_service_logout() {
    beayy_sync::service_logout(&data_root());
}

/// 停掉 Beayy 服务同步、登录留给官方服务（换同步方式时用）；没在用 Beayy 服务同步就照常退出
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn account_stop_sync() {
    let root = data_root();
    if beayy_sync::keep_for_service(&root).is_err() {
        beayy_sync::logout(&root);
    }
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn account_recovery_code() -> Option<String> {
    beayy_sync::recovery_code(&data_root())
}

/// 命令行：beayy --account-signup <邀请码> <邮箱> <主密码>｜--account-login <邮箱> <主密码>｜--account-claim｜--account-recover｜--account-password｜--account-info｜--account-remove <id>｜--account-logout｜--sync-once｜--sync-status。返回退出码
pub fn cli(args: &[String]) -> Option<i32> {
    let root = data_root();
    let out = match args.get(1).map(String::as_str) {
        Some("--account-signup") => beayy_sync::signup(&root, &server(), args.get(2)?, args.get(3)?, args.get(4)?, &local_device(), &device_name()).map(|c| format!("已注册，恢复码 {c}")),
        Some("--account-login") => beayy_sync::login(&root, &server(), args.get(2)?, args.get(3)?, &local_device(), &device_name()).map(|_| "已登录".into()),
        Some("--account-claim") => beayy_sync::claim(&root, args.get(2)?, args.get(3)?, &local_device(), &device_name()).map(|c| format!("已设好邮箱和主密码，恢复码 {c}")),
        Some("--account-recover") => beayy_sync::recover(&root, &server(), args.get(2)?, args.get(3)?, args.get(4)?, &local_device(), &device_name()).map(|_| "已用恢复码重设并登录".into()),
        Some("--account-password") => beayy_sync::change_password(&root, args.get(2)?).map(|_| "已改主密码".into()),
        Some("--account-info") => beayy_sync::account(&root).map(|v| serde_json::to_string_pretty(&v).unwrap()),
        Some("--account-remove") => beayy_sync::remove_session(&root, args.get(2)?).map(|_| "已移除".into()),
        Some("--account-logout") => {
            beayy_sync::logout(&root);
            Ok("已退出".into())
        }
        Some("--sync-status") => Ok(serde_json::to_string_pretty(&beayy_sync::status(&root)).unwrap()),
        Some("--sync-once") => {
            let t = std::time::Instant::now();
            match beayy_sync::sync_once(&root, &local_device()) {
                Some(r) => Ok(format!("上传 {}，下载 {}，{:.1} 秒{}", r.uploaded, r.downloaded, t.elapsed().as_secs_f64(), if r.errors.is_empty() { String::new() } else { format!("；出错：{}", r.errors.join("；")) })),
                None => Err("没开同步".into()),
            }
        }
        _ => return None,
    };
    match out {
        Ok(s) => {
            println!("{s}");
            Some(0)
        }
        Err(e) => {
            eprintln!("{e}");
            Some(1)
        }
    }
}
