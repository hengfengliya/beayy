//! 给网页的取数命令（清单见 COMMANDS.md）。口径和 app/ui/mock/生成样例.py 一致：
//! 读仓库的 日数据/日数据_*.json、日总结/日总结_*.md，本机 Beayy/summaries/ 的自采摘要，
//! Beayy/devices/iphone-*/ 的 iPhone 前台事件（手机算法同 脚本/日数据包.py 的 section_phone）。
//! 读的都是正式数据目录（并跑版自己写影子目录，但页面看正式数据）。

use beayy_core::default_root;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

const PICKUP_GAP: f64 = 60.0;
const HARD_STOP: u32 = 21;
const LOCK_ID: &str = "com.apple.SleepLockScreen";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn settings_path() -> PathBuf {
    default_root().join("settings.json")
}

pub(crate) fn read_settings_raw() -> Map<String, Value> {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// 外部数据目录（可选）：放着别的工具生成的 日数据/日数据_*.json、日总结/日总结_*.md。没填就只用 app 自己的数据
pub fn repo_path() -> Option<PathBuf> {
    match read_settings_raw().get("repo_path").and_then(|v| v.as_str()) {
        Some(p) if p.starts_with("~/") => Some(home().join(&p[2..])),
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => None,
    }
}

fn utc_iso(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn section(md: &str, heading: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?ms)^## {}\n+(.+?)(?:\n## |\z)", regex::escape(heading))).ok()?;
    re.captures(md).map(|c| c[1].trim().to_string())
}

/// 常见 iPhone app 的中文名（没有的用 App Store 上查到的名字，见 icons.rs）
const PHONE_NAMES: &[(&str, &str)] = &[
    ("com.tencent.xin", "微信"),
    ("com.ss.iphone.ugc.Aweme", "抖音"),
    ("com.bytedance.ee.lark", "飞书"),
    ("com.tencent.weread", "微信读书"),
    ("com.xingin.discover", "小红书"),
    ("com.dragon.read", "番茄小说"),
    ("com.yadea.smartmoto", "雅迪"),
    ("com.apple.camera", "相机"),
    ("com.alipay.iphoneclient", "支付宝"),
    ("com.openai.chat", "ChatGPT"),
    ("com.apple.InCallService", "通话"),
    ("com.xiaojukeji.didi", "滴滴"),
    ("com.autonavi.amap", "高德地图"),
    ("com.apple.mobileslideshow", "照片"),
    ("com.apple.ClockAngel", "时钟"),
    ("com.tencent.ww", "企业微信"),
    ("com.apple.mobilesafari", "Safari"),
    ("com.taobao.taobao4iphone", "淘宝"),
    ("com.apple.Preferences", "设置"),
    ("com.apple.MobileSMS", "信息"),
    ("com.apple.mobilephone", "电话"),
    ("com.netease.cloudmusic", "网易云音乐"),
    ("tv.danmaku.bilianime", "哔哩哔哩"),
    ("com.anthropic.claude", "Claude"),
    ("com.apple.AppStore", "App Store"),
    ("com.apple.mobiletimer", "时钟"),
    ("com.laiwang.DingTalk", "钉钉"),
    ("com.atebits.Tweetie2", "X"),
    ("com.sina.weibo", "微博"),
    ("ph.telegra.Telegraph", "Telegram"),
];

/// 安卓 app 的名字：安卓外壳记事件时带上的（app_name），读手机记录时收集
static ANDROID_NAMES: std::sync::Mutex<Option<HashMap<String, String>>> = std::sync::Mutex::new(None);

/// 手机应用名：内置的常见 app，安卓事件带来的名字，加上查图标时从 App Store 拿到的名字（Beayy/icons/ios/<id>.name）
fn app_names() -> HashMap<String, String> {
    let mut m: HashMap<String, String> = PHONE_NAMES.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    phone_events();
    if let Some(a) = ANDROID_NAMES.lock().unwrap().as_ref() {
        m.extend(a.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Ok(it) = fs::read_dir(default_root().join("icons/ios")) {
        for f in it.filter_map(|e| e.ok()) {
            let n = f.file_name().to_string_lossy().to_string();
            if let Some(id) = n.strip_suffix(".name") {
                if let Ok(name) = fs::read_to_string(f.path()) {
                    if !name.trim().is_empty() {
                        m.entry(id.to_string()).or_insert_with(|| name.trim().to_string());
                    }
                }
            }
        }
    }
    m
}

// ---------- iPhone ----------

struct PhoneEv {
    t: f64,
    start: bool,
    bundle: String,
}

/// iPhone 记录要读 2 千多个分段文件（约 1 秒），app 里缓存 60 秒；新同步来的记录最多晚 1 分钟出现在页面上
fn phone_events() -> std::sync::Arc<Vec<PhoneEv>> {
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};
    static CACHE: OnceLock<Mutex<Option<(Instant, Arc<Vec<PhoneEv>>)>>> = OnceLock::new();
    let mut g = CACHE.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some((t, v)) = g.as_ref() {
        if t.elapsed() < Duration::from_secs(60) {
            return v.clone();
        }
    }
    let v = Arc::new(read_phone_events());
    *g = Some((std::time::Instant::now(), v.clone()));
    v
}

fn read_phone_events() -> Vec<PhoneEv> {
    let mut out = Vec::new();
    let mut names = HashMap::new();
    let devices = default_root().join("devices");
    let Ok(devs) = fs::read_dir(&devices) else { return out };
    // iPhone（电脑读屏幕使用时间写的）和安卓（手机自己记、同步来的）是同一种事件
    for dev in devs.filter_map(|e| e.ok()).filter(|e| { let n = e.file_name().to_string_lossy().to_string(); n.starts_with("iphone-") || n.starts_with("android-") }) {
        let Ok(segs) = fs::read_dir(dev.path().join("segments")) else { continue };
        for seg in segs.filter_map(|e| e.ok()) {
            let Ok(bytes) = fs::read(seg.path().join("events.jsonl")) else { continue };
            for line in String::from_utf8_lossy(&bytes).lines() {
                let Ok(e) = serde_json::from_str::<Value>(line) else { continue };
                if e["event_type"] == "app.focus" {
                    if let (Some(t), Some(b)) = (e["biome_time"].as_f64(), e["bundle_id"].as_str()) {
                        if let Some(n) = e["app_name"].as_str().filter(|n| !n.is_empty()) {
                            names.insert(b.to_string(), n.to_string());
                        }
                        out.push(PhoneEv { t, start: e["state"] == "start", bundle: b.into() });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    *ANDROID_NAMES.lock().unwrap() = Some(names);
    out
}

fn day_bounds(date: NaiveDate) -> (DateTime<Local>, DateTime<Local>) {
    let s = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap()).unwrap();
    (s, s + Duration::days(1))
}

fn phone_day(ev: &[PhoneEv], date: NaiveDate, names: &HashMap<String, String>) -> Option<Value> {
    if ev.is_empty() {
        return None;
    }
    let (s, e) = day_bounds(date);
    let (s0, e0) = (s.timestamp() as f64, e.timestamp() as f64);
    let stop = (s + Duration::hours(HARD_STOP as i64)).timestamp() as f64;
    let dawn_end = (s + Duration::hours(5)).timestamp() as f64;
    let synced = ev.iter().map(|x| x.t).fold(f64::MIN, f64::max);
    let mut spans: Vec<(f64, f64, String)> = Vec::new();
    let mut opened: HashMap<&str, f64> = HashMap::new();
    // 事件已按时间排好：只看当天前 1 天到当天结束这一段（跨午夜的前台区间要从前一天开始配对），不必每天扫全部记录
    let lo = ev.partition_point(|x| x.t < s0 - 86_400.0);
    let hi = ev.partition_point(|x| x.t <= e0 + 86_400.0);
    for x in &ev[lo..hi] {
        if x.start {
            opened.insert(&x.bundle, x.t);
        } else if let Some(a) = opened.remove(x.bundle.as_str()) {
            if x.t > s0 && a < e0 {
                spans.push((a.max(s0), x.t.min(e0), x.bundle.clone()));
            }
        }
    }
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut pickups = 0;
    let mut last_end: Option<f64> = None;
    for (a, z, _) in &spans {
        if last_end.map(|l| a - l > PICKUP_GAP).unwrap_or(true) {
            pickups += 1;
        }
        last_end = Some(last_end.map(|l| l.max(*z)).unwrap_or(*z));
    }
    let used: Vec<&(f64, f64, String)> = spans.iter().filter(|x| x.2 != LOCK_ID).collect();
    let mut by_app: HashMap<&str, f64> = HashMap::new();
    let mut hours: BTreeMap<String, f64> = BTreeMap::new();
    for (a, z, b) in &used {
        *by_app.entry(b.as_str()).or_default() += z - a;
        let h = Local.timestamp_opt(*a as i64, 0).unwrap().hour();
        *hours.entry(format!("{h:02}")).or_default() += z - a;
    }
    let sessions = phone_sessions(&used, names);
    let mut apps: Vec<(&str, f64)> = by_app.into_iter().filter(|(_, v)| *v >= 60.0).collect();
    apps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    Some(json!({
        "total_sec": used.iter().map(|(a, z, _)| z - a).sum::<f64>().round() as i64,
        "pickups": pickups,
        "late_sec": used.iter().filter(|(_, z, _)| *z > stop).map(|(a, z, _)| z - a.max(stop)).sum::<f64>().round() as i64,
        "night_sec": used.iter().filter(|(a, _, _)| *a < dawn_end).map(|(a, z, _)| z.min(dawn_end) - a).sum::<f64>().round() as i64,
        "apps": apps.iter().take(15).map(|(b, v)| json!({"bundle_id": b, "name": names.get(*b).cloned().unwrap_or_else(|| b.to_string()), "sec": v.round() as i64})).collect::<Vec<_>>(),
        "hours": hours.into_iter().map(|(k, v)| (k, json!(v.round() as i64))).collect::<Map<_, _>>(),
        "synced_until": utc_iso(Utc.timestamp_opt(synced as i64, 0).unwrap()),
        "sessions": sessions,
    }))
}

/// 不到这么久的拿起算「零碎」：连着的几次合成一行
const BRIEF_SEC: f64 = 60.0;
/// 零碎拿起之间隔超过这么久就不再合并
const BRIEF_JOIN_GAP: f64 = 15.0 * 60.0;

/// 手机时间线：一次拿起一行（前后两段前台之间空超过 PICKUP_GAP 算新的一次，同「拿起次数」）。
/// 每行：起止、总秒数、各 app 秒数（多到少）。不到 1 分钟的零碎拿起，连着的合成一行（kind＝brief，count＝几次）
fn phone_sessions(used: &[&(f64, f64, String)], names: &HashMap<String, String>) -> Vec<Value> {
    struct Sess {
        a: f64,
        z: f64,
        sec: f64,
        count: usize,
        apps: Vec<(String, f64)>,
        /// 合进来的每一次都不到 BRIEF_SEC
        small: bool,
    }
    let mut out: Vec<Sess> = vec![];
    for (a, z, b) in used {
        let new = out.last().map(|l| a - l.z > PICKUP_GAP).unwrap_or(true);
        if new {
            out.push(Sess { a: *a, z: *z, sec: 0.0, count: 1, apps: vec![], small: true });
        }
        let cur = out.last_mut().unwrap();
        cur.z = cur.z.max(*z);
        cur.sec += z - a;
        match cur.apps.iter_mut().find(|(x, _)| x == b) {
            Some(x) => x.1 += z - a,
            None => cur.apps.push((b.clone(), z - a)),
        }
    }
    for x in out.iter_mut() {
        x.small = x.sec < BRIEF_SEC;
    }
    // 零碎拿起合并
    let mut merged: Vec<Sess> = vec![];
    for x in out {
        if let Some(l) = merged.last_mut() {
            if x.small && l.small && x.a - l.z < BRIEF_JOIN_GAP {
                l.z = x.z;
                l.sec += x.sec;
                l.count += x.count;
                for (b, v) in x.apps {
                    match l.apps.iter_mut().find(|(y, _)| *y == b) {
                        Some(y) => y.1 += v,
                        None => l.apps.push((b, v)),
                    }
                }
                continue;
            }
        }
        merged.push(x);
    }
    merged
        .into_iter()
        .map(|mut x| {
            x.apps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            json!({
                "start": utc_iso(Utc.timestamp_opt(x.a as i64, 0).unwrap()),
                "end": utc_iso(Utc.timestamp_opt(x.z.ceil() as i64, 0).unwrap()),
                "sec": x.sec.round() as i64,
                "kind": if x.count > 1 { "brief" } else { "session" },
                "count": x.count,
                "apps": x.apps.iter().map(|(b, v)| json!({"bundle_id": b, "name": names.get(b).cloned().unwrap_or_else(|| b.clone()), "sec": v.round() as i64})).collect::<Vec<_>>(),
            })
        })
        .collect()
}

// ---------- 摘要 ----------

/// 解析过的 10 分钟摘要：文件 →（修改时间、大小、本地日期、这一段，parts 还没归一）。文件没变就不再读
static SEG_FILES: std::sync::LazyLock<std::sync::Mutex<HashMap<PathBuf, (std::time::SystemTime, u64, String, Value)>>> = std::sync::LazyLock::new(Default::default);

/// 文件的（修改时间、大小）：缓存用来判断变没变
fn stamp(p: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let m = fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn self_segments() -> HashMap<String, Vec<Value>> {
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    // 项目名按别名表归到规范名，领域按登记表固定（同一个项目前后一致）
    let sm = beayy_summary::Summarizer::new(default_root());
    let (aliases, domains) = (sm.aliases(), sm.domains());
    let canon = |parts: Option<&Value>| -> Value {
        Value::Array(
            parts
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut x| {
                    let p = x["project"].as_str().unwrap_or("").to_string();
                    let p = aliases.get(&p).cloned().unwrap_or(p);
                    if let Some(d) = domains.get(&p) {
                        x["domain"] = json!(d);
                    }
                    x["project"] = json!(p);
                    // 旧分类名（开发、会议……）也归到现在的二级类型
                    let act = beayy_summary::canon_activity(x["activity"].as_str().unwrap_or(""));
                    x["activity"] = json!(act);
                    x
                })
                .collect(),
        )
    };
    // 本机写的在 summaries/；别的电脑写的经同步落在 devices/<设备>/summaries/（带上是哪台）
    let me = beayy_core::local_device();
    let mut files: Vec<(PathBuf, String)> = vec![];
    let list = |dir: PathBuf| -> Vec<PathBuf> { fs::read_dir(dir).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default() };
    files.extend(list(default_root().join("summaries")).into_iter().map(|p| (p, me.clone())));
    for dev in list(default_root().join("devices")) {
        let d = dev.file_name().unwrap_or_default().to_string_lossy().to_string();
        if d != me && beayy_summary::is_computer(&d) {
            files.extend(list(dev.join("summaries")).into_iter().map(|p| (p, d.clone())));
        }
    }
    files.sort();
    // 2000 多个文件每次都读一遍要 150–600 毫秒：只读新的、改过的，其余用缓存（别名表、领域表每次重新套，改了马上生效）
    let mut cache = SEG_FILES.lock().unwrap_or_else(|e| e.into_inner());
    let listed: HashSet<&PathBuf> = files.iter().map(|(f, _)| f).collect();
    cache.retain(|p, _| listed.contains(p));
    let front_re = Regex::new(r"(?m)^(title|description|applications): (.*)$").unwrap();
    let mem_re = Regex::new(r"(?ms)^## Memory summary\n+(.+?)(?:\n#|\z)").unwrap();
    let ctx_re = Regex::new(r"(?ms)^### Important non-obvious context about the user\n+(.+?)(?:\n#|\z)").unwrap();
    let rec_re = Regex::new(r"(?ms)^## Recording summary\n+(.+?)(?:\n## |\z)").unwrap();
    for (f, device) in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with("-memory-summary.md") || name.len() < 19 {
            continue;
        }
        let Ok(start) = chrono::NaiveDateTime::parse_from_str(&name[..19], "%Y-%m-%dT%H-%M-%S") else { continue };
        let start = Utc.from_utc_datetime(&start);
        let six = name.contains("-6h-");
        let end = start + if six { Duration::hours(6) } else { Duration::minutes(10) };
        let st = stamp(&f);
        if let Some((m, n, date, seg)) = cache.get(&f) {
            if st == Some((*m, *n)) {
                let mut seg = seg.clone();
                seg["parts"] = canon(seg.get("parts"));
                out.entry(date.clone()).or_default().push(seg);
                continue;
            }
        }
        let md = fs::read_to_string(&f).unwrap_or_default();
        let head = md.split("\n---").next().unwrap_or("");
        let front: HashMap<String, String> = front_re.captures_iter(head).map(|c| (c[1].to_string(), c[2].to_string())).collect();
        let ctx: Vec<String> = ctx_re
            .captures(&md)
            .map(|c| c[1].lines().filter(|l| l.starts_with("- ")).map(|l| l[2..].trim().to_string()).collect())
            .unwrap_or_default();
        let date = start.with_timezone(&Local).format("%Y-%m-%d").to_string();
        let fm = beayy_summary::front(&md);
        let seg = json!({
            "active_sec": fm.get("active_sec").cloned().unwrap_or(Value::Null),
            "parts": fm.get("parts").cloned().unwrap_or(Value::Null),
            "start": utc_iso(start), "end": utc_iso(end), "kind": if six { "6h" } else { "10min" }, "source": "beayy", "device": device,
            "title": front.get("title").cloned().unwrap_or_default(),
            "apps": front.get("applications").map(|a| a.trim_matches(|c| c == '[' || c == ']').split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()).unwrap_or_default(),
            "description": front.get("description").cloned().unwrap_or_default(),
            "memory": mem_re.captures(&md).map(|c| c[1].trim().to_string()).unwrap_or_default(),
            "context": ctx,
            "recording": rec_re.captures(&md).map(|c| c[1].trim().to_string()).unwrap_or_default(),
        });
        if let Some((m, n)) = st {
            cache.insert(f.clone(), (m, n, date.clone(), seg.clone()));
        }
        let mut seg = seg;
        seg["parts"] = canon(seg.get("parts"));
        out.entry(date).or_default().push(seg);
    }
    out
}

fn local_at(date: NaiveDate, hhmm: &str, after: Option<DateTime<Local>>) -> Option<DateTime<Local>> {
    let t = chrono::NaiveTime::parse_from_str(hhmm, "%H:%M").ok()?;
    let mut dt = Local.from_local_datetime(&date.and_time(t)).single()?;
    if let Some(a) = after {
        if dt <= a {
            dt += Duration::days(1);
        }
    }
    Some(dt)
}

fn codex_segments(pack: &Value, date: NaiveDate) -> Vec<Value> {
    let mut out = Vec::new();
    for x in pack["codex"].as_array().cloned().unwrap_or_default() {
        let (Some(a), Some(z0)) = (x["start"].as_str(), x["end"].as_str()) else { continue };
        let Some(a) = local_at(date, a, None) else { continue };
        let Some(z) = local_at(date, z0, Some(a)) else { continue };
        out.push(json!({
            "start": utc_iso(a.with_timezone(&Utc)), "end": utc_iso(z.with_timezone(&Utc)),
            "kind": x["kind"].as_str().unwrap_or("10min"), "source": "codex",
            "title": x["title"], "description": x["description"], "memory": x["memory"],
            "context": x.get("context").cloned().unwrap_or(json!([])), "recording": x["recording"],
        }));
    }
    out
}

// ---------- 电脑：实时（自采事件） ----------

/// 某天的电脑活动，直接从自采事件算（数据包每晚 20:55 才生成，白天页面要用这个）。缓存 60 秒
pub fn live_computer(date: NaiveDate) -> Option<Value> {
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static CACHE: OnceLock<Mutex<HashMap<NaiveDate, (Instant, Option<Value>)>>> = OnceLock::new();
    let mut g = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    if let Some((t, v)) = g.get(&date) {
        if t.elapsed().as_secs() < 60 {
            return v.clone();
        }
    }
    // 过去的日子不会再变：算一次存盘（Beayy/daily/stats_YYYY-MM-DD.json），之后直接读。
    // 算出来是空的先不存：别的电脑的数据可能晚几天才同步到，存了空的这天就一直空着；一周以前的才存空
    let cache = default_root().join("daily").join(format!("stats_{date}.json"));
    let today = Local::now().date_naive();
    let (past, settled) = (date < today, date < today - Duration::days(7));
    let v = match fs::read_to_string(&cache).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        Some(v) if past && (!v.is_null() || settled) => Some(v).filter(|v| !v.is_null()),
        _ => {
            let v = compute_live(date);
            if past && (v.is_some() || settled) {
                let _ = fs::create_dir_all(cache.parent().unwrap());
                let _ = fs::write(&cache, serde_json::to_string(&v).unwrap_or_default());
            }
            v
        }
    };
    g.insert(date, (Instant::now(), v.clone()));
    v
}

/// 某天每个 10 分钟里用过的 app（按秒，多的在前），给没有摘要的时段当时间线的一行（source: "raw"）。
/// 今天的缓存 60 秒；以前的日子算一次留在内存里
fn raw_segments(date: NaiveDate) -> Vec<Value> {
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static CACHE: OnceLock<Mutex<HashMap<NaiveDate, (Instant, Vec<Value>)>>> = OnceLock::new();
    let today = Local::now().date_naive();
    let mut g = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    if let Some((t, v)) = g.get(&date) {
        if date < today - Duration::days(1) || t.elapsed().as_secs() < 60 {
            return v.clone();
        }
    }
    let (s, e) = day_bounds(date);
    let act = beayy_summary::Summarizer::new(default_root()).activity(s.with_timezone(&Utc), e.with_timezone(&Utc));
    let slot = |t: DateTime<Utc>| t - Duration::seconds(t.timestamp().rem_euclid(600)) - Duration::nanoseconds(t.timestamp_subsec_nanos() as i64);
    // 段开始 → [(名字, bundle, 秒)]
    let mut by: BTreeMap<DateTime<Utc>, Vec<(String, String, f64)>> = BTreeMap::new();
    for (a, b, name, bundle) in &act.windows {
        let mut t = *a;
        while t < *b {
            let k = slot(t);
            let next = (k + Duration::minutes(10)).min(*b);
            let sec = (next - t).num_milliseconds() as f64 / 1000.0;
            let apps = by.entry(k).or_default();
            match apps.iter_mut().find(|x| x.1 == *bundle && x.0 == *name) {
                Some(x) => x.2 += sec,
                None => apps.push((name.clone(), bundle.clone(), sec)),
            }
            t = next;
        }
    }
    let out: Vec<Value> = by
        .into_iter()
        .filter_map(|(k, mut apps)| {
            let total: f64 = apps.iter().map(|x| x.2).sum();
            if total < 30.0 {
                return None;
            }
            apps.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            Some(json!({
                "start": utc_iso(k), "end": utc_iso(k + Duration::minutes(10)), "kind": "10min", "source": "raw",
                "title": "", "description": "", "active_sec": total.round() as i64,
                "apps": apps.iter().map(|x| x.1.clone()).filter(|b| !b.is_empty()).collect::<Vec<_>>(),
                "raw_apps": apps.iter().take(6).map(|(n, b, v)| json!({"name": n, "bundle_id": b, "sec": v.round() as i64})).collect::<Vec<_>>(),
            }))
        })
        .collect();
    g.insert(date, (Instant::now(), out.clone()));
    out
}

fn compute_live(date: NaiveDate) -> Option<Value> {
    let (s, e) = day_bounds(date);
    let act = beayy_summary::Summarizer::new(default_root()).activity(s.with_timezone(&Utc), e.with_timezone(&Utc));
    if act.spans.is_empty() && act.windows.is_empty() {
        return None;
    }
    let secs = |a: DateTime<Utc>, b: DateTime<Utc>| (b - a).num_milliseconds() as f64 / 1000.0;
    let mut hours: BTreeMap<String, f64> = BTreeMap::new();
    for (a, b) in &act.spans {
        // 按小时切开
        let mut t = *a;
        while t < *b {
            let lt = t.with_timezone(&Local);
            let next = (lt + Duration::hours(1)).with_minute(0).and_then(|x| x.with_second(0)).map(|x| x.with_timezone(&Utc)).unwrap_or(*b).min(*b);
            *hours.entry(format!("{:02}", lt.hour())).or_default() += secs(t, next);
            t = next;
        }
    }
    let mut apps: Vec<(String, String, f64)> = vec![];
    for (a, b, name, bundle) in &act.windows {
        match apps.iter_mut().find(|x| x.0 == *name) {
            Some(x) => x.2 += secs(*a, *b),
            None => apps.push((name.clone(), bundle.clone(), secs(*a, *b))),
        }
    }
    apps.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
    let hm = |t: &DateTime<Utc>| t.with_timezone(&Local).format("%H:%M").to_string();
    Some(json!({
        "active_sec": act.spans.iter().map(|(a, b)| secs(*a, *b)).sum::<f64>().round() as i64,
        "first": act.spans.first().map(|x| hm(&x.0)),
        "last": act.spans.last().map(|x| hm(&x.1)),
        "switches": act.switches,
        "active_hours": hours.into_iter().map(|(k, v)| (k, json!(v.round() as i64))).collect::<Map<_, _>>(),
        "spans": act.spans.iter().map(|(a, b)| json!([utc_iso(*a), utc_iso(*b)])).collect::<Vec<_>>(),
        "apps": apps.iter().take(20).map(|(n, b, v)| json!({"name": n, "bundle_id": b, "sec": v.round() as i64})).collect::<Vec<_>>(),
    }))
}

// ---------- 手表对话、iPhone 停留点（beayy-remote 从录音服务器拉来的） ----------

/// 两句之间空档超过 3 分钟算另一段对话（同 脚本/日数据包.py 的 section_audio）
const CONV_GAP: f64 = 180.0;
/// 同一个人 20 秒内接着说的并成一行
const MERGE_GAP: f64 = 20.0;
/// 录音分段之间空档超过 5 分钟记为断档
const REC_GAP: f64 = 300.0;

/// 某种设备（watch-、iphone-）在 [from, to) 里的事件：只读这段时间的分段目录
/// device_events 读过的：（events.jsonl, 要的类型）→（修改时间、大小、挑出来的事件）。地点统计 30 天每个文件要读三遍
static EV_FILES: std::sync::LazyLock<std::sync::Mutex<HashMap<(PathBuf, String), (std::time::SystemTime, u64, Vec<Value>)>>> = std::sync::LazyLock::new(Default::default);

fn device_events(prefix: &str, types: &[&str], from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Value> {
    let mut out = vec![];
    let kinds = types.join(",");
    let (lo, hi) = (beayy_core::segment_key(from), beayy_core::segment_key(to));
    let Ok(devs) = fs::read_dir(default_root().join("devices")) else { return out };
    for dev in devs.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with(prefix)) {
        let Ok(segs) = fs::read_dir(dev.path().join("segments")) else { continue };
        for seg in segs.filter_map(|e| e.ok()) {
            let n = seg.file_name().to_string_lossy().to_string();
            if n < lo || n > hi {
                continue;
            }
            let path = seg.path().join("events.jsonl");
            let Some(st) = stamp(&path) else { continue };
            let key = (path, kinds.clone());
            if let Some((m, n, evs)) = EV_FILES.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
                if (*m, *n) == st {
                    out.extend(evs.iter().cloned());
                    continue;
                }
            }
            let Ok(bytes) = fs::read(&key.0) else { continue };
            let mut evs = vec![];
            for line in String::from_utf8_lossy(&bytes).lines() {
                if !types.iter().any(|t| line.contains(t)) {
                    continue;
                }
                if let Ok(e) = serde_json::from_str::<Value>(line) {
                    if types.iter().any(|t| e["event_type"] == *t) {
                        evs.push(e);
                    }
                }
            }
            out.extend(evs.iter().cloned());
            EV_FILES.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (st.0, st.1, evs));
        }
    }
    out
}

fn has_device(prefix: &str) -> bool {
    fs::read_dir(default_root().join("devices")).map(|r| r.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with(prefix))).unwrap_or(false)
}

fn ts_of(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

/// 本人：服务器按声纹认出的名字（不是「他人N」「未辨」的那个）
fn is_self(speaker: &str) -> bool {
    !speaker.starts_with("他人") && speaker != "未辨"
}

/// 某天的对话（手表录音转写；没戴手表时 iPhone 自己录的也算）。没有手表设备、iPhone 也没录过时返回 None
pub fn speech_day(date: NaiveDate) -> Option<Value> {
    let (s, e) = day_bounds(date);
    let (s, e) = (s.with_timezone(&Utc), e.with_timezone(&Utc));
    let mut ev = device_events("watch-", &["speech.segment", "speech.utterance"], s - Duration::minutes(10), e);
    ev.extend(device_events("iphone-", &["speech.segment", "speech.utterance"], s - Duration::minutes(10), e));
    if ev.is_empty() && !has_device("watch-") {
        return None;
    }
    let inday = |x: &Value| ts_of(&x["start"]).map(|t| t >= s && t < e).unwrap_or(false);
    let mut segs: Vec<&Value> = ev.iter().filter(|x| x["event_type"] == "speech.segment" && inday(x)).collect();
    segs.sort_by(|a, b| a["start"].as_str().cmp(&b["start"].as_str()));
    segs.dedup_by(|a, b| a["seg_id"] == b["seg_id"]);
    // 以后同一段话被两台设备都录到：开口时刻＋原文相同的只留一句
    let mut seen = HashSet::new();
    let mut utts: Vec<(DateTime<Utc>, DateTime<Utc>, String, String, String)> = ev
        .iter()
        .filter(|x| x["event_type"] == "speech.utterance" && inday(x))
        .filter_map(|x| {
            let a = ts_of(&x["start"])?;
            let z = ts_of(&x["end"]).unwrap_or(a).max(a);
            Some((a, z, x["speaker"].as_str().unwrap_or("未辨").to_string(), x["text"].as_str().unwrap_or("").to_string(), x["device"].as_str().unwrap_or("").to_string()))
        })
        .filter(|u| seen.insert((u.0, u.3.clone())))
        .collect();
    utts.sort_by(|a, b| a.0.cmp(&b.0));
    let secs = |a: DateTime<Utc>, b: DateTime<Utc>| (b - a).num_milliseconds() as f64 / 1000.0;
    let mut by_spk: BTreeMap<String, f64> = BTreeMap::new();
    let mut hours: BTreeMap<String, f64> = BTreeMap::new();
    for (a, z, who, _, _) in &utts {
        *by_spk.entry(who.clone()).or_default() += secs(*a, *z);
        *hours.entry(format!("{:02}", a.with_timezone(&Local).hour())).or_default() += secs(*a, *z);
    }
    // 分成一段段对话；同一个人连着说的并成一行
    struct Line { a: DateTime<Utc>, z: DateTime<Utc>, who: String, text: String }
    struct Conv { a: DateTime<Utc>, z: DateTime<Utc>, lines: Vec<Line>, talk: BTreeMap<String, f64>, dev: String }
    let mut convs: Vec<Conv> = vec![];
    for (a, z, who, text, dev) in utts {
        if convs.last().map(|c| secs(c.z, a) > CONV_GAP).unwrap_or(true) {
            convs.push(Conv { a, z: a, lines: vec![], talk: BTreeMap::new(), dev });
        }
        let c = convs.last_mut().unwrap();
        *c.talk.entry(who.clone()).or_default() += secs(a, z);
        match c.lines.last_mut() {
            Some(l) if l.who == who && secs(l.z, a) <= MERGE_GAP => {
                l.text += &text;
                l.z = z;
            }
            _ => c.lines.push(Line { a, z, who, text }),
        }
        c.z = c.z.max(z);
    }
    let rec: f64 = segs.iter().map(|x| x["audio_sec"].as_f64().unwrap_or(0.0)).sum();
    let mut gaps = vec![];
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = vec![];
    for x in &segs {
        let (Some(a), Some(z)) = (ts_of(&x["start"]), ts_of(&x["end"])) else { continue };
        if let Some(l) = spans.last_mut() {
            if secs(l.1, a) <= REC_GAP {
                l.1 = l.1.max(z);
                continue;
            }
            gaps.push(json!([utc_iso(l.1), utc_iso(a)]));
        }
        spans.push((a, z));
    }
    let batt: Vec<f64> = segs.iter().filter_map(|x| x["battery"].as_f64()).collect();
    let self_name = by_spk.keys().find(|k| is_self(k)).cloned();
    let round = |m: BTreeMap<String, f64>| m.into_iter().map(|(k, v)| (k, json!(v.round() as i64))).collect::<Map<_, _>>();
    let talk: f64 = by_spk.values().sum();
    Some(json!({
        "recorded_sec": rec.round() as i64,
        "talk_sec": talk.round() as i64,
        "self": self_name,
        "self_sec": self_name.as_ref().and_then(|n| by_spk.get(n)).map(|v| v.round() as i64).unwrap_or(0),
        "speakers_sec": round(by_spk),
        "hours": round(hours),
        "first": segs.first().and_then(|x| x["start"].as_str().and_then(|t| ts_of(&json!(t)))).map(utc_iso),
        "last": segs.iter().filter_map(|x| ts_of(&x["end"])).max().map(utc_iso),
        "recorded": spans.iter().map(|(a, z)| json!([utc_iso(*a), utc_iso(*z)])).collect::<Vec<_>>(),
        "gaps": gaps,
        "battery": if batt.is_empty() { Value::Null } else { json!([batt[0], batt[batt.len() - 1]]) },
        "segments": segs.len(),
        "conversations": convs.into_iter().map(|c| json!({
            "start": utc_iso(c.a), "end": utc_iso(c.z), "device": c.dev,
            "talk": c.talk.iter().map(|(k, v)| json!({"speaker": k, "sec": v.round() as i64})).collect::<Vec<_>>(),
            "lines": c.lines.iter().map(|l| json!({"start": utc_iso(l.a), "end": utc_iso(l.z), "speaker": l.who, "text": l.text})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    }))
}

/// 同一地点两段之间隔不到 15 分钟并成一段（同 脚本/日数据包.py 的 load_stays）
const MERGE_PLACE: i64 = 15 * 60;
/// 地点标签默认半径（米）
const PLACE_R: f64 = 150.0;

pub(crate) fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let h = ((p2 - p1) / 2.0).sin().powi(2) + p1.cos() * p2.cos() * ((lon2 - lon1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().asin()
}

/// 自己起的地点名（家、公司……）：[{label, type, lat, lon, r}]。先读 Beayy/places/labels.json，没有再读外部数据目录的 raw/places/labels.json
fn place_labels() -> Vec<Value> {
    let mut paths = vec![labels_path()];
    if let Some(r) = repo_path() {
        paths.push(r.join("raw").join("places").join("labels.json"));
    }
    paths.iter().find_map(|p| fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())).unwrap_or_default()
}

/// 停留点的可信程度：iPhone 的 CLVisit（系统判定的到访）> iPhone 显著位置变化补的短停留 > 手表录音时取点算的
fn place_rank(v: &Value) -> u8 {
    if v["device"].as_str().unwrap_or("").starts_with("watch-") {
        1
    } else if v["place_source"] == "slc" {
        2
    } else {
        3
    }
}

/// 自己起的地点名存在哪：Beayy/places/labels.json
fn labels_path() -> PathBuf {
    default_root().join("places").join("labels.json")
}

/// 给一个地方起名（家、公司……），存进 Beayy/places/labels.json；半径内已有的就改它，label 为空＝删掉这个名字。
/// 第一次写时先把外部数据目录 raw/places/labels.json 里已有的抄过来，免得新文件把它们盖住
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_place_label(lat: f64, lon: f64, label: String, kind: Option<String>) -> Result<(), String> {
    let mut all = place_labels();
    let label = label.trim().to_string();
    let hit = all.iter().position(|x| haversine(lat, lon, x["lat"].as_f64().unwrap_or(0.0), x["lon"].as_f64().unwrap_or(0.0)) <= x["r"].as_f64().unwrap_or(PLACE_R));
    match (hit, label.is_empty()) {
        (Some(i), true) => {
            all.remove(i);
        }
        (Some(i), false) => {
            all[i]["label"] = json!(label);
            if let Some(k) = &kind {
                all[i]["type"] = json!(k);
            }
        }
        (None, false) => all.push(json!({ "label": label, "type": kind.unwrap_or_else(|| "other".into()), "lat": (lat * 1e5).round() / 1e5, "lon": (lon * 1e5).round() / 1e5, "r": PLACE_R })),
        (None, true) => return Ok(()),
    }
    let p = labels_path();
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// 在电脑上确认过是哪家店：Beayy/places/picks.json，{"stays": {停留点 id: {shop, type}}, "remembered": [{name, type, lat, lon, at}]}。
/// 同手机的 picked.json：remembered 里的店，以后 30 米内的停留只要它在候选里就当已确认
fn picks_path() -> PathBuf {
    default_root().join("places").join("picks.json")
}

fn place_picks() -> Value {
    fs::read_to_string(picks_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({ "stays": {}, "remembered": [] }))
}

fn write_json(p: &PathBuf, v: &Value) -> Result<(), String> {
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(v).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// 确认一段停留是哪家店（「这次在哪」里点的，或自己输入的店名）；店的坐标用高德给的，自己输入的用停留点的。
/// 同一处同名的店只记一条
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_place_pick(id: String, shop: String, kind: Option<String>, lat: f64, lon: f64) -> Result<(), String> {
    let shop = shop.trim().to_string();
    if shop.is_empty() {
        return Err("店名是空的".into());
    }
    let mut p = place_picks();
    if !p["stays"].is_object() {
        p["stays"] = json!({});
    }
    // 一段停留可以认定好几家（商场里逛了几家）：再点一次同一家＝取消。旧格式 {shop, type} 先转成 {shops: [...]}
    let mut shops: Vec<Value> = picks_of(&p["stays"][&id]);
    let had = shops.iter().any(|x| x["name"] == shop.as_str());
    if had {
        shops.retain(|x| x["name"] != shop.as_str());
    } else {
        shops.push(json!({ "name": shop, "type": kind }));
    }
    if shops.is_empty() {
        if let Some(m) = p["stays"].as_object_mut() {
            m.remove(&id);
        }
    } else {
        p["stays"][&id] = json!({ "shops": shops });
    }
    if had {
        return write_json(&picks_path(), &p);
    }
    let mut rem: Vec<Value> = p["remembered"].as_array().cloned().unwrap_or_default();
    rem.retain(|x| !(x["name"] == shop.as_str() && haversine(lat, lon, x["lat"].as_f64().unwrap_or(0.0), x["lon"].as_f64().unwrap_or(0.0)) <= 30.0));
    rem.push(json!({ "name": shop, "type": kind, "lat": (lat * 1e6).round() / 1e6, "lon": (lon * 1e6).round() / 1e6, "at": utc_iso(Utc::now()) }));
    p["remembered"] = json!(rem);
    write_json(&picks_path(), &p)
}

/// 一段停留认定过的店（新格式 {shops: [{name, type}]}，旧格式 {shop, type}）
fn picks_of(v: &Value) -> Vec<Value> {
    if let Some(a) = v["shops"].as_array() {
        return a.clone();
    }
    v["shop"].as_str().map(|n| vec![json!({ "name": n, "type": v["type"] })]).unwrap_or_default()
}

/// 撤销对一段停留的确认（名字退回只写区域）
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn clear_place_pick(id: String) -> Result<(), String> {
    let mut p = place_picks();
    if let Some(m) = p["stays"].as_object_mut() {
        m.remove(&id);
    }
    write_json(&picks_path(), &p)
}

/// 「地点 · 常去」：最近 days 天（默认 30）的停留按地方合计。起过名的按名字合（没去过的也列，次数 0），
/// 没起名的按区域合（同一个区县 · 区域），只列去过 5 次以上的。只用缓存，不问高德（没缓存过分级的用手机传来的地名）
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn place_stats(days: Option<i64>) -> Value {
    let days = days.unwrap_or(30).clamp(1, 120);
    let today = Local::now().date_naive();
    let labels = place_labels();
    // 名字 → (次数, 秒, 最近一天, 纬度, 经度, 区县, 种类)
    let mut agg: BTreeMap<(bool, String), (BTreeSet<String>, f64, String, f64, f64, String, String, String)> = BTreeMap::new();
    for k in 0..days {
        let date = today - Duration::days(k);
        let Some(p) = places_day_opt(date, false) else { continue };
        for v in p["visits"].as_array().cloned().unwrap_or_default() {
            let named = v["label"].as_str().filter(|x| !x.is_empty()).map(String::from);
            let area = v["admin"]["area"].as_str().map(String::from).or_else(|| v["place_name"].as_str().and_then(|n| n.split(" · ").nth(1).or(n.split(" · ").next()).map(String::from)));
            let Some(key) = named.clone().or(area) else { continue };
            let sec = match (ts_of(&v["start"]), ts_of(&v["end"]).or(Some(Utc::now()))) { (Some(a), Some(z)) => (z - a).num_seconds().max(0) as f64, _ => 0.0 };
            let e = agg.entry((named.is_some(), key)).or_insert_with(|| (BTreeSet::new(), 0.0, String::new(), v["lat"].as_f64().unwrap_or(0.0), v["lon"].as_f64().unwrap_or(0.0), v["admin"]["district"].as_str().unwrap_or("").to_string(), v["type"].as_str().unwrap_or("").to_string(), v["admin"]["city"].as_str().unwrap_or("").to_string()));
            e.0.insert(v["id"].as_str().unwrap_or("").to_string());
            e.1 += sec;
            let d = date.format("%Y-%m-%d").to_string();
            if d > e.2 {
                e.2 = d;
            }
        }
    }
    let row = |name: &str, (ids, sec, last, lat, lon, district, kind, city): &(BTreeSet<String>, f64, String, f64, f64, String, String, String)| json!({ "name": name, "count": ids.len(), "sec": sec.round(), "last": last, "lat": lat, "lon": lon, "district": district, "city": city, "type": kind });
    let named: Vec<Value> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let n = l["label"].as_str().unwrap_or("");
            let mut r = agg.get(&(true, n.to_string())).map(|x| row(n, x)).unwrap_or_else(|| json!({ "name": n, "count": 0, "sec": 0, "last": null }));
            r["index"] = json!(i);
            r["type"] = l["type"].clone();
            r["address"] = l["address"].clone();
            r["r"] = l["r"].clone();
            r
        })
        .collect();
    let mut named = named;
    named.sort_by(|a, b| b["sec"].as_f64().unwrap_or(0.0).partial_cmp(&a["sec"].as_f64().unwrap_or(0.0)).unwrap());
    let mut frequent: Vec<Value> = agg.iter().filter(|((n, _), x)| !n && x.0.len() >= 5).map(|((_, k), x)| row(k, x)).collect();
    frequent.sort_by(|a, b| b["sec"].as_f64().unwrap_or(0.0).partial_cmp(&a["sec"].as_f64().unwrap_or(0.0)).unwrap());
    frequent.truncate(12);
    json!({ "days": days, "named": named, "frequent": frequent })
}

/// 常去的地方（自己起过名的）：[{label, type, lat, lon, r, address?}]，设置页列出来改
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn list_place_labels() -> Vec<Value> {
    place_labels()
}

/// 改常去的地方里的一项：名字、种类、范围（米）、地址；名字空＝删掉。第一次写同 set_place_label（先把外部数据目录的抄过来）
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn edit_place_label(index: usize, label: String, kind: String, r: f64, address: Option<String>) -> Result<(), String> {
    let mut all = place_labels();
    if index >= all.len() {
        return Err("这一项不在了".into());
    }
    let label = label.trim().to_string();
    if label.is_empty() {
        all.remove(index);
    } else {
        all[index]["label"] = json!(label);
        all[index]["type"] = json!(kind);
        all[index]["r"] = json!(r.clamp(30.0, 1000.0));
        match address.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()) {
            Some(a) => all[index]["address"] = json!(a),
            None => {
                if let Some(o) = all[index].as_object_mut() {
                    o.remove("address");
                }
            }
        }
    }
    write_json(&labels_path(), &json!(all))
}

/// 某天的停留点（iPhone、手表记的位置），读法同 脚本/日数据包.py 的 read_places／load_stays：
/// 同一个停留点取最新一版（rev 大的）；可信程度低的（slc、手表）和更可信的重叠超过自身一半的丢掉；
/// 还没离开的算到下一个到达或现在；同一地点 15 分钟内并成一段，重叠的前一段截到后一段开始。没有任何停留点记录时返回 None
pub fn places_day(date: NaiveDate) -> Option<Value> {
    places_day_opt(date, true)
}

/// online＝false 只用缓存，不问高德（统计 30 天常去的地方时用，免得一次问几百次）
pub fn places_day_opt(date: NaiveDate, online: bool) -> Option<Value> {
    let (s, e) = day_bounds(date);
    let (s, e) = (s.with_timezone(&Utc), e.with_timezone(&Utc));
    // 到达在前一天的也读（在家过夜）
    let mut ev = device_events("iphone-", &["place.visit"], s - Duration::days(2), e);
    ev.extend(device_events("watch-", &["place.visit"], s - Duration::days(2), e));
    if ev.is_empty() {
        return None;
    }
    let mut last: BTreeMap<String, &Value> = BTreeMap::new();
    for x in &ev {
        let id = x["place_id"].as_str().unwrap_or("").to_string();
        let key = |v: &Value| (v["rev"].as_i64().unwrap_or(1), v["recorded_at"].as_str().unwrap_or("").to_string());
        if last.get(&id).map(|o| key(x) >= key(o)).unwrap_or(true) {
            last.insert(id, x);
        }
    }
    let now = Utc::now();
    let span = |v: &Value, open: DateTime<Utc>| -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let (a, d) = (ts_of(&v["arrive"]), ts_of(&v["depart"]));
        Some((a.or(d.map(|d| d - Duration::minutes(1)))?, d.unwrap_or(open)))
    };
    let rows: Vec<&Value> = last.values().copied().collect();
    let ranked: Vec<(u8, DateTime<Utc>, DateTime<Utc>)> = rows.iter().filter_map(|r| span(r, now).map(|(a, d)| (place_rank(r), a, d))).collect();
    let mut rows: Vec<&Value> = rows
        .into_iter()
        .filter(|r| {
            let Some((a, d)) = span(r, now) else { return false };
            // 到达和离开同一刻：作废的记录（比如误传上来又撤掉的），不算停留，也不拿来截断前一段
            if ts_of(&r["depart"]).is_some() && d <= a {
                return false;
            }
            let (rank, dur) = (place_rank(r), ((d - a).num_seconds() as f64).max(1.0));
            !ranked.iter().any(|(k, va, vd)| *k > rank && ((*vd).min(d) - (*va).max(a)).num_seconds() as f64 > dur / 2.0)
        })
        .collect();
    rows.sort_by_key(|r| ts_of(&r["arrive"]).or(ts_of(&r["depart"])));
    // 同一段两条记录（系统到访和位置变化）：时间用留下的那条，地名和候选用定位更准的那条（误差大的点容易落到隔壁区域）。同手机
    let all: Vec<&Value> = last.values().copied().collect();
    // 算同一段：到达相差 10 分钟内，离开也相差 10 分钟内（有一条没离开的不比离开）。没离开的记录不能按「算到现在」比重叠，会把后面几段都吞进来
    let same = |x: &Value, r: &Value| -> bool {
        let near = |a: Option<DateTime<Utc>>, b: Option<DateTime<Utc>>| match (a, b) {
            (Some(a), Some(b)) => (a - b).num_seconds().abs() <= 600,
            _ => true,
        };
        ts_of(&x["arrive"]).is_some() && ts_of(&r["arrive"]).is_some() && near(ts_of(&x["arrive"]), ts_of(&r["arrive"])) && near(ts_of(&x["depart"]), ts_of(&r["depart"]))
    };
    let precise = |r: &Value| -> Option<&Value> {
        let acc = r["acc"].as_f64().unwrap_or(f64::MAX);
        all.iter()
            .copied()
            .filter(|x| x["name"].is_string() && x["acc"].as_f64().unwrap_or(f64::MAX) < acc && same(x, r))
            .min_by(|x, y| x["acc"].as_f64().unwrap_or(f64::MAX).partial_cmp(&y["acc"].as_f64().unwrap_or(f64::MAX)).unwrap())
    };
    let picks = place_picks();
    let labels = place_labels();
    // 家在哪个城市（种类是家的那个起名地方）：别的城市的停留，名字前面加城市
    let home_city = labels.iter().find(|x| x["type"] == "home").and_then(|h| crate::amap::admin(h["lat"].as_f64()?, h["lon"].as_f64()?, 20.0, online)).and_then(|a| a["city"].as_str().map(String::from));
    let mut out: Vec<Map<String, Value>> = vec![];
    for (i, r) in rows.iter().enumerate() {
        let a = ts_of(&r["arrive"]).unwrap_or(s);
        let next = rows[i + 1..].iter().find_map(|x| ts_of(&x["arrive"]));
        // 还在那儿：没离开、后面也没有新的到达
        let open = ts_of(&r["depart"]).is_none() && next.is_none();
        let d = ts_of(&r["depart"]).unwrap_or_else(|| next.map(|n| n.min(now)).unwrap_or(now));
        let (a, d) = (a.max(s), d.min(e));
        if d <= a {
            continue;
        }
        let (lat, lon) = (r["lat"].as_f64().unwrap_or(0.0), r["lon"].as_f64().unwrap_or(0.0));
        let lab = labels
            .iter()
            .map(|x| (haversine(lat, lon, x["lat"].as_f64().unwrap_or(0.0), x["lon"].as_f64().unwrap_or(0.0)), x))
            .filter(|(m, x)| *m <= x["r"].as_f64().unwrap_or(PLACE_R))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // 名字：电脑上起的（labels.json）优先，没有就用手机上起的（随停留点传来的 label）
        let label = lab.and_then(|(_, x)| x["label"].as_str()).or(r["label"].as_str().filter(|x| !x.is_empty())).map(String::from);
        // 手机已经确认了店（名字三层「区 · 区域 · 店」）的不换；不然地名和候选取定位更准的那条
        let src = if r["name"].as_str().map(|n| n.split(" · ").count() >= 3).unwrap_or(false) { *r } else { precise(r).unwrap_or(r) };
        let mut name = src["name"].as_str().unwrap_or("").to_string();
        let mut guesses = src["guesses"].clone();
        // 在哪（电脑为准）：电脑用高德取完整分级（国家 › 省 › 市 › 区县 › 街道 › 区域），名字的前两层「区县 · 区域」用它；问不到才用手机传来的
        let (slat0, slon0) = (src["lat"].as_f64().unwrap_or(lat), src["lon"].as_f64().unwrap_or(lon));
        let admin = crate::amap::admin(slat0, slon0, src["acc"].as_f64().unwrap_or(50.0), online);
        // 手机上认定过店：10-21 15:00 起手机用高德取名，名字只写「区 · 区域」，三层的就是认定过的；之前收到的三层是苹果反查的门牌，不算
        let phone_pick = name.split(" · ").count() >= 3 && r["recorded_at"].as_str().map(|t| t >= "2026-10-21T07:00:00").unwrap_or(false);
        if let Some(ad) = &admin {
            let region2 = [ad["district"].as_str(), ad["area"].as_str()].iter().flatten().copied().collect::<Vec<_>>().join(" · ");
            if !region2.is_empty() {
                let shop = if phone_pick { name.split(" · ").last().map(String::from) } else { None };
                name = [Some(region2), shop].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            }
        } else if !phone_pick {
            name = name.split(" · ").take(2).collect::<Vec<_>>().join(" · ");
        }
        // 是哪家店（电脑为准）：在电脑上确认过的（picks.json）写进名字；手机上确认过的（名字已经三层）照用；
        // 不然电脑自己用高德算候选（起过名的地方不算），手机传来的候选只在高德问不到时兜底。
        // 第一名把握到 50% 以上（比其余候选加起来还像）就自动写进名字（30 米内确认过的店算分时加了很多，通常就是它）。没离开的记录（系统漏送离开）按下一段到达算待了多久
        let region = name.split(" · ").take(2).collect::<Vec<_>>().join(" · ");
        let id = r["place_id"].as_str().unwrap_or("");
        let remembered: Vec<Value> = picks["remembered"].as_array().cloned().unwrap_or_default();
        let picked: Vec<String> = picks_of(&picks["stays"][id]).iter().filter_map(|x| x["name"].as_str().map(String::from)).collect();
        let mut confirmed: Option<(Vec<String>, &str)> = if picked.is_empty() { None } else { Some((picked, "picked")) };
        if confirmed.is_none() && phone_pick {
            confirmed = Some((vec![name.split(" · ").last().unwrap_or("").to_string()], "phone"));
        }
        // 候选总是算（认定过的也算，好再加别的店）；自动认定只在一家都没认定时
        if label.is_none() {
            let (slat, slon) = (src["lat"].as_f64().unwrap_or(lat), src["lon"].as_f64().unwrap_or(lon));
            let mine = crate::amap::guesses_for(id, r["rev"].as_i64().unwrap_or(1), slat, slon, src["acc"].as_f64().unwrap_or(50.0), ts_of(&r["arrive"]), ts_of(&r["depart"]).or(next), &remembered, online);
            if let Some(g) = mine {
                guesses = json!(g);
            }
            if confirmed.is_none() {
                if let Some(top) = guesses.as_array().and_then(|g| g.first()).filter(|t| t["p"].as_f64().unwrap_or(0.0) >= 0.5) {
                    let shop = top["name"].as_str().unwrap_or("").to_string();
                    let known = remembered.iter().any(|x| x["name"] == shop.as_str() && haversine(lat, lon, x["lat"].as_f64().unwrap_or(0.0), x["lon"].as_f64().unwrap_or(0.0)) <= 30.0);
                    confirmed = Some((vec![shop], if known { "remembered" } else { "auto" }));
                }
            }
        }
        // 名字：区域 · 店（几家用顿号，三家以上写「等 N 家」）
        if let Some((shops, how)) = &confirmed {
            if *how != "phone" {
                let names: Vec<String> = shops.iter().map(|x| crate::amap::short(x)).collect();
                let list = if names.len() > 2 { format!("{}、{} 等 {} 家", names[0], names[1], names.len()) } else { names.join("、") };
                name = [region.as_str(), list.as_str()].iter().filter(|x| !x.is_empty()).copied().collect::<Vec<_>>().join(" · ");
            }
        }
        // 在做什么：候选的置信度按高德大类加起来，取最大的（同手机）
        let doing = guesses.as_array().and_then(|gs| {
            let mut m: BTreeMap<String, f64> = BTreeMap::new();
            for g in gs {
                let major = g["type"].as_str().unwrap_or("").split(';').next().unwrap_or("").to_string();
                if !major.is_empty() {
                    *m.entry(major).or_insert(0.0) += g["p"].as_f64().unwrap_or(0.0);
                }
            }
            m.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|(k, p)| json!({ "kind": k, "p": p }))
        });
        let show = label.clone().unwrap_or_else(|| name.clone());
        if let Some(prev) = out.last_mut() {
            let pd = ts_of(&prev["end_t"]).unwrap();
            if prev["name"] == show.as_str() && (a - pd).num_seconds() <= MERGE_PLACE {
                prev.insert("end_t".into(), json!(utc_iso(pd.max(d))));
                prev.insert("open".into(), json!(open));
                continue;
            }
            if a < pd {
                prev.insert("end_t".into(), json!(utc_iso(a)));
            }
        }
        let mut m = Map::new();
        m.insert("id".into(), r["place_id"].clone());
        m.insert("start".into(), json!(utc_iso(a)));
        m.insert("end_t".into(), json!(utc_iso(d)));
        m.insert("open".into(), json!(open));
        m.insert("arrive_known".into(), json!(ts_of(&r["arrive"]).map(|x| x >= s).unwrap_or(false)));
        // 真实的到达时刻（前一天到的也给，页面写「昨天 22:10」）
        m.insert("arrive".into(), ts_of(&r["arrive"]).map(|x| json!(utc_iso(x))).unwrap_or(Value::Null));
        m.insert("lat".into(), json!(lat));
        m.insert("lon".into(), json!(lon));
        m.insert("device".into(), r["device"].clone());
        m.insert("name".into(), json!(show));
        m.insert("place_name".into(), json!(name));
        m.insert("label".into(), json!(label));
        m.insert("type".into(), lab.and_then(|(_, x)| x["type"].as_str()).map(|t| json!(t)).unwrap_or(Value::Null));
        m.insert("source".into(), r["place_source"].clone());
        m.insert("acc".into(), src["acc"].clone());
        // 完整分级；和家不在一个城市时给 city_prefix，页面写在名字前面（「上海 · 静安寺」）
        let city = admin.as_ref().and_then(|a| a["city"].as_str()).map(String::from);
        m.insert("city_prefix".into(), match (&city, &home_city) { (Some(c), Some(h)) if c != h => json!(c), _ => Value::Null });
        m.insert("admin".into(), admin.unwrap_or(Value::Null));
        m.insert("guesses".into(), if label.is_none() { guesses } else { Value::Null });
        m.insert("doing".into(), if label.is_none() { doing.unwrap_or(Value::Null) } else { Value::Null });
        m.insert("shops".into(), confirmed.as_ref().map(|(s, _)| json!(s)).unwrap_or(json!([])));
        m.insert("confirmed".into(), confirmed.map(|(_, how)| json!(how)).unwrap_or(Value::Null));
        out.push(m);
    }
    // end：还在那儿（没离开、后面也没有新的到达）给 null，页面算到现在
    let visits: Vec<Value> = out
        .into_iter()
        .filter(|m| ts_of(&m["end_t"]) > ts_of(&m["start"]))
        .map(|mut m| {
            let end = if m["open"] == true { Value::Null } else { m["end_t"].clone() };
            m.remove("end_t");
            m.remove("open");
            m.insert("end".into(), end);
            Value::Object(m)
        })
        .collect();
    Some(json!({ "visits": visits }))
}

// ---------- 命令 ----------

fn pack_of(repo: &Option<PathBuf>, date: &str) -> Option<Value> {
    let p = repo.as_ref()?.join("日数据").join(format!("日数据_{}.json", date.replace('-', "")));
    fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}

fn summary_of(repo: &Option<PathBuf>, date: &str) -> Option<String> {
    // 以 app 自己写的（Beayy/daily/）为准；没有再看别的设备同步来的，再看外部目录里的（app 以前、外部脚本写的）
    let name = format!("日总结_{}.md", date.replace('-', ""));
    daily_md(&name).or_else(|| repo.as_ref().and_then(|r| fs::read_to_string(r.join("日总结").join(&name)).ok()))
}

/// 日／周／月总结：本机写的（Beayy/daily/<文件名>）为准；本机没有（手机、没开 app 的那几天）就用别的设备同步来的
/// （devices/<设备>/daily/<文件名>），几台都有取最后改的
pub fn daily_md(name: &str) -> Option<String> {
    if let Ok(t) = fs::read_to_string(default_root().join("daily").join(name)) {
        return Some(t);
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for dev in fs::read_dir(default_root().join("devices")).into_iter().flatten().flatten() {
        let p = dev.path().join("daily").join(name);
        if let Ok(t) = fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().map(|b| t > b.0).unwrap_or(true) {
                best = Some((t, p));
            }
        }
    }
    best.and_then(|(_, p)| fs::read_to_string(p).ok())
}

/// 区间 [[ISO, ISO]] 落在 [from, to) 里的秒数
fn spans_in(spans: &Value, from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    spans.as_array().into_iter().flatten().filter_map(|x| {
        let a = DateTime::parse_from_rfc3339(x[0].as_str()?).ok()?.with_timezone(&Utc);
        let b = DateTime::parse_from_rfc3339(x[1].as_str()?).ok()?.with_timezone(&Utc);
        Some((b.min(to) - a.max(from)).num_milliseconds().max(0) as f64 / 1000.0)
    }).sum()
}

/// 一段日子（周、月总结页用）：每天的在电脑前、手机、拿起、熬夜（这一晚＝当天 21:00 以后＋次日 0–5 点，同日总结）、停留点，
/// 加上整段的时间花在哪（项目、类型）。今天以后的日子不算
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_range(from: String, to: String) -> Result<Value, String> {
    let a = NaiveDate::parse_from_str(&from, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let z = NaiveDate::parse_from_str(&to, "%Y-%m-%d").map_err(|e| e.to_string())?.min(Local::now().date_naive());
    let repo = repo_path();
    let names = app_names();
    let ev = phone_events();
    let mut mine = self_segments();
    // 电脑：数据包的为准，没有（或只到一半）用自采事件实时算；连带次日，算凌晨
    let computer = |d: NaiveDate| -> Option<Value> {
        let ds = d.format("%Y-%m-%d").to_string();
        let pack = pack_of(&repo, &ds);
        let pc = pack.as_ref().map(|p| p["computer"].clone()).filter(|c| !c["active_sec"].is_null());
        match pc {
            Some(c) if pack.as_ref().map(|p| p["partial"] != true).unwrap_or(false) => Some(c),
            _ => live_computer(d).or(pc),
        }
    };
    let mut days = vec![];
    let mut segs: Vec<Value> = vec![];
    let mut d = a;
    while d <= z {
        let ds = d.format("%Y-%m-%d").to_string();
        let (s, e) = day_bounds(d);
        let (s, e) = (s.with_timezone(&Utc), e.with_timezone(&Utc));
        let c = computer(d);
        let n = computer(d + Duration::days(1));
        let ph = phone_day(ev.as_slice(), d, &names);
        let nph = phone_day(ev.as_slice(), d + Duration::days(1), &names);
        let stop = s + Duration::hours(HARD_STOP as i64);
        let late_c = c.as_ref().map(|c| c["late_sec"].as_f64().unwrap_or_else(|| spans_in(&c["spans"], stop, e))).unwrap_or(0.0);
        let dawn_c = n.as_ref().map(|n| {
            let h = &n["active_hours"];
            (0..5).map(|k| h[format!("{k:02}")].as_f64().unwrap_or(0.0)).sum::<f64>()
        }).unwrap_or(0.0);
        let late_p = ph.as_ref().map(|p| p["late_sec"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
        let dawn_p = nph.as_ref().map(|p| p["night_sec"].as_f64().unwrap_or(0.0)).unwrap_or(0.0);
        let visits = places_day_opt(d, false).map(|p| p["visits"].clone()).unwrap_or(json!([]));
        let health = crate::health::health_day(d);
        // 时间花在哪：同 get_day，自采摘要为准，数据包里同一段的不重复算
        let own = mine.remove(&ds).unwrap_or_default();
        let taken: HashSet<String> = own.iter().filter_map(|s| s["start"].as_str().map(String::from)).collect();
        if let Some(p) = pack_of(&repo, &ds) {
            segs.extend(codex_segments(&p, d).into_iter().filter(|s| !taken.contains(s["start"].as_str().unwrap_or(""))));
        }
        segs.extend(own);
        days.push(json!({
            "date": ds,
            "active_sec": c.as_ref().and_then(|c| c["active_sec"].as_i64()),
            "phone_sec": ph.as_ref().and_then(|p| p["total_sec"].as_i64()),
            "pickups": ph.as_ref().and_then(|p| p["pickups"].as_i64()),
            "night_sec": if c.is_some() || ph.is_some() { json!((late_c + dawn_c + late_p + dawn_p).round() as i64) } else { Value::Null },
            "visits": visits,
            "sleep_sec": health.as_ref().and_then(|h| h["sleep"]["asleep_sec"].as_i64()),
            "steps": health.as_ref().and_then(|h| h["activity"]["steps"].as_i64()),
        }));
        d += Duration::days(1);
    }
    Ok(json!({"from": from, "to": z.format("%Y-%m-%d").to_string(), "days": days, "work": work_of(&segs)}))
}

fn computer_hours(c: &Value) -> Value {
    if let Some(h) = c.get("active_hours").and_then(|v| v.as_object()) {
        return Value::Object(h.iter().map(|(k, v)| (k.clone(), json!(v.as_f64().unwrap_or(0.0).round() as i64))).collect());
    }
    let mut m = Map::new();
    if let Some(h) = c.get("hours").and_then(|v| v.as_object()) {
        for (k, v) in h {
            let s: f64 = v.as_object().map(|o| o.values().filter_map(|x| x.as_f64()).sum()).unwrap_or(0.0);
            m.insert(k.clone(), json!(s.round() as i64));
        }
    }
    Value::Object(m)
}

#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn list_days() -> Vec<Value> {
    let repo = repo_path();
    let names = app_names();
    let ev = phone_events();
    let mine = self_segments();
    let mut dates: HashSet<String> = mine.keys().cloned().collect();
    if let Some(Ok(it)) = repo.as_ref().map(|r| fs::read_dir(r.join("日数据"))) {
        for e in it.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(d) = n.strip_prefix("日数据_").and_then(|s| s.strip_suffix(".json")) {
                if d.len() == 8 {
                    dates.insert(format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]));
                }
            }
        }
    }
    // 没有摘要（没配模型）也没有数据包的日子：有电脑的原始分段就列上，算得出读数才留
    let raw: HashSet<String> = raw_days().difference(&dates).cloned().collect();
    dates.extend(raw.iter().cloned());
    let mut dates: Vec<String> = dates.into_iter().collect();
    dates.sort_by(|a, b| b.cmp(a));
    dates
        .iter()
        .filter_map(|date| {
            let pack = pack_of(&repo, date);
            let md = summary_of(&repo, date);
            let nd = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
            let phone = phone_day(&ev, nd, &names);
            let mut c = pack.as_ref().map(|p| p["computer"].clone()).unwrap_or(Value::Null);
            // 还没有数据包的那天（通常是今天）：电脑用自采事件实时算
            if c.get("active_sec").map(Value::is_null).unwrap_or(true) {
                if let Some(l) = live_computer(nd) {
                    c = l;
                }
            }
            if raw.contains(date) && c.is_null() && phone.is_none() {
                return None;
            }
            let pp = pack.as_ref().map(|p| p["phone"].clone()).unwrap_or(Value::Null);
            Some(json!({
                "date": date,
                "line": md.as_deref().and_then(|m| section(m, "今天的一句话")),
                "active_sec": c.get("active_sec").cloned().unwrap_or(Value::Null),
                "phone_sec": phone.as_ref().map(|p| p["total_sec"].clone()).unwrap_or_else(|| pp.get("total_sec").cloned().unwrap_or(Value::Null)),
                "hc": if c.is_null() { json!({}) } else { computer_hours(&c) },
                "hp": phone.as_ref().map(|p| p["hours"].clone()).unwrap_or_else(|| pp.get("hours").cloned().unwrap_or(json!({}))),
            }))
        })
        .collect()
}

/// 电脑原始分段（本机的和同步来的）落在哪些本地日期
fn raw_days() -> HashSet<String> {
    beayy_summary::Summarizer::new(default_root())
        .segment_names()
        .iter()
        .filter_map(|n| chrono::NaiveDateTime::parse_from_str(n, "%Y-%m-%dT%H-%M-%SZ").ok())
        .map(|t| Utc.from_utc_datetime(&t).with_timezone(&Local).format("%Y-%m-%d").to_string())
        .collect()
}

#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_day(date: String) -> Value {
    let repo = repo_path();
    let Ok(nd) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") else { return Value::Null };
    let pack = pack_of(&repo, &date);
    let own = self_segments().remove(&date).unwrap_or_default();
    let live = if pack.as_ref().map(|p| p["computer"]["active_sec"].is_null() || p["partial"] == true).unwrap_or(true) || date == Local::now().format("%Y-%m-%d").to_string() {
        live_computer(nd)
    } else {
        None
    };
    let taken: HashSet<String> = own.iter().filter_map(|s| s["start"].as_str().map(String::from)).collect();
    let mut segs: Vec<Value> = pack.as_ref().map(|p| codex_segments(p, nd)).unwrap_or_default();
    segs.retain(|s| !taken.contains(s["start"].as_str().unwrap_or("")));
    segs.extend(own);
    // 没有摘要的 10 分钟（没配模型、额度用完、还没轮到）按原始记录补一行：用了哪些 app、各多久
    let covered: Vec<(String, String)> = segs.iter().filter_map(|s| Some((s["start"].as_str()?.to_string(), s["end"].as_str()?.to_string()))).collect();
    segs.extend(raw_segments(nd).into_iter().filter(|r| {
        let (a, z) = (r["start"].as_str().unwrap_or(""), r["end"].as_str().unwrap_or(""));
        !covered.iter().any(|(s, e)| s.as_str() < z && a < e.as_str())
    }));
    segs.sort_by(|a, b| a["start"].as_str().cmp(&b["start"].as_str()));
    let work = work_of(&segs);
    let hours = rollups_of(nd);
    // 别的电脑「不在电脑前」的段不显示（你在这台用着的时候，那台闲着不用列出来）；有几台电脑的摘要，页面就给每行标上是哪台
    let me = beayy_core::local_device();
    segs.retain(|s| s["device"].as_str().map(|d| d == me).unwrap_or(true) || s["title"] != beayy_summary::IDLE_TITLE);
    let computers: BTreeSet<String> = segs.iter().filter_map(|s| s["device"].as_str().map(String::from)).collect();
    let segs = merge_idle(segs);
    json!({
        "date": date,
        "pack": pack,
        "summary_md": summary_of(&repo, &date),
        "live": live,
        "work": work,
        "hours": hours,
        "segments": segs,
        "computers": computers,
        "device_names": beayy_sync::device_names(&beayy_core::data_root()),
        "phone": phone_day(phone_events().as_slice(), nd, &app_names()),
        // 手机记录从哪来：有安卓设备就是安卓自己传的（页面不提「等苹果传来」）
        "phone_kind": if fs::read_dir(default_root().join("devices")).map(|r| r.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("android-"))).unwrap_or(false) { "android" } else { "iphone" },
        "speech": speech_day(nd),
        "places": places_day(nd),
        "health": crate::health::health_day(nd),
    })
}

