//! Windows 上装、卸 Beayy。只装给当前用户（默认 %LOCALAPPDATA%\Programs\Beayy，不要管理员），自动更新能直接换 beayy.exe。
//! 安装器（setup/）用 `place`、`register`、`set_desktop_shortcut`、`set_login_item`；
//! app 自己用开机自启的开关、`sync_installed_version`，`beayy.exe --uninstall` 走 `uninstall`。
//! 记录的数据在 %APPDATA%\Beayy，卸载不删。
#![cfg(target_os = "windows")]

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use beayy_core::i18n::tr;
use windows::core::{w, Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellLink, KNOWN_FOLDER_FLAG};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_OKCANCEL};

pub const EXE: &str = "beayy.exe";
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const UNINSTALL_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Beayy");
const NAME: PCWSTR = w!("Beayy");
const LNK: &str = "Beayy.lnk";
const NO_WINDOW: u32 = 0x0800_0000;

/// 装到哪：装过的就是上次的位置，没装过是 %LOCALAPPDATA%\Programs\Beayy
pub fn default_dir() -> PathBuf {
    if let Some(d) = reg_string(UNINSTALL_KEY, w!("InstallLocation")).filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Programs").join("Beayy")
}

/// 关掉在跑的 Beayy（换文件前；以前解压 zip 跑的也关，免得两个一起采）。不关自己
pub fn stop_running() {
    let me = format!("PID ne {}", std::process::id());
    let ok = Command::new("taskkill").args(["/F", "/IM", EXE, "/FI", &me]).creation_flags(NO_WINDOW).output().map(|o| String::from_utf8_lossy(&o.stdout).contains(EXE)).unwrap_or(false);
    if ok {
        std::thread::sleep(std::time::Duration::from_millis(800));
    }
}

/// 把程序写到 dir\beayy.exe（先写 .part 再换名），边写边报进度（0–1）；返回 exe 的路径
pub fn place(dir: &Path, bytes: &[u8], progress: &mut dyn FnMut(f64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("建不了 {}：{e}", dir.display()))?;
    let exe = dir.join(EXE);
    let part = dir.join("beayy.exe.part");
    let mut f = std::fs::File::create(&part).map_err(|e| format!("写不进 {}：{e}", dir.display()))?;
    let step = 1 << 20;
    for (i, chunk) in bytes.chunks(step).enumerate() {
        f.write_all(chunk).map_err(|e| e.to_string())?;
        progress(((i + 1) * step).min(bytes.len()) as f64 / bytes.len().max(1) as f64);
    }
    drop(f);
    if exe.exists() {
        std::fs::remove_file(&exe).map_err(|_| "Beayy 还在运行，退出后再装".to_string())?;
    }
    std::fs::rename(&part, &exe).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join("beayy.old.exe"));
    Ok(exe)
}

/// 登记：开始菜单快捷方式、「应用和功能」里的卸载项
pub fn register(exe: &Path, version: &str) -> Result<(), String> {
    if let Some(p) = known(&FOLDERID_Programs) {
        shortcut(&p.join(LNK), exe)?;
    }
    let dir = exe.parent().unwrap_or(exe).display().to_string();
    let size = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    for (k, v) in [
        (w!("DisplayName"), "Beayy".to_string()),
        (w!("DisplayVersion"), version.to_string()),
        (w!("DisplayIcon"), exe.display().to_string()),
        (w!("Publisher"), "Beayy".to_string()),
        (w!("URLInfoAbout"), "https://beayy.com".to_string()),
        (w!("InstallLocation"), dir),
        (w!("UninstallString"), format!("\"{}\" --uninstall", exe.display())),
    ] {
        set_reg_string(UNINSTALL_KEY, k, &v)?;
    }
    for (k, v) in [(w!("NoModify"), 1), (w!("NoRepair"), 1), (w!("EstimatedSize"), size)] {
        set_reg_dword(UNINSTALL_KEY, k, v)?;
    }
    Ok(())
}

/// 桌面快捷方式：建或删
pub fn set_desktop_shortcut(exe: &Path, on: bool) -> Result<(), String> {
    let Some(desk) = known(&FOLDERID_Desktop) else { return Err("找不到桌面".into()) };
    let lnk = desk.join(LNK);
    if on {
        shortcut(&lnk, exe)
    } else {
        let _ = std::fs::remove_file(lnk);
        Ok(())
    }
}

