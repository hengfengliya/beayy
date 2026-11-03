//! 自动更新：查 sync.beayy.com/update/latest.json，有新版本就在后台下载、校验 sha256，下好后由用户点「重启更新」。
//! 更新包放在自己的同步服务上（hk2 的 data/sync/update/，公开只读），不放 GitHub：国内不翻墙下 GitHub 很慢。
//!
//! latest.json：{"version": "0.2.1", "notes": "…", "mac": {"file": "Beayy-0.2.1.dmg", "sha256": "…"}, "win": {"file": "…zip", "sha256": "…"}}
//! - macOS：挂载 DMG，校验里面的 Beayy.app 是团队 8R7XUVNM53 的 Developer ID 签名，换掉正在用的 .app，重新打开。
//!   本机自己编译、用开发证书签的版本（凤梨的开发机）不自动更新：换成 Developer ID 签名会掉系统授权，那台走 build.sh。
//! - Windows：解压 zip，把正在运行的 beayy.exe 改名成 beayy.old.exe（Windows 允许改名运行中的程序），新的放回原位，重新打开；
//!   下次启动删掉 beayy.old.exe。
//! 下载放在 Beayy/update/。启动 1 分钟后查一次，之后每小时查一次；打开主窗口时也查（10 分钟内查过就不再查）。

use beayy_core::default_root;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;

const BASE: &str = "https://sync.beayy.com/update/";
#[cfg(target_os = "macos")]
const TEAM: &str = "8R7XUVNM53";

#[derive(Default)]
struct State {
    checking: bool,
    latest: Option<Value>,
    /// 下好、校验过的安装包
    ready: Option<PathBuf>,
    error: Option<String>,
    checked: Option<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = STATE.lock().unwrap();
    f(g.get_or_insert_with(State::default))
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split('.').map(|x| x.parse().unwrap_or(0)).collect()
}

fn newer(a: &str, b: &str) -> bool {
    parse(a) > parse(b)
}

fn platform() -> &'static str {
    if cfg!(target_os = "windows") { "win" } else { "mac" }
}

fn agent(secs: u64) -> ureq::Agent {
    beayy_sync::http_agent(ureq::Agent::config_builder()
        .proxy(None)
        .tls_config(beayy_core::tls!())
        .timeout_global(Some(Duration::from_secs(secs)))
        .build())
}

fn dir() -> PathBuf {
    default_root().join("update")
}

/// 这份 app 能不能自己更新：macOS 上要是 Developer ID 签名的正式包（开发机上的开发证书版不换）
fn updatable() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let bundle = beayy_capture::macos::bundle_path().ok_or("直接运行的程序，不是装好的 app")?;
        let out = std::process::Command::new("/usr/bin/codesign").args(["-dvv", &bundle]).output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stderr);
        if !text.contains("Authority=Developer ID Application") {
            return Err("这台装的是自己编译的开发版，不自动更新".into());
        }
    }
    Ok(())
}