/// 时间花在哪：10 分钟摘要的 parts（类型、项目、占比）× 这段在电脑前的秒数，按项目、按类型累计；项目名按别名表归并
fn work_of(segs: &[Value]) -> Value {
    // 一级 → 二级 → 秒
    let mut acts: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    let mut doms: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    let (mut tagged, mut untagged) = (0f64, 0f64);
    for sg in segs {
        let sec = sg["active_sec"].as_f64().unwrap_or(0.0);
        let parts = sg["parts"].as_array().cloned().unwrap_or_default();
        if parts.is_empty() {
            untagged += sec;
            continue;
        }
        tagged += sec;
        for x in parts {
            let v = sec * x["share"].as_f64().unwrap_or(0.0);
            let act = x["activity"].as_str().unwrap_or("其他");
            let group = beayy_summary::activity_group(act);
            let child = if act == group && act != "其他" { "未细分" } else { act };
            *acts.entry(group.to_string()).or_default().entry(child.to_string()).or_default() += v;
            let pj = x["project"].as_str().unwrap_or("");
            let dom = x["domain"].as_str().unwrap_or("工作");
            *doms.entry(dom.to_string()).or_default().entry(if pj.is_empty() { "未归项目".into() } else { pj.to_string() }).or_default() += v;
        }
    }
    let tree = |m: BTreeMap<String, BTreeMap<String, f64>>| {
        let mut v: Vec<Value> = m
            .into_iter()
            .map(|(k, kids)| {
                let total: f64 = kids.values().sum();
                let mut c: Vec<(String, f64)> = kids.into_iter().filter(|x| x.1 >= 30.0).collect();
                c.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                json!({"name": k, "sec": total.round() as i64, "children": c.into_iter().map(|(n, s)| json!({"name": n, "sec": s.round() as i64})).collect::<Vec<_>>()})
            })
            .filter(|x| x["sec"].as_i64().unwrap_or(0) >= 30)
            .collect();
        v.sort_by(|a, b| b["sec"].as_i64().cmp(&a["sec"].as_i64()));
        v
    };
    json!({"activities": tree(acts), "domains": tree(doms), "tagged_sec": tagged.round() as i64, "untagged_sec": untagged.round() as i64})
}

