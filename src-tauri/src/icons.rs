//! app 图标：时间线每段下面那排小图标。
//! - 电脑：本机装了的 app 直接取图标（beayy-capture 的 app_icon_png；Windows 上 id 是程序路径）。
//! - 手机：按 bundle id 查 App Store 公开接口（itunes.apple.com/lookup），顺带拿到 app 名；苹果自带的查不到，
//!   换成 Mac 上对应的 app 取图标。
//! 结果缓存在 `Beayy/icons/<mac|win|ios>/`：`<id>.png`、`<id>.name`；查不到写 `<id>.none`，以后不再查（删掉目录就会重查）。
//! 返回给网页的是 data URL，免得开放文件访问。

use beayy_core::default_root;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// iPhone 自带 app → Mac 上对应的 app
const IOS_TO_MAC: &[(&str, &str)] = &[
    ("com.apple.mobilesafari", "com.apple.Safari"),
    ("com.apple.MobileSMS", "com.apple.MobileSMS"),
    ("com.apple.mobilenotes", "com.apple.Notes"),
    ("com.apple.mobilecal", "com.apple.iCal"),
    ("com.apple.mobilemail", "com.apple.mail"),
    ("com.apple.mobileslideshow", "com.apple.Photos"),
    ("com.apple.mobiletimer", "com.apple.clock"),
    ("com.apple.Preferences", "com.apple.systempreferences"),
    ("com.apple.reminders", "com.apple.reminders"),
    ("com.apple.Maps", "com.apple.Maps"),
    ("com.apple.Music", "com.apple.Music"),
    ("com.apple.AppStore", "com.apple.AppStore"),
    ("com.apple.weather", "com.apple.weather"),
    ("com.apple.facetime", "com.apple.FaceTime"),
    ("com.apple.mobilephone", "com.apple.mobilephone"),
    // 通话中界面、闹钟响铃界面这类系统界面，用对应 app 的图标
    ("com.apple.InCallService", "com.apple.mobilephone"),
    ("com.apple.ClockAngel", "com.apple.clock"),
    ("com.apple.shortcuts", "com.apple.shortcuts"),
    ("com.apple.podcasts", "com.apple.podcasts"),
    ("com.apple.stocks", "com.apple.stocks"),
    ("com.apple.Health", "com.apple.Health"),
    ("com.apple.findmy", "com.apple.findmy"),
    ("com.apple.calculator", "com.apple.calculator"),
    ("com.apple.VoiceMemos", "com.apple.VoiceMemos"),
    ("com.apple.Passbook", "com.apple.Passbook"),
];

fn dir(phone: bool) -> PathBuf {
    // Windows 0.2.44 以前取不到电脑图标、写满了 .none，换个目录重新取
    default_root().join("icons").join(if phone { "ios" } else if cfg!(target_os = "windows") { "win" } else { "mac" })
}

fn safe(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect()
}

pub(crate) fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64(png))
}

pub(crate) fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

// 手机版（mobile/）也编这个文件，它不依赖 beayy-capture
#[cfg(all(target_os = "macos", not(beayy_mobile)))]
fn desktop_icon(id: &str) -> Option<Vec<u8>> {
    beayy_capture::macos::app_icon_png(id, 64)
}
#[cfg(all(target_os = "windows", not(beayy_mobile)))]
fn desktop_icon(id: &str) -> Option<Vec<u8>> {
    beayy_capture::windows::app_icon_png(id, 64)
}
#[cfg(not(any(all(target_os = "macos", not(beayy_mobile)), all(target_os = "windows", not(beayy_mobile)))))]
fn desktop_icon(_id: &str) -> Option<Vec<u8>> {
    None
}

/// App Store 查图标和名字（名字也给手机时间线用，见 data.rs 的 app_names）
fn app_store(id: &str) -> Option<(Vec<u8>, String)> {
    let agent: ureq::Agent = beayy_sync::http_agent(ureq::Agent::config_builder()
        .proxy(None)
        .tls_config(beayy_core::tls!())
        .timeout_global(Some(Duration::from_secs(8)))
        .build());
    // 先查中国区，没上架的（比如 Claude）再查美区
    let lookup = |country: &str| -> Option<Value> {
        let v: Value = agent.get(&format!("https://itunes.apple.com/lookup?bundleId={id}&country={country}")).call().ok()?.body_mut().read_json().ok()?;
        v["results"].get(0).cloned()
    };
    let r = lookup("cn").or_else(|| lookup("us"))?;
    let art = r["artworkUrl100"].as_str().or(r["artworkUrl60"].as_str())?;
    // 100 换成 128 的 PNG
    let art = art.replace("100x100bb.jpg", "128x128bb.png");
    let mut resp = agent.get(&art).call().ok()?;
    let bytes = resp.body_mut().read_to_vec().ok()?;
    Some((bytes, r["trackName"].as_str().unwrap_or("").to_string()))
}

/// 取一个 app 的图标（和手机 app 的名字），先看缓存
fn one(id: &str, phone: bool) -> Value {
    let d = dir(phone);
    let f = safe(id);
    let (png, name, none) = (d.join(format!("{f}.png")), d.join(format!("{f}.name")), d.join(format!("{f}.none")));
    let name_of = || fs::read_to_string(&name).ok().filter(|s| !s.is_empty());
    if let Ok(b) = fs::read(&png) {
        return json!({"icon": data_url(&b), "name": name_of()});
    }
    if none.exists() {
        return json!({"icon": null, "name": name_of()});
    }
    let _ = fs::create_dir_all(&d);
    let got = if phone {
        match app_store(id) {
            Some((b, n)) => {
                let _ = fs::write(&name, &n);
                Some(b)
            }
            None => IOS_TO_MAC.iter().find(|(i, _)| i.eq_ignore_ascii_case(id)).and_then(|(_, m)| desktop_icon(m)).or_else(|| desktop_icon(id)),
        }
    } else {
        desktop_icon(id)
    };
    match got {
        Some(b) => {
            let _ = fs::write(&png, &b);
            json!({"icon": data_url(&b), "name": name_of()})
        }
        None => {
            let _ = fs::write(&none, "");
            json!({"icon": null, "name": name_of()})
        }
    }
}

/// 一批 bundle id 的图标：{id: {icon: data URL 或 null, name: App Store 上的名字或 null}}。
/// 没缓存的手机 app 要联网查，并发查，第一次会慢几秒，之后都走缓存。
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn get_icons(ids: Vec<String>, phone: bool) -> Map<String, Value> {
    let mut ids = ids;
    ids.sort();
    ids.dedup();
    ids.truncate(200);
    let results: Vec<(String, Value)> = std::thread::scope(|s| {
        let hs: Vec<_> = ids.iter().filter(|id| !id.is_empty()).map(|id| s.spawn(move || (id.clone(), one(id, phone)))).collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}
