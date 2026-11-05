//! 录音和停留点走账号同步（方案见 报告/方案_20261021_录音与存储.md）：手机把加密的录音（`audio.<段 id>`）、
//! 每天的停留点（`places.<日>`）传到同步服务；电脑每 2 分钟一轮（停留点早点到）；
//! 列表都是增量的（游标存 cloud/state.json 的 cursors），不每轮从头列。
//! 1. 推 `account/config.json`（转写服务地址、高德 Key）给手机，内容变了才传；
//! 2. 拉有更新的停留点，写成 `place.visit` 事件；有更新的苹果健康（`health.<日>`，一天一块整天替换）存成 `devices/<设备>/health/<日>.jsonl`；
//! 3. 别的电脑转好的 `transcript.*` 拉下来；还没转的录音按时间顺序交转写服务，结果写成 `speech.*` 事件、再传 `transcript.<段 id>`；
//! 4. 转好超过 7 天的录音块删掉（没转的服务器 30 天兜底删）。
//! 转写服务用同步服务发的票据认账号（拿不到同步令牌）；本人声纹在 `account/voiceprint.json`，当天他人的质心存本机
//! `<数据根>/cloud/speakers/<日>.json`，按时间顺序一段段交、带上、拿回，「他人N」在同一天里一致。

use crate::{parse_places, parse_transcripts, Relay};
use base64::Engine;
use beayy_core::default_root;
use chrono::{DateTime, Local, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 转好的录音在同步服务上再留几天
pub const AUDIO_KEEP_DAYS: i64 = 7;
/// 没转的录音同步服务最多留几天（worker.js 的 AUDIO_MAX_DAYS）
const AUDIO_SERVER_DAYS: i64 = 30;
/// 一段转写最多等多久（排队＋转）
const JOB_WAIT: Duration = Duration::from_secs(15 * 60);
/// 同一段转失败几次就不再交（坏文件）
const MAX_FAILS: u64 = 3;
/// 解析事件时的来源名（设备名都带编号，用不上服务器地址）
const SOURCE: &str = "sync";

fn settings() -> Map<String, Value> {
    fs::read_to_string(default_root().join("settings.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// 转写服务地址：设置里的 transcribe_server，没填用官方的
pub fn transcribe_server() -> String {
    settings()
        .get("transcribe_server")
        .and_then(Value::as_str)
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        .unwrap_or_else(|| beayy_core::DEFAULT_TRANSCRIBE.to_string())
}

/// 本人在转写里的名字：设置里的 speaker_name，没填叫「我」
pub fn speaker_name() -> String {
    settings().get("speaker_name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("我").chars().take(20).collect()
}

fn dir(root: &Path) -> PathBuf {
    root.join("cloud")
}

fn read_state(root: &Path) -> Map<String, Value> {
    fs::read_to_string(dir(root).join("state.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_state(root: &Path, st: &Map<String, Value>) {
    let _ = fs::create_dir_all(dir(root));
    let _ = fs::write(dir(root).join("state.json"), serde_json::to_vec_pretty(st).unwrap_or_default());
}

/// 列表往回让 10 分钟：服务器的 updated 是请求进来时取的，并发上传可能晚一点才落库
const LIST_OVERLAP: i64 = 10 * 60_000;

/// 按块名前缀增量列：从上次列到的最新 updated 往回让一点开始。同步服务的 D1 按读到的行计费，
/// 从 0 列等于每 2 分钟把整个账号的块扫一遍（10-23 起每天撞免费额度，所有账号的同步、官方服务跟着停）
fn list_since(root: &Path, st: &Map<String, Value>, prefix: &str) -> Result<Vec<Value>, String> {
    // 同步文件夹、iCloud、WebDAV：网盘晚到的文件修改时刻可能比游标早，不靠游标、每轮全列（列本地目录；WebDAV 有 60 秒缓存），
    // 靠下面各自记的 updated 去重
    if beayy_sync::official_auth(root).is_none() {
        return beayy_sync::list_named(root, prefix, 0);
    }
    let since = st.get("cursors").and_then(|c| c.get(prefix)).and_then(Value::as_i64).unwrap_or(0);
    beayy_sync::list_named(root, prefix, if since > 0 { (since - LIST_OVERLAP).max(1) } else { 0 })
}

/// 记下这一轮列到哪了；有没拉成的（miss＝其中最早的 updated），游标停在它前面，下一轮再列到
fn advance(st: &mut Map<String, Value>, prefix: &str, items: &[Value], miss: Option<i64>) {
    let Some(top) = items.iter().filter_map(|x| x["updated"].as_i64()).max() else { return };
    let to = miss.map(|m| m - 1).unwrap_or(top);
    let c = st.entry("cursors").or_insert_with(|| json!({}));
    if to > c.get(prefix).and_then(Value::as_i64).unwrap_or(0) {
        c[prefix] = json!(to);
    }
}

fn agent() -> ureq::Agent {
    beayy_sync::http_agent(ureq::Agent::config_builder()
        .proxy(None)
        .tls_config(beayy_core::tls!())
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build())
}

#[derive(Default, Debug)]
pub struct CloudNote {
    pub places: usize,
    /// 拉了几天的苹果健康
    pub health: usize,
    pub transcribed: usize,
    pub fetched: usize,
    pub deleted: usize,
    /// 还没转的录音段数
    pub waiting: usize,
    pub added: usize,
    pub errors: Vec<String>,
}

impl CloudNote {
    pub fn summary(&self) -> String {
        let mut s = format!("新增 {} 条", self.added);
        if self.transcribed > 0 {
            s += &format!("，转写 {} 段", self.transcribed);
        }
        if self.waiting > 0 {
            s += &format!("，还有 {} 段等转写", self.waiting);
        }
        if let Some(e) = self.errors.first() {
            s += &format!("；{e}");
        }
        s
    }
}

/// 转写票据：快到期（5 分钟内）再取新的
struct Ticket(Option<(String, i64)>);

impl Ticket {
    fn get(&mut self, root: &Path) -> Result<String, String> {
        let now = Utc::now().timestamp_millis();
        if let Some((t, exp)) = &self.0 {
            if *exp - now > 300_000 {
                return Ok(t.clone());
            }
        }
        let got = beayy_sync::ticket(root)?;
        self.0 = Some(got.clone());
        Ok(got.0)
    }
}

/// 一轮。没登录账号返回 Ok（什么都不做）；budget 用完就把剩下的录音留到下一轮
pub fn round(relay: &mut Relay, root: &Path, budget: Duration) -> Result<CloudNote, String> {
    if beayy_sync::load(root).is_none() {
        return Err("没登录账号".into());
    }
    let t0 = Instant::now();
    let mut st = read_state(root);
    let mut note = CloudNote::default();
    // 配置以电脑为准：电脑推，手机拉（手机本地没有高德 Key，推了会盖掉电脑的）
    #[cfg(not(target_os = "ios"))]
    if let Err(e) = push_config(root, &mut st) {
        note.errors.push(format!("传配置：{e}"));
    }
    #[cfg(target_os = "ios")]
    if let Err(e) = pull_config(root) {
        note.errors.push(format!("取配置：{e}"));
    }
    // 停留点：有更新的那几天整天拉
    let seen_places = st.get("places").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut places = seen_places.clone();
    let mut miss = None;
    let items = list_since(root, &st, "places.")?;
    for it in &items {
        let (d, n, up) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""), it["updated"].as_i64().unwrap_or(0));
        let k = format!("{d}/{n}");
        if seen_places.get(&k).and_then(Value::as_i64) == Some(up) {
            continue;
        }
        match beayy_sync::get_named(root, d, n) {
            Ok(Some(b)) => {
                note.added += relay.ingest(root, parse_places(&String::from_utf8_lossy(&b), SOURCE));
                note.places += 1;
                places.insert(k, json!(up));
            }
            Ok(None) => {}
            Err(e) => {
                miss = Some(miss.map_or(up, |m: i64| m.min(up)));
                note.errors.push(format!("拉停留点 {n}：{e}"));
            }
        }
    }
    st.insert("places".into(), Value::Object(places));
    advance(&mut st, "places.", &items, miss);
    write_state(root, &st);

    // 苹果健康：有更新的那几天整天拉，原样存成文件（不写事件：一天几千行，进分段会跟着电脑记录再同步一遍）
    let seen_health = st.get("health").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut health = seen_health.clone();
    let mut miss = None;
    let items = list_since(root, &st, "health.")?;
    for it in &items {
        let (d, n, up) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""), it["updated"].as_i64().unwrap_or(0));
        let k = format!("{d}/{n}");
        let day = n.trim_start_matches("health.");
        if seen_health.get(&k).and_then(Value::as_i64) == Some(up) || chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() {
            continue;
        }
        match beayy_sync::get_named(root, d, n) {
            Ok(Some(b)) => {
                let dir = root.join("devices").join(d).join("health");
                match fs::create_dir_all(&dir).and_then(|_| fs::write(dir.join(format!("{day}.jsonl")), &b)) {
                    Ok(()) => {
                        note.health += 1;
                        health.insert(k, json!(up));
                    }
                    Err(e) => note.errors.push(format!("存健康 {day}：{e}")),
                }
            }
            Ok(None) => {}
            Err(e) => {
                miss = Some(miss.map_or(up, |m: i64| m.min(up)));
                note.errors.push(format!("拉健康 {n}：{e}"));
            }
        }
    }
    st.insert("health".into(), Value::Object(health));
    advance(&mut st, "health.", &items, miss);
    write_state(root, &st);

    // 转写：已有的（别的电脑转的、或这台以前转的）拉下来
    let seen_tr = st.get("transcripts").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut trs = seen_tr.clone();
    let mut miss = None;
    let items = list_since(root, &st, "transcript.")?;
    for it in &items {
        let (d, n, up) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""), it["updated"].as_i64().unwrap_or(0));
        let k = format!("{d}/{n}");
        if seen_tr.get(&k).and_then(Value::as_i64).map(|x| x >= up).unwrap_or(false) {
            continue;
        }
        match beayy_sync::get_named(root, d, n) {
            Ok(Some(b)) => {
                note.added += relay.ingest(root, parse_transcripts(&String::from_utf8_lossy(&b), SOURCE));
                note.fetched += 1;
                trs.insert(k, json!(up));
            }
            Ok(None) => {}
            Err(e) => {
                miss = Some(miss.map_or(up, |m: i64| m.min(up)));
                note.errors.push(format!("拉转写 {n}：{e}"));
            }
        }
    }
    // 转过的段：拉到过的转写都算（列表是增量的，只看这一轮会漏）
    let done: HashSet<String> = trs.keys().map(|k| k.replacen("/transcript.", "/", 1)).collect();
    st.insert("transcripts".into(), Value::Object(trs));
    advance(&mut st, "transcript.", &items, miss);
    write_state(root, &st);

    // 转写要同步服务发的票据：同步文件夹、iCloud、WebDAV 没有服务器，录音这一步不做（手机在这些模式下也不传录音，留在手机上）
    if beayy_sync::official_auth(root).is_none() {
        return Ok(note);
    }

    // 录音：本机记一份服务器上有哪些录音块（增量列来的并进去），没转的按时间顺序交（段 id 里是开始时刻），转好超过 7 天的删
    let mut audio = st.get("audio").and_then(Value::as_object).cloned().unwrap_or_default();
    let items = list_since(root, &st, "audio.")?;
    for it in &items {
        audio.insert(format!("{}/{}", it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or("")), it["updated"].clone());
    }
    advance(&mut st, "audio.", &items, None);
    // 服务器 30 天兜底删了的，本机这份也不留
    let gone = Utc::now().timestamp_millis() - (AUDIO_SERVER_DAYS + 1) * 86_400_000;
    audio.retain(|_, up| up.as_i64().unwrap_or(0) >= gone);
    let mut todo: Vec<(String, String)> = vec![];
    let cutoff = Utc::now().timestamp_millis() - AUDIO_KEEP_DAYS * 86_400_000;
    for (k, up) in audio.clone() {
        let Some((d, n)) = k.split_once('/') else { continue };
        let (d, up) = (d.to_string(), up.as_i64().unwrap_or(0));
        let seg = n.trim_start_matches("audio.").to_string();
        if done.contains(&format!("{d}/{seg}")) {
            if up < cutoff {
                match beayy_sync::delete_named(root, &d, n) {
                    Ok(()) => {
                        note.deleted += 1;
                        audio.remove(&k);
                    }
                    Err(e) => note.errors.push(format!("删录音 {seg}：{e}")),
                }
            }
            continue;
        }
        let fails = st.get("failed").and_then(|f| f.get(format!("{d}/{seg}"))).and_then(Value::as_u64).unwrap_or(0);
        if fails < MAX_FAILS {
            todo.push((d, seg));
        }
    }
    st.insert("audio".into(), Value::Object(audio));
    write_state(root, &st);
    todo.sort_by(|a, b| a.1.split('-').next_back().cmp(&b.1.split('-').next_back()).then(a.cmp(b)));
    note.waiting = todo.len();
    if todo.is_empty() {
        return Ok(note);
    }
    let me = voiceprint(root);
    let mut ticket = Ticket(None);
    for (d, seg) in todo {
        if t0.elapsed() > budget {
            break;
        }
        match transcribe_one(root, relay, &mut ticket, &d, &seg, me.as_deref()) {
            Ok(added) => {
                note.added += added;
                note.transcribed += 1;
                note.waiting -= 1;
                if let Some(Value::Object(f)) = st.get_mut("failed") {
                    f.remove(&format!("{d}/{seg}"));
                }
            }
            Err(e) => {
                let f = st.entry("failed").or_insert_with(|| json!({}));
                let n = f.get(format!("{d}/{seg}")).and_then(Value::as_u64).unwrap_or(0) + 1;
                f[format!("{d}/{seg}")] = json!(n);
                if e.contains("没有这段录音了") {
                    if let Some(Value::Object(a)) = st.get_mut("audio") {
                        a.remove(&format!("{d}/audio.{seg}"));
                    }
                }
                note.errors.push(format!("转写 {seg}：{e}"));
                write_state(root, &st);
                // 连不上转写服务就别一段段试了
                if e.contains("连不上") || e.contains("票据") {
                    break;
                }
            }
        }
        write_state(root, &st);
    }
    Ok(note)
}

/// 推给手机的配置：转写服务地址、高德 Key。内容变了才传
#[cfg(not(target_os = "ios"))]
fn push_config(root: &Path, st: &mut Map<String, Value>) -> Result<(), String> {
    // 地图用官方的：不推 Key，手机也经同步服务中转
    let official_map = settings().get("map_mode").and_then(Value::as_str).map(|m| m == "official").unwrap_or_else(|| beayy_core::secret_get("amap_key").is_none());
    let provider = settings().get("map_provider").and_then(Value::as_str).filter(|p| *p == "osm").unwrap_or("amap").to_string();
    let cfg = json!({"v": 1, "transcribe_server": transcribe_server(), "map_mode": if official_map { "official" } else { "custom" },
        "amap_key": if official_map { None } else { beayy_core::secret_get("amap_key") }, "map_provider": provider});
    let sig = format!("{:x}", fnv(&cfg.to_string()));
    if st.get("config_sig").and_then(Value::as_str) == Some(sig.as_str()) {
        return Ok(());
    }
    let mut body = cfg;
    body["updated"] = json!(Utc::now().timestamp_millis());
    beayy_sync::put_named(root, "account", "config.json", body.to_string().as_bytes())?;
    st.insert("config_sig".into(), json!(sig));
    write_state(root, st);
    Ok(())
}

/// 手机：取电脑推的配置，高德 Key 存进本机（地名、候选用），转写服务地址写进设置
#[cfg(target_os = "ios")]
fn pull_config(root: &Path) -> Result<(), String> {
    let Some(b) = beayy_sync::get_named(root, "account", "config.json")? else { return Ok(()) };
    let v: Value = serde_json::from_slice(&b).map_err(|e| e.to_string())?;
    if let Some(k) = v["amap_key"].as_str().filter(|k| !k.is_empty()) {
        if beayy_core::secret_get("amap_key").as_deref() != Some(k) {
            beayy_core::secret_set("amap_key", k)?;
        }
    }
    // 转写服务地址、地图服务（高德／OpenStreetMap）照电脑的写进手机的设置
    let p = default_root().join("settings.json");
    let mut s: Map<String, Value> = fs::read_to_string(&p).ok().and_then(|x| serde_json::from_str(&x).ok()).unwrap_or_default();
    let mut changed = false;
    for k in ["transcribe_server", "map_provider"] {
        if let Some(t) = v[k].as_str().filter(|t| !t.is_empty()) {
            if s.get(k).and_then(Value::as_str) != Some(t) {
                s.insert(k.into(), json!(t));
                changed = true;
            }
        }
    }
    if changed {
        let _ = fs::write(&p, serde_json::to_vec_pretty(&s).unwrap_or_default());
    }
    Ok(())
}

#[cfg(not(target_os = "ios"))]
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// 本人声纹（base64 的 float32），没录过返回 None
fn voiceprint(root: &Path) -> Option<String> {
    let b = beayy_sync::get_named(root, "account", "voiceprint.json").ok()??;
    let v: Value = serde_json::from_slice(&b).ok()?;
    v["embedding"].as_str().map(String::from)
}

fn speakers_path(root: &Path, day: &str) -> PathBuf {
    dir(root).join("speakers").join(format!("{day}.json"))
}

/// 交一段转写、等结果、写事件、传 transcript 块；返回新写的事件条数
fn transcribe_one(root: &Path, relay: &mut Relay, ticket: &mut Ticket, device: &str, seg: &str, me: Option<&str>) -> Result<usize, String> {
    let b = beayy_sync::get_named(root, device, &format!("audio.{seg}"))?.ok_or("同步服务上没有这段录音了")?;
    let cut = b.iter().position(|&c| c == b'\n').ok_or("录音块格式不对：缺元数据")?;
    let meta: Value = serde_json::from_slice(&b[..cut]).map_err(|_| "录音块格式不对：元数据不是 JSON")?;
    let audio = &b[cut + 1..];
    let start: DateTime<Utc> = meta["start"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).ok_or("录音块缺开始时刻")?.with_timezone(&Utc);
    let day = start.with_timezone(&Local).format("%Y-%m-%d").to_string();
    let sp = speakers_path(root, &day);
    let others: Value = fs::read_to_string(&sp).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!([]));
    let server = transcribe_server();
    let ag = agent();
    let auth = format!("Ticket {}", ticket.get(root)?);
    let mut m = meta.as_object().cloned().unwrap_or_default();
    m.insert("device".into(), json!(device));
    let body = json!({
        "meta": m, "audio": base64::engine::general_purpose::STANDARD.encode(audio),
        "self": me, "self_name": speaker_name(), "others": others,
    });
    let mut r = ag.post(&format!("{server}/v2/jobs")).header("Authorization", &auth).send_json(&body).map_err(|e| format!("连不上转写服务：{e}"))?;
    let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
    if !r.status().is_success() {
        return Err(format!("转写服务返回 {} {}", r.status().as_u16(), v["detail"].as_str().unwrap_or("")));
    }
    let job = v["job"].as_str().ok_or("转写服务没给任务号")?.to_string();
    let t0 = Instant::now();
    let result = loop {
        std::thread::sleep(Duration::from_secs(3));
        let mut r = ag.get(&format!("{server}/v2/jobs/{job}")).header("Authorization", &auth).call().map_err(|e| format!("连不上转写服务：{e}"))?;
        let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
        if !r.status().is_success() {
            return Err(format!("转写服务返回 {} {}", r.status().as_u16(), v["detail"].as_str().unwrap_or("")));
        }
        match v["status"].as_str() {
            Some("done") => break v["result"].clone(),
            Some("failed") => {
                let _ = ag.delete(&format!("{server}/v2/jobs/{job}")).header("Authorization", &auth).call();
                return Err(format!("转写失败：{}", v["error"].as_str().unwrap_or("")));
            }
            _ if t0.elapsed() > JOB_WAIT => return Err("等了 15 分钟还没转完，下一轮再取".into()),
            _ => {}
        }
    };
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
    beayy_sync::put_named(root, device, &format!("transcript.{seg}"), text.as_bytes())?;
    if let Some(o) = result.get("others").filter(|o| o.is_array()) {
        let _ = fs::create_dir_all(sp.parent().unwrap());
        let _ = fs::write(&sp, o.to_string());
    }
    let _ = ag.delete(&format!("{server}/v2/jobs/{job}")).header("Authorization", &auth).call();
    Ok(relay.ingest(root, parse_transcripts(&text, SOURCE)))
}