/// 这一天的小时摘要（Beayy/summaries-1h/），项目名按别名表归并
fn rollups_of(date: NaiveDate) -> Vec<Value> {
    let (s, e) = day_bounds(date);
    let al = beayy_summary::Summarizer::new(default_root()).aliases();
    let mut out = vec![];
    for f in fs::read_dir(default_root().join("summaries-1h")).map(|r| r.filter_map(|x| x.ok()).map(|x| x.path()).collect::<Vec<_>>()).unwrap_or_default() {
        let n = f.file_name().unwrap().to_string_lossy().to_string();
        let Ok(t) = chrono::NaiveDateTime::parse_from_str(n.get(..19).unwrap_or(""), "%Y-%m-%dT%H-%M-%S") else { continue };
        let t = t.and_utc();
        if t < s.with_timezone(&Utc) || t >= e.with_timezone(&Utc) {
            continue;
        }
        let fm = beayy_summary::front(&fs::read_to_string(&f).unwrap_or_default());
        let parts: Vec<Value> = fm
            .get("parts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|mut x| {
                if let Some(c) = x["project"].as_str().and_then(|p| al.get(p)) {
                    x["project"] = json!(c);
                }
                x
            })
            .collect();
        out.push(json!({
            "start": utc_iso(t), "end": utc_iso(t + Duration::hours(1)),
            "title": fm.get("title"), "description": fm.get("description"),
            "active_sec": fm.get("active_sec"), "parts": parts,
        }));
    }
    out.sort_by(|a, b| a["start"].as_str().cmp(&b["start"].as_str()));
    out
}