fn sha256_file(p: &Path) -> Result<String, String> {
    let mut f = fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// 查一次；有新版本就下载。返回状态
pub fn check() -> Value {
    if with(|s| std::mem::replace(&mut s.checking, true)) {
        return status();
    }
    let r = check_inner();
    with(|s| {
        s.checking = false;
        s.checked = Some(chrono::Utc::now().to_rfc3339());
        s.error = r.err();
    });
    status()
}

fn check_inner() -> Result<(), String> {
    let latest: Value = agent(60).get(&format!("{BASE}latest.json")).call().map_err(|e| format!("连不上更新服务器：{e}"))?.body_mut().read_json().map_err(|e| e.to_string())?;
    let ver = latest["version"].as_str().unwrap_or("").to_string();
    with(|s| s.latest = Some(latest.clone()));
    if !newer(&ver, current()) {
        return Ok(());
    }
    updatable()?;
    let item = &latest[platform()];
    // 这个系统的包还没传上来（比如 Mac 的还在等苹果公证）：先当没有新版本
    let (Some(file), Some(sum)) = (item["file"].as_str(), item["sha256"].as_str()) else { return Ok(()) };
    if file.contains('/') || file.contains('\\') || file.starts_with('.') {
        return Err("安装包名字不对".into());
    }
    let d = dir();
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let dest = d.join(file);
    if dest.is_file() && sha256_file(&dest)? == sum {
        with(|s| s.ready = Some(dest));
        return Ok(());
    }
    let part = d.join(format!("{file}.part"));
    let mut resp = agent(30 * 60).get(&format!("{BASE}{file}")).call().map_err(|e| format!("下载失败：{e}"))?;
    let mut reader = resp.body_mut().with_config().limit(500 * 1024 * 1024).reader();
    let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
    std::io::copy(&mut reader, &mut out).map_err(|e| format!("下载中断：{e}"))?;
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    if sha256_file(&part)? != sum {
        let _ = fs::remove_file(&part);
        return Err("下载的安装包校验不对，已删掉，下次再下".into());
    }
    fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    // 只留这一份
    if let Ok(it) = fs::read_dir(&d) {
        for e in it.flatten() {
            if e.path() != dest {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    with(|s| s.ready = Some(dest));
    Ok(())
}

pub fn status() -> Value {
    with(|s| {
        let latest = s.latest.as_ref().and_then(|l| l["version"].as_str()).map(String::from);
        json!({
            "current": current(),
            "latest": latest,
            "newer": latest.as_deref().map(|v| newer(v, current())).unwrap_or(false) && s.latest.as_ref().map(|l| l[platform()]["file"].is_string()).unwrap_or(false),
            "notes": s.latest.as_ref().and_then(|l| l["notes"].as_str()),
            "ready": s.ready.is_some(),
            "checking": s.checking,
            "checked": s.checked,
            "error": s.error,
            "updatable": updatable().err(),
        })
    })
}

/// 上次查是 10 分钟以前（或还没查过）
fn due() -> bool {
    with(|s| s.checked.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()))
        .map(|t| chrono::Utc::now().signed_duration_since(t) > chrono::Duration::minutes(10))
        .unwrap_or(true)
}

/// 启动 1 分钟后查一次，之后每小时
pub fn start() {
    #[cfg(target_os = "windows")]
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(exe.with_file_name("beayy.old.exe"));
    }
    #[cfg(target_os = "windows")]
    beayy_install::sync_installed_version(env!("CARGO_PKG_VERSION"));
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            if due() {
                check();
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

/// 打开主窗口时在后台查一次（10 分钟内查过就不查）
pub fn check_soon() {
    if due() {
        std::thread::spawn(check);
    }
}

#[tauri::command(async)]
pub fn update_status() -> Value {
    status()
}

#[tauri::command]
pub async fn update_check() -> Value {
    tauri::async_runtime::spawn_blocking(check).await.unwrap_or_else(|_| status())
}

/// 装上下好的新版本并重启
#[tauri::command]
pub async fn update_apply(app: AppHandle) -> Result<(), String> {
    // 点两下只装一次：装两次会开出两个新版本（每个都 open -n）；没装成就放开，可以再点
    static APPLYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if APPLYING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("正在更新，请稍等".into());
    }
    let r: Result<(), String> = async {
        let file = with(|s| s.ready.clone()).ok_or("还没有下好的新版本")?;
        updatable()?;
        tauri::async_runtime::spawn_blocking(move || install(&file)).await.map_err(|e| e.to_string())?
    }
    .await;
    if r.is_err() {
        APPLYING.store(false, std::sync::atomic::Ordering::SeqCst);
        return r;
    }
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "macos")]
fn install(dmg: &Path) -> Result<(), String> {
    use std::process::Command;
    let run = |cmd: &str, args: &[&str]| -> Result<String, String> {
        let o = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
        if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err(format!("{cmd} 失败：{}", String::from_utf8_lossy(&o.stderr).trim())) }
    };
    let bundle = PathBuf::from(beayy_capture::macos::bundle_path().ok_or("找不到正在用的 app")?);
    let mnt = dir().join("mnt");
    let _ = fs::create_dir_all(&mnt);
    let mnt_s = mnt.to_string_lossy().to_string();
    run("/usr/bin/hdiutil", &["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", &mnt_s, &dmg.to_string_lossy()])?;
    let result = (|| -> Result<(), String> {
        let src = mnt.join("Beayy.app");
        let src_s = src.to_string_lossy().to_string();
        run("/usr/bin/codesign", &["--verify", "--deep", "--strict", &src_s])?;
        let info = Command::new("/usr/bin/codesign").args(["-dvv", &src_s]).output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&info.stderr);
        if !text.contains(&format!("TeamIdentifier={TEAM}")) || !text.contains("Authority=Developer ID Application") {
            return Err("新版本的签名不对，没装".into());
        }
        let staged = bundle.with_extension("app.new");
        let old = bundle.with_extension("app.old");
        let _ = fs::remove_dir_all(&staged);
        let _ = fs::remove_dir_all(&old);
        run("/usr/bin/ditto", &[&src_s, &staged.to_string_lossy()])?;
        fs::rename(&bundle, &old).map_err(|e| format!("换不了正在用的 app：{e}"))?;
        if let Err(e) = fs::rename(&staged, &bundle) {
            let _ = fs::rename(&old, &bundle);
            return Err(format!("换不了正在用的 app：{e}"));
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = run("/usr/bin/hdiutil", &["detach", "-force", &mnt_s]);
    result?;
    // 退出后再打开新的，带 --open 直接开主窗口（不带只在菜单栏，看着像没重新打开）
    Command::new("/bin/sh").args(["-c", &format!("sleep 2; /usr/bin/open -n '{}' --args --open", bundle.display())]).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install(zip: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let tmp = dir().join("unpacked");
    let _ = fs::remove_dir_all(&tmp);
    let ps = format!("Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force", zip.display(), tmp.display());
    let ok = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &ps]).creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
    let new = tmp.join("beayy.exe");
    if !ok || !new.is_file() {
        return Err("解压新版本失败".into());
    }
    let old = exe.with_file_name("beayy.old.exe");
    let _ = fs::remove_file(&old);
    fs::rename(&exe, &old).map_err(|e| format!("换不了正在用的程序：{e}"))?;
    if let Err(e) = fs::copy(&new, &exe) {
        let _ = fs::rename(&old, &exe);
        return Err(format!("换不了正在用的程序：{e}"));
    }
    let _ = fs::remove_dir_all(&tmp);
    // 等这个进程退出再打开新的，带 --open 直接开主窗口
    Command::new("cmd")
        .args(["/C", &format!("ping -n 3 127.0.0.1 >nul & start \"\" \"{}\" --open", exe.display())])
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install(_: &Path) -> Result<(), String> {
    Err("这个系统还不支持自动更新".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        assert!(super::newer("0.2.1", "0.2.0"));
        assert!(super::newer("v0.10.0", "0.9.9"));
        assert!(!super::newer("0.2.0", "0.2.0"));
    }
}