/// 开机自动启动开着没有：当前用户的 Run 里有 Beayy，且指向这个 exe
pub fn login_item_enabled(exe: &Path) -> bool {
    reg_string(RUN_KEY, NAME).map(|v| v.trim().trim_matches('"').eq_ignore_ascii_case(&exe.to_string_lossy())).unwrap_or(false)
}

/// 打开或关掉开机自动启动（当前用户的 Run，不要管理员）
pub fn set_login_item(exe: &Path, on: bool) -> Result<(), String> {
    if on {
        set_reg_string(RUN_KEY, NAME, &format!("\"{}\"", exe.display()))
    } else {
        delete_value(RUN_KEY, NAME)
    }
}

/// 用安装器装的：自动更新换了 exe 以后，把「应用和功能」里显示的版本跟上
pub fn sync_installed_version(version: &str) {
    if reg_string(UNINSTALL_KEY, w!("DisplayName")).is_none() {
        return;
    }
    if reg_string(UNINSTALL_KEY, w!("DisplayVersion")).as_deref() != Some(version) {
        let _ = set_reg_string(UNINSTALL_KEY, w!("DisplayVersion"), version);
    }
}

/// 卸载（「应用和功能」里点卸载 → beayy.exe --uninstall）：问一句，删快捷方式、开机自启、卸载项，
/// 这个进程退出后删程序（只删自己放的文件，目录空了才删，不碰别的）。数据不删
pub fn uninstall() {
    let title = HSTRING::from(tr("卸载 Beayy"));
    let ok = unsafe { MessageBoxW(None, &HSTRING::from(tr("卸载 Beayy？\n记录的数据会留在这台电脑上。")), &title, MB_OKCANCEL | MB_ICONQUESTION) };
    if ok != IDOK {
        return;
    }
    stop_running();
    let exe = std::env::current_exe().unwrap_or_default();
    for f in [&FOLDERID_Desktop, &FOLDERID_Programs] {
        if let Some(p) = known(f) {
            let _ = std::fs::remove_file(p.join(LNK));
        }
    }
    let _ = delete_value(RUN_KEY, NAME);
    let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY) };
    unsafe { MessageBoxW(None, &HSTRING::from(tr("Beayy 已卸载。")), &title, MB_OK | MB_ICONINFORMATION) };
    if let Some(dir) = exe.parent() {
        let cmd = format!(
            "ping -n 3 127.0.0.1 >nul & del /f /q \"{0}\\beayy.exe\" \"{0}\\beayy.old.exe\" & rmdir \"{0}\"",
            dir.display()
        );
        // raw_arg 原样交给 cmd：走 args 会把引号转义成 \"，cmd 不认，del、rmdir 就删不掉
        let _ = Command::new("cmd").raw_arg(format!("/C {cmd}")).creation_flags(NO_WINDOW).spawn();
    }
}

fn shortcut(lnk: &Path, exe: &Path) -> Result<(), String> {
    let dir = exe.parent().unwrap_or(exe);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.message())?;
        link.SetArguments(w!("--open")).map_err(|e| e.message())?;
        link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(|e| e.message())?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0).map_err(|e| e.message())?;
        link.SetDescription(w!("Be Authentic. Your Yesterdays.")).map_err(|e| e.message())?;
        let file: IPersistFile = link.cast().map_err(|e| e.message())?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| format!("建不了快捷方式：{}", e.message()))
    }
}

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

fn reg_string(key: PCWSTR, name: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    if err.is_err() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

fn set_reg_string(key: PCWSTR, name: PCWSTR, value: &str) -> Result<(), String> {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_SZ.0, Some(wide.as_ptr().cast()), (wide.len() * 2) as u32) }.ok().map_err(|e| e.message())
}

fn set_reg_dword(key: PCWSTR, name: PCWSTR, value: u32) -> Result<(), String> {
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_DWORD.0, Some((&value as *const u32).cast()), 4) }.ok().map_err(|e| e.message())
}

fn delete_value(key: PCWSTR, name: PCWSTR) -> Result<(), String> {
    let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, name) };
    if err == ERROR_FILE_NOT_FOUND { Ok(()) } else { err.ok().map_err(|e| e.message()) }
}