/// 连续的「不在电脑前」段（自采、首尾相接）合成一段，kind＝idle
fn merge_idle(segs: Vec<Value>) -> Vec<Value> {
    let idle = |s: &Value| s["source"] == "beayy" && s["title"] == beayy_summary::IDLE_TITLE;
    let mut out: Vec<Value> = Vec::new();
    for s in segs {
        if idle(&s) {
            if let Some(last) = out.last_mut() {
                if last["kind"] == "idle" && last["end"] == s["start"] {
                    last["end"] = s["end"].clone();
                    continue;
                }
            }
            out.push(json!({"start": s["start"], "end": s["end"], "kind": "idle", "source": "beayy",
                "title": beayy_summary::IDLE_TITLE, "description": "", "memory": "", "context": [], "recording": ""}));
        } else {
            out.push(s);
        }
    }
    out
}

fn rules_path() -> PathBuf {
    default_root().join("rules.json")
}

#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_rules() -> Value {
    fs::read_to_string(rules_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!([]))
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn save_rules(rules: Value) -> Result<(), String> {
    fs::create_dir_all(default_root()).map_err(|e| e.to_string())?;
    fs::write(rules_path(), serde_json::to_string_pretty(&rules).unwrap()).map_err(|e| e.to_string())
}


/// settings.json 的键沿用 Python 摘要脚本的叫法（summary_model、daily_model、blacklist），对网页换成 COMMANDS.md 的叫法
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_settings() -> Value {
    let s = read_settings_raw();
    let get = |k: &str, d: &str| s.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).unwrap_or(d).to_string();
    json!({
        "gateway": get("gateway", beayy_core::DEFAULT_GATEWAY),
        "model_segment": get("summary_model", beayy_core::DEFAULT_MODEL),
        "model_daily": get("daily_model", &get("summary_model", beayy_core::DEFAULT_MODEL)),
        "default_gateway": beayy_core::DEFAULT_GATEWAY,
        "default_model": beayy_core::DEFAULT_MODEL,
        "transcribe_server": beayy_remote::cloud::transcribe_server(),
        "default_transcribe_server": beayy_core::DEFAULT_TRANSCRIBE,
        "speaker_name": beayy_remote::cloud::speaker_name(),
        "sync_server": crate::sync::server(),
        "sync_mode": crate::sync::mode(),
        "sync_folder": crate::sync::folder(),
        // 选同步文件夹时给的默认：iCloud 云盘里的 Beayy 文件夹（只有 Mac 有）
        "icloud_folder": if cfg!(target_os = "macos") { std::env::var("HOME").map(|h| format!("{h}/Library/Mobile Documents/com~apple~CloudDocs/Beayy")).unwrap_or_default() } else { String::new() },
        "default_sync_server": beayy_sync::DEFAULT_SERVER,
        "repo_path": get("repo_path", ""),
        "onboarded": s.get("onboarded").and_then(Value::as_bool).unwrap_or(false),
        // 默认值和采集端一致（beayy-capture 的 DEFAULT_BLOCKLIST）：Windows 按 exe 文件名
        "blocklist": s.get("blacklist").cloned().unwrap_or(if cfg!(target_os = "windows") { json!(["1password"]) } else { json!(["com.1password.1password"]) }),
        "has_api_key": beayy_summary::api_key_source().is_some(),
        // 模型用官方的（按账号开通，经同步服务代调）还是自己的网关；没选过的：有自己的密钥算自己的，否则官方
        "model_mode": s.get("model_mode").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| if beayy_summary::api_key_source().is_some() { "custom".into() } else { "official".into() }),
        "api_key_source": beayy_summary::api_key_source(),
        "relay_server": get("relay_server", ""),
        "has_relay_token": beayy_core::secret_get(beayy_remote::SECRET).is_some(),
        "has_amap_key": beayy_core::secret_get(crate::amap::SECRET).is_some(),
        "map_mode": if crate::amap::official() { "official" } else { "custom" },
        "has_amap_js": beayy_core::secret_get("amap_js_key").is_some() && beayy_core::secret_get("amap_js_code").is_some(),
        // 地图服务：amap（高德，默认）／osm（OpenStreetMap，免费、不用 Key）
        "map_provider": crate::amap::provider(),
        // AI 总结的语言：zh／en，空＝跟随界面语言（ui_lang 是界面启动时报的）
        "summary_language": get("summary_language", ""),
        "ui_lang": get("ui_lang", ""),
        // 棒喝的力度：off／low／medium／high／max／ultra，日总结、周总结多写一段狠话，默认 off（0.2.46 存的 true 算 medium）
        "roast": match s.get("roast") {
            Some(Value::Bool(true)) => "medium",
            Some(Value::String(k)) if ["low", "medium", "high", "max", "ultra"].contains(&k.as_str()) => k.as_str(),
            _ => "off",
        },
    })
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn save_settings(settings: Value) -> Result<(), String> {
    let mut s = read_settings_raw();
    let map = [("gateway", "gateway"), ("model_segment", "summary_model"), ("model_daily", "daily_model"), ("repo_path", "repo_path"), ("blocklist", "blacklist"), ("onboarded", "onboarded"), ("relay_server", "relay_server"), ("sync_server", "sync_server"), ("transcribe_server", "transcribe_server"), ("speaker_name", "speaker_name"), ("model_mode", "model_mode"), ("map_mode", "map_mode"), ("sync_mode", "sync_mode"), ("sync_folder", "sync_folder"), ("map_provider", "map_provider"), ("summary_language", "summary_language"), ("ui_lang", "ui_lang"), ("roast", "roast")];
    for (from, to) in map {
        if let Some(mut v) = settings.get(from).cloned() {
            // 同步服务器、转写服务填的就是官方的，存成空（官方地址以后变了跟着变）
            if (from == "sync_server" && v.as_str().map(|x| x.trim_end_matches('/')) == Some(beayy_sync::DEFAULT_SERVER))
                || (from == "transcribe_server" && v.as_str().map(|x| x.trim_end_matches('/')) == Some(beayy_core::DEFAULT_TRANSCRIBE))
            {
                v = json!("");
            }
            s.insert(to.into(), v);
        }
    }
    fs::create_dir_all(default_root()).map_err(|e| e.to_string())?;
    fs::write(settings_path(), serde_json::to_string_pretty(&Value::Object(s)).unwrap()).map_err(|e| e.to_string())
}