/// 设置「数据」页用：本机这几样各占多少（字节）
pub fn local_sizes(root: &Path) -> BTreeMap<String, u64> {
    fn walk(p: &Path) -> u64 {
        let Ok(rd) = fs::read_dir(p) else { return fs::metadata(p).map(|m| m.len()).unwrap_or(0) };
        rd.filter_map(|e| e.ok()).map(|e| {
            let ft = e.file_type().ok();
            if ft.map(|t| t.is_dir()).unwrap_or(false) { walk(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
        }).sum()
    }
    let mut out = BTreeMap::new();
    for e in fs::read_dir(root).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default() {
        out.insert(e.file_name().to_string_lossy().to_string(), walk(&e.path()));
    }
    out
}

/// 转写服务上这个账号的任务（设置「数据」页用）：{jobs: {状态: 个数}, bytes, keep_days}
pub fn transcribe_usage(root: &Path) -> Result<Value, String> {
    let t = beayy_sync::ticket(root)?.0;
    let mut r = agent().get(&format!("{}/v2/usage", transcribe_server())).header("Authorization", &format!("Ticket {t}")).call().map_err(|e| format!("连不上转写服务：{e}"))?;
    let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
    if !r.status().is_success() {
        return Err(format!("转写服务返回 {} {}", r.status().as_u16(), v["detail"].as_str().unwrap_or("")));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_stops_before_missed() {
        let mut st = Map::new();
        let items = vec![json!({"updated": 100}), json!({"updated": 300}), json!({"updated": 200})];
        advance(&mut st, "places.", &items, None);
        assert_eq!(st["cursors"]["places."], json!(300));
        // 有没拉成的：停在它前面；不往回退
        advance(&mut st, "places.", &[json!({"updated": 500})], Some(400));
        assert_eq!(st["cursors"]["places."], json!(399));
        advance(&mut st, "places.", &[json!({"updated": 600})], Some(50));
        assert_eq!(st["cursors"]["places."], json!(399));
        advance(&mut st, "audio.", &[], None);
        assert!(st["cursors"].get("audio.").is_none());
    }

    /// 手机传的 health.<日> 经同步（这里用同步文件夹）到电脑：整天存成 devices/<设备>/health/<日>.jsonl；没变的下一轮不重拉，变了整天替换
    #[test]
    fn health_pull() {
        let base = std::env::temp_dir().join(format!("beayy-health-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (phone, mac, dir) = (base.join("phone"), base.join("mac"), base.join("cloud"));
        std::env::set_var("BEAYY_DATA_ROOT", &phone); // 账号存文件，不碰钥匙串
        beayy_sync::folder_create(&phone, dir.to_str().unwrap(), "password-1").unwrap();
        beayy_sync::put_named(&phone, "iphone-test", "health.2026-10-29", b"{\"id\":\"a\"}\n").unwrap();
        beayy_sync::folder_join(&mac, dir.to_str().unwrap(), "password-1").unwrap();
        let mut relay = Relay::default();
        let n = round(&mut relay, &mac, Duration::from_secs(5)).unwrap();
        let f = mac.join("devices/iphone-test/health/2026-10-29.jsonl");
        assert_eq!((n.health, fs::read_to_string(&f).unwrap()), (1, "{\"id\":\"a\"}\n".to_string()));
        assert_eq!(round(&mut relay, &mac, Duration::from_secs(5)).unwrap().health, 0);
        std::thread::sleep(Duration::from_millis(20));
        beayy_sync::put_named(&phone, "iphone-test", "health.2026-10-29", b"{\"id\":\"a\"}\n{\"id\":\"b\"}\n").unwrap();
        assert_eq!(round(&mut relay, &mac, Duration::from_secs(5)).unwrap().health, 1);
        assert_eq!(fs::read_to_string(&f).unwrap().lines().count(), 2);
        let _ = fs::remove_dir_all(&base);
    }
}
