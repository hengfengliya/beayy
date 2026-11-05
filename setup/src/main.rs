//! Beayy 的 Windows 安装器：一个窗口，欢迎 → 安装（进度）→ 完成（桌面快捷方式、开机自动启动、打开 Beayy）。
//! beayy.exe 由 build.rs 压缩后打包在里面；装、登记、卸载的做法在 beayy-install。
//! 别的系统上只演示界面（假进度、不写文件），开发时看样子用。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
#[cfg(target_os = "windows")]
use std::io::Read;
use tauri::{Emitter, WebviewUrl, WebviewWindowBuilder};

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.deflate"));

#[derive(Serialize)]
struct Info {
    version: &'static str,
    dir: String,
    demo: bool,
}

#[tauri::command]
fn info() -> Info {
    #[cfg(target_os = "windows")]
    let dir = beayy_install::default_dir().display().to_string();
    #[cfg(not(target_os = "windows"))]
    let dir = r"C:\Users\you\AppData\Local\Programs\Beayy".to_string();
    Info { version: env!("CARGO_PKG_VERSION"), dir, demo: PAYLOAD.is_empty() || cfg!(not(target_os = "windows")) }
}

/// 换安装位置：选的文件夹不叫 Beayy 就在里面建一个 Beayy（别的安装器也这样，免得把文件撒在选的目录里）
#[tauri::command]
async fn pick_dir() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let o = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if p.is_empty() {
            return None;
        }
        let p = std::path::PathBuf::from(p);
        let named = p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("Beayy")).unwrap_or(false);
        return Some(if named { p } else { p.join("Beayy") }.display().to_string());
    }
    #[cfg(not(target_os = "windows"))]
    None
}

/// 装：关掉在跑的、解压、写程序、登记开始菜单和卸载项。进度用 progress 事件发给页面（0–1），返回 exe 的路径
#[tauri::command]
async fn install(app: tauri::AppHandle, dir: String) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut report = |p: f64| {
            let _ = app.emit("progress", p);
        };
        let _ = tx.send(run(&dir, &mut report));
    });
    rx.recv().map_err(|e| e.to_string())?
}

/// 读压缩数据时数一下读了多少，好报解压进度
#[cfg(target_os = "windows")]
struct Counting<'a> {
    inner: &'a [u8],
    read: usize,
}

#[cfg(target_os = "windows")]
impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner[self.read..].len().min(buf.len()).min(256 * 1024);
        buf[..n].copy_from_slice(&self.inner[self.read..self.read + n]);
        self.read += n;
        Ok(n)
    }
}

fn run(dir: &str, report: &mut dyn FnMut(f64)) -> Result<String, String> {
    if PAYLOAD.is_empty() || cfg!(not(target_os = "windows")) {
        for i in 1..=40 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            report(i as f64 / 40.0);
        }
        return Ok(format!("{dir}\\beayy.exe"));
    }
    #[cfg(target_os = "windows")]
    {
        beayy_install::stop_running();
        report(0.05);
        let mut z = flate2::read::DeflateDecoder::new(Counting { inner: PAYLOAD, read: 0 });
        let mut exe = Vec::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = z.read(&mut buf).map_err(|e| format!("安装包坏了：{e}"))?;
            if n == 0 {
                break;
            }
            exe.extend_from_slice(&buf[..n]);
            report(0.05 + 0.6 * z.get_ref().read as f64 / PAYLOAD.len() as f64);
        }
        let path = beayy_install::place(std::path::Path::new(dir), &exe, &mut |p| report(0.65 + 0.3 * p))?;
        beayy_install::register(&path, env!("CARGO_PKG_VERSION"))?;
        report(1.0);
        return Ok(path.display().to_string());
    }
    #[allow(unreachable_code)]
    Err("这个系统装不了".into())
}

/// 完成：按勾选建桌面快捷方式、开机自启，要的话打开 Beayy，然后退出
#[tauri::command]
fn finish(app: tauri::AppHandle, exe: String, desktop: bool, autostart: bool, open: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let p = std::path::Path::new(&exe);
        beayy_install::set_desktop_shortcut(p, desktop)?;
        beayy_install::set_login_item(p, autostart)?;
        if open {
            std::process::Command::new(p).arg("--open").spawn().map_err(|e| e.to_string())?;
        }
    }
    let _ = (&exe, desktop, autostart, open);
    app.exit(0);
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![info, pick_dir, install, finish])
        .setup(|app| {
            let w = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Beayy")
                .inner_size(640.0, 440.0)
                .resizable(false)
                .maximizable(false)
                .decorations(false)
                .center();
            // WebView2 的缓存放临时目录，不在用户目录里留一个安装器的文件夹
            #[cfg(target_os = "windows")]
            let w = w.data_directory(std::env::temp_dir().join("beayy-setup"));
            w.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("安装器启动失败");
}