/// 存密钥：macOS 进钥匙串，Windows 写数据目录里的文件（见 beayy_summary::store_api_key）；不回传网页
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_api_key(key: String) -> Result<(), String> {
    beayy_summary::store_api_key(key.trim())
}

/// 存录音服务器（手表对话、iPhone 停留点）的口令：macOS 进钥匙串，Windows 写数据目录；不回传网页。空字符串＝删掉
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_relay_token(token: String) -> Result<(), String> {
    beayy_core::secret_set(beayy_remote::SECRET, token.trim())
}

// ---------- 删除记录 ----------

/// 删掉本机电脑事件里某时刻之后的记录，连同这些分段的 10 分钟摘要（摘要下一轮按剩下的事件重写）。
/// 只动本机这台电脑的事件（同步来的别的设备不动）；iPhone 记录、已经归档进仓库 raw/ 的不动。返回删掉的条数
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn delete_since(minutes: Option<i64>, today: Option<bool>) -> Result<usize, String> {
    let from: DateTime<Utc> = if today == Some(true) {
        day_bounds(Local::now().date_naive()).0.with_timezone(&Utc)
    } else {
        Utc::now() - Duration::minutes(minutes.unwrap_or(60).clamp(1, 24 * 60))
    };
    let first_seg = from - Duration::minutes(10);
    let mut removed = 0;
    let mut touched: Vec<String> = vec![];
    let Ok(devs) = fs::read_dir(default_root().join("devices")) else { return Ok(0) };
    let me = beayy_core::local_device();
    for dev in devs.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy() == me.as_str()) {
        let Ok(segs) = fs::read_dir(dev.path().join("segments")) else { continue };
        for seg in segs.filter_map(|e| e.ok()) {
            let name = seg.file_name().to_string_lossy().to_string();
            let Ok(t) = chrono::NaiveDateTime::parse_from_str(&name, "%Y-%m-%dT%H-%M-%SZ") else { continue };
            if t.and_utc() < first_seg {
                continue;
            }
            let f = seg.path().join("events.jsonl");
            let Ok(bytes) = fs::read(&f) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            let mut keep = String::new();
            let mut n = 0;
            for line in text.lines() {
                let t = serde_json::from_str::<Value>(line).ok().and_then(|e| e["timestamp"].as_str().and_then(|x| DateTime::parse_from_rfc3339(x).ok()));
                match t {
                    Some(t) if t.with_timezone(&Utc) >= from => n += 1,
                    _ => {
                        keep.push_str(line);
                        keep.push('\n');
                    }
                }
            }
            if n > 0 {
                // 原地重写（不能换文件：采集线程开着这个文件在追加）
                fs::write(&f, keep).map_err(|e| e.to_string())?;
                removed += n;
                touched.push(name);
            }
        }
    }
    if let Ok(files) = fs::read_dir(default_root().join("summaries")) {
        for f in files.filter_map(|e| e.ok()) {
            let n = f.file_name().to_string_lossy().to_string();
            if touched.iter().any(|t| n.starts_with(t.trim_end_matches('Z'))) {
                let _ = fs::remove_file(f.path());
            }
        }
    }
    Ok(removed)
}

// ---------- 设备 ----------

/// 设备详情「事件」页签：某台设备最近的原始事件，新的在前。从最新的分段往回读，读够 limit 条（默认 200、最多 1000）
/// 或往回翻了 48 个分段（8 小时）就停，不全量扫。before＝只要这个时刻以前的（「加载更早」接着读）；
/// types＝只要这些种类，以「.」结尾的按前缀算（keyboard. 包含 keyboard.text_input、keyboard.submit……）。
/// 打的字、界面文字原样返回（采集时密码框已经打码），只给本机界面看，不进网站
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn recent_events(device: String, before: Option<String>, limit: Option<usize>, types: Option<Vec<String>>) -> Result<Value, String> {
    if device.is_empty() || !device.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err("设备编号不对".into());
    }
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    let types = types.unwrap_or_default();
    let want = |t: &str| types.is_empty() || types.iter().any(|x| if x.ends_with('.') { t.starts_with(x.as_str()) } else { t == x });
    let before_t = before.as_deref().and_then(|b| DateTime::parse_from_rfc3339(b).ok()).map(|t| t.with_timezone(&Utc));
    let top = before_t.map(beayy_core::segment_key);
    let Ok(rd) = fs::read_dir(default_root().join("devices").join(&device).join("segments")) else {
        return Ok(json!({ "events": [], "more": false }));
    };
    let mut segs: Vec<(String, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .filter(|(n, _)| top.as_ref().map(|t| n <= t).unwrap_or(true))
        .collect();
    segs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<(DateTime<Utc>, Value)> = vec![];
    let mut scanned = 0;
    for (_, path) in &segs {
        if out.len() >= limit || scanned >= 48 {
            break;
        }
        scanned += 1;
        let Ok(bytes) = fs::read(path.join("events.jsonl")) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let Ok(e) = serde_json::from_str::<Value>(line) else { continue };
            if !want(e["event_type"].as_str().unwrap_or("")) {
                continue;
            }
            let Some(t) = ts_of(&e["timestamp"]) else { continue };
            if before_t.map(|b| t >= b).unwrap_or(false) {
                continue;
            }
            out.push((t, e));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    // 还有更早的：这次截掉了一些，或者没翻完的分段
    let more = out.len() > limit || scanned < segs.len();
    out.truncate(limit);
    let events: Vec<Value> = out.into_iter().map(|(_, e)| e).collect();
    Ok(json!({ "events": events, "more": more }))
}

/// 数据目录里的每台设备（devices/<设备>/）：种类、是不是本机、记录从哪天开始、最近一条、条数、占用空间。缓存 60 秒
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn list_devices() -> Vec<Value> {
    let mut g = DEV_CACHE.lock().unwrap();
    if let Some((t, v)) = g.as_ref() {
        if t.elapsed().as_secs() < 60 {
            return v.clone();
        }
    }
    let v = read_devices();
    *g = Some((std::time::Instant::now(), v.clone()));
    v
}

static DEV_CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<Value>)>> = std::sync::Mutex::new(None);

/// 给设备改名（空＝改回默认名），会同步到别的设备
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn rename_device(id: String, name: String) -> Result<(), String> {
    beayy_sync::rename_device(&beayy_core::data_root(), &id, &name)?;
    *DEV_CACHE.lock().unwrap() = None;
    Ok(())
}

fn read_devices() -> Vec<Value> {
    let custom = beayy_sync::device_names(&beayy_core::data_root());
    let me = beayy_core::local_device();
    let synced = beayy_sync::remote_set(&beayy_core::data_root());
    let mut out = vec![];
    let Ok(devs) = fs::read_dir(default_root().join("devices")) else { return out };
    let mut devs: Vec<_> = devs.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).collect();
    devs.sort_by_key(|e| e.file_name());
    for dev in devs {
        let id = dev.file_name().to_string_lossy().to_string();
        // 手机版自己算出的那份（-m）和它的同步客户端（iosapp-）不是设备，不列；
        // 账号的配置、声纹（account）也不是设备：0.2.14 以前会把它们当分段下载下来
        if id.ends_with("-m") || id.starts_with("iosapp-") || id == "account" {
            continue;
        }
        let kind = id.split('-').next().unwrap_or("other").to_string();
        let mut segs: Vec<PathBuf> = fs::read_dir(dev.path().join("segments")).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default();
        segs.sort();
        let (mut events, mut bytes) = (0u64, 0u64);
        for s in &segs {
            if let Ok(b) = fs::read(s.join("events.jsonl")) {
                bytes += b.len() as u64;
                if kind != "iphone" {
                    events += b.iter().filter(|c| **c == b'\n').count() as u64;
                }
            }
        }
        let seg_time = |p: &PathBuf| {
            chrono::NaiveDateTime::parse_from_str(&p.file_name().unwrap().to_string_lossy(), "%Y-%m-%dT%H-%M-%SZ").ok().map(|t| t.and_utc())
        };
        let (mut first, mut last) = (segs.first().and_then(seg_time), segs.last().and_then(seg_time).map(|t| t + Duration::minutes(10)));
        if kind == "iphone" {
            // iPhone 的分段按事件发生时刻放，按事件本身算更准
            let ev = phone_events();
            events = ev.len() as u64;
            first = ev.first().and_then(|x| Utc.timestamp_opt(x.t as i64, 0).single());
            last = ev.last().and_then(|x| Utc.timestamp_opt(x.t as i64, 0).single());
        }
        let default_name = match kind.as_str() {
            "mac" => format!("Mac · {}", &id[4..]),
            "iphone" => format!("iPhone · {}", &id[7..]),
            "win" => format!("Windows · {}", &id[4..]),
            "watch" => format!("Apple Watch · {}", &id[6..]),
            "android" => format!("Android · {}", &id[8..]),
            _ => id.clone(),
        };
        let name = custom.get(&id).cloned().unwrap_or_else(|| default_name.clone());
        out.push(json!({
            "synced": synced.contains(&id),
            "id": id, "kind": kind, "name": name, "default_name": default_name, "custom": custom.contains_key(&id), "this": id == me,
            "first": first.map(utc_iso), "last": last.map(utc_iso),
            "events": events, "bytes": bytes,
        }));
    }
    // iPhone、手表的编号是随机的，同类只有一台时默认名不带它
    for kind in ["iphone", "watch"] {
        if out.iter().filter(|d| d["kind"] == kind).count() == 1 {
            let d = out.iter_mut().find(|d| d["kind"] == kind).unwrap();
            let short = if kind == "iphone" { "iPhone" } else { "Apple Watch" };
            d["default_name"] = json!(short);
            if !d["custom"].as_bool().unwrap_or(false) {
                d["name"] = json!(short);
            }
        }
    }
    // 本机排第一
    out.sort_by_key(|d| !d["this"].as_bool().unwrap_or(false));
    out
}

// ---------- 头像和名称 ----------

/// 本机存的显示名称和头像（settings.json 的 profile_*）；没设名称时用账号邮箱 @ 前面那段，再没有就「我」
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_profile() -> Value {
    let s = read_settings_raw();
    let email = beayy_sync::load(&beayy_core::data_root()).and_then(|a| a.email);
    let name = s.get("profile_name").and_then(Value::as_str).filter(|x| !x.is_empty()).map(String::from)
        .or_else(|| email.as_ref().map(|e| e.split('@').next().unwrap_or("").to_string()))
        .unwrap_or_else(|| "我".into());
    json!({"name": name, "avatar": s.get("profile_avatar").cloned().unwrap_or(Value::Null), "email": email})
}

pub fn write_profile(name: &str, avatar: Option<&str>, updated: i64) -> Result<(), String> {
    let mut s = read_settings_raw();
    s.insert("profile_name".into(), json!(name.trim()));
    s.insert("profile_avatar".into(), avatar.map(|a| json!(a)).unwrap_or(Value::Null));
    s.insert("profile_updated".into(), json!(updated));
    fs::create_dir_all(default_root()).map_err(|e| e.to_string())?;
    fs::write(settings_path(), serde_json::to_string_pretty(&Value::Object(s)).unwrap()).map_err(|e| e.to_string())
}

pub fn profile_updated() -> i64 {
    read_settings_raw().get("profile_updated").and_then(Value::as_i64).unwrap_or(0)
}

/// 改名称和头像：先存本机；登录着就同时存到账号上（换设备登录也是这个头像和名字）
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn save_profile(name: String, avatar: Option<String>) -> Result<(), String> {
    if let Some(a) = &avatar {
        if !a.starts_with("data:image/") || a.len() > 300_000 {
            return Err("头像要是图片，且压缩后不超过 300KB".into());
        }
    }
    let name: String = name.trim().chars().take(40).collect();
    let root = beayy_core::data_root();
    let updated = if beayy_sync::load(&root).map(|a| a.email.is_some()).unwrap_or(false) {
        beayy_sync::set_profile(&root, &name, avatar.as_deref())?
    } else {
        Utc::now().timestamp_millis()
    };
    write_profile(&name, avatar.as_deref(), updated)
}

// ---------- 项目（领域 · 项目） ----------

/// 所有出现过的项目：规范名、领域、累计时长、最近一次、被合并进来的别名
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn list_projects() -> Vec<Value> {
    let sm = beayy_summary::Summarizer::new(default_root());
    let aliases = sm.aliases();
    let mut agg: BTreeMap<String, (String, f64, String)> = BTreeMap::new();
    for segs in self_segments().into_values() {
        for sg in segs {
            let sec = sg["active_sec"].as_f64().unwrap_or(0.0);
            for x in sg["parts"].as_array().cloned().unwrap_or_default() {
                let p = x["project"].as_str().unwrap_or("");
                if p.is_empty() {
                    continue;
                }
                let e = agg.entry(p.to_string()).or_insert((x["domain"].as_str().unwrap_or("工作").to_string(), 0.0, String::new()));
                e.1 += sec * x["share"].as_f64().unwrap_or(0.0);
                let st = sg["start"].as_str().unwrap_or("").to_string();
                if st > e.2 {
                    e.2 = st;
                }
            }
        }
    }
    let mut out: Vec<Value> = agg
        .into_iter()
        .map(|(name, (domain, sec, last))| {
            let als: Vec<&String> = aliases.iter().filter(|(_, v)| **v == name).map(|(k, _)| k).collect();
            json!({"name": name, "domain": domain, "sec": sec.round() as i64, "last": last, "aliases": als})
        })
        .collect();
    out.sort_by(|a, b| b["sec"].as_i64().cmp(&a["sec"].as_i64()));
    out
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn merge_project(from: String, into: String) -> Result<(), String> {
    beayy_summary::Summarizer::new(default_root()).merge_project(&from, &into)
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_project_domain(project: String, domain: String) -> Result<(), String> {
    beayy_summary::Summarizer::new(default_root()).set_project_domain(&project, &domain)
}

/// 首次引导里「测试一下」：用现在的网关、模型、密钥问一句
/// 官方服务（模型、转写、地图）在这个账号上开通了没有、本月各用了多少：
/// {plan: null|"official"|"stopped", month, llm: {used, quota}, asr: {used, quota}（秒）, map: {used, quota}（次）}；没登录返回 {error}
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn llm_status() -> Value {
    match beayy_sync::account(&beayy_core::data_root()) {
        Ok(a) => a.get("services").cloned().unwrap_or(json!({"plan": null})),
        Err(e) => json!({"error": e}),
    }
}

/// 模型用量（设置 → 连接 → 模型 → 用量）：最近 30 天每天的输入、输出 token，按用途分。
/// 记在 logs/llm_usage.jsonl（0.2.26 起每次调模型记一行）；那以前的只有 10 分钟摘要，从 summarizer.log 的「用量 a+b token」补
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn llm_usage() -> Value {
    let logs = default_root().join("logs");
    let since = Local::now().date_naive() - Duration::days(29);
    // 日期 → 用途 → [输入, 输出, 次数]
    let mut days: BTreeMap<String, BTreeMap<String, [i64; 3]>> = BTreeMap::new();
    let mut add = |t: DateTime<Utc>, kind: &str, i: i64, o: i64| {
        let d = t.with_timezone(&Local).date_naive();
        if d >= since {
            let x = days.entry(d.format("%Y-%m-%d").to_string()).or_default().entry(kind.to_string()).or_default();
            x[0] += i;
            x[1] += o;
            x[2] += 1;
        }
    };
    let mut first: Option<DateTime<Utc>> = None;
    for line in fs::read_to_string(logs.join("llm_usage.jsonl")).unwrap_or_default().lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let Some(t) = v["at"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc)) else { continue };
        first = Some(first.map(|f| f.min(t)).unwrap_or(t));
        add(t, v["kind"].as_str().unwrap_or("other"), v["in"].as_i64().unwrap_or(0), v["out"].as_i64().unwrap_or(0));
    }
    let re = Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2})Z：.*用量 (\d+)\+(\d+) token").unwrap();
    for line in fs::read_to_string(logs.join("summarizer.log")).unwrap_or_default().lines() {
        let Some(c) = re.captures(line) else { continue };
        let Ok(t) = chrono::NaiveDateTime::parse_from_str(&c[1], "%Y-%m-%dT%H-%M-%S").map(|t| t.and_utc()) else { continue };
        if first.map(|f| t < f).unwrap_or(true) {
            add(t, "segment", c[2].parse().unwrap_or(0), c[3].parse().unwrap_or(0));
        }
    }
    let list: Vec<Value> = (0..30).map(|k| {
        let d = (since + Duration::days(k)).format("%Y-%m-%d").to_string();
        let m = days.get(&d).cloned().unwrap_or_default();
        json!({
            "date": d,
            "in": m.values().map(|x| x[0]).sum::<i64>(),
            "out": m.values().map(|x| x[1]).sum::<i64>(),
            "kinds": m.iter().map(|(k, x)| (k.clone(), json!({"in": x[0], "out": x[1], "calls": x[2]}))).collect::<Map<_, _>>(),
        })
    }).collect();
    json!({"days": list, "official": read_settings_raw().get("model_mode").and_then(Value::as_str) == Some("official")})
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn test_model() -> Result<(), String> {
    beayy_summary::Summarizer::new(default_root()).ask("只返回一个 JSON 对象：{\"ok\": true}", "测试连接").map(|_| ())
}

/// 还没做过首次引导（主窗口启动时直接打开）
pub fn needs_onboarding() -> bool {
    !read_settings_raw().get("onboarded").and_then(Value::as_bool).unwrap_or(false)
}

/// 给 MCP 搜索用：全部 10 分钟摘要，按本地日期分组
pub fn self_segments_pub() -> HashMap<String, Vec<Value>> {
    self_segments()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 没配模型（没有 10 分钟摘要）的日子也要列出来；旧版本存下的空缓存要重算
    #[test]
    fn raw_only_day_listed() {
        let home = std::env::temp_dir().join(format!("beayy-data-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        std::env::set_var("HOME", &home);
        std::env::set_var("APPDATA", &home);
        let day = Local::now().date_naive() - Duration::days(1);
        let t = Local.from_local_datetime(&day.and_hms_opt(20, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
        let log = beayy_core::EventLog::new(default_root(), "win-test");
        let ev = |k: &str| json!({"state": "active", "app_name": "Code", "bundle_id": "code.exe", "k": k}).as_object().unwrap().clone();
        log.write("app.activated", ev("a"), Some(t));
        for i in 1..=5 {
            log.write("heartbeat", ev("h"), Some(t + Duration::minutes(i)));
        }
        fs::create_dir_all(default_root().join("daily")).unwrap();
        fs::write(default_root().join("daily").join(format!("stats_{day}.json")), "null").unwrap();
        let days = list_days();
        let row = days.iter().find(|d| d["date"] == day.to_string()).expect("昨天应该在日期列表里");
        assert!(row["active_sec"].as_i64().unwrap_or(0) >= 300, "{row}");
        let _ = fs::remove_dir_all(&home);
    }
}
