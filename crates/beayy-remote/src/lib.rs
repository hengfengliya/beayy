//! 手表录音和位置：从你自己的录音服务器（HTTPS＋Bearer 口令，接口见 src-tauri/COMMANDS.md）拉回对话转写和停留点，写成事件进 `devices/`：
//! - 对话 → `devices/watch-<id>/`：每段录音一条 `speech.segment`，每句话一条 `speech.utterance`（时刻＝开口时刻）；
//! - 停留点 → 记它的那台设备的目录：iPhone 记的进 `devices/<服务器给的 iphone-…>/`（和屏幕使用同一台 iPhone），
//!   手表录音时取点算的（device 以 watch 开头）进和对话同一个 `devices/watch-<id>/`：`place.visit`。
//!   同一个停留点补上离开时间、地名后服务器上是新的一版，这里再记一条，读的时候同一个 place_id 取最后一条。
//!
//! 按（种类、id、内容）去重，只增不改。服务器 `GET /v1/days` 给出每天文件的大小，只拉变了的那几天（`<数据根>/relay/state.json`）。
//! 设置：`settings.json` 的 `relay_server`（如 https://audio.example.com，没填就不拉）；
//! 口令在钥匙串（服务 com.fengli.beayy，账号 relay_token），Windows 在数据目录 `secrets/relay_token`。
//! 拉来的数据不经多设备同步：每台电脑各自从服务器拉。

pub mod cloud;

use beayy_core::i18n::{tr, tr_with};
use beayy_core::{default_root, iso, EventLog};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// 口令在钥匙串里的账号名
pub const SECRET: &str = "relay_token";
/// 两种数据：服务器上的目录名
pub const KINDS: [&str; 2] = ["transcripts", "places"];

/// settings.json 里的录音服务器地址（去掉末尾的 /）；没填返回 None
pub fn server() -> Option<String> {
    let v: Value = fs::read_to_string(default_root().join("settings.json")).ok().and_then(|t| serde_json::from_str(&t).ok())?;
    let s = v.get("relay_server")?.as_str()?.trim().trim_end_matches('/').to_string();
    (s.starts_with("https://") || s.starts_with("http://")).then_some(s)
}

fn host_of(server: &str) -> String {
    server.split("://").nth(1).unwrap_or(server).split('/').next().unwrap_or("").to_string()
}

/// FNV-1a：设备目录名、去重键用（不用于安全）
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// 服务器给的设备名 → 本机设备目录名。已经是 `<种类>-<id>` 的照用；只写了种类（如手表的 "watch"）的，用服务器地址补一个稳定的 id
fn device_dir(kind: &str, given: &str, host: &str) -> String {
    if given.starts_with(&format!("{kind}-")) && safe(given) {
        return given.to_string();
    }
    format!("{kind}-{:08x}", fnv(&format!("{host}/{given}")) as u32)
}

fn utc(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

fn utc_or_null(v: &Value) -> Value {
    utc(v).map(|t| json!(iso(t))).unwrap_or(Value::Null)
}

/// 一条要写的事件
#[derive(Debug, Clone)]
pub struct Item {
    pub device: String,
    pub event_type: &'static str,
    pub at: DateTime<Utc>,
    pub key: String,
    pub fields: Map<String, Value>,
}

/// transcripts/<日>.jsonl（type＝segment／utt，见 录音/README.md）→ 事件
pub fn parse_transcripts(text: &str, host: &str) -> Vec<Item> {
    let mut out = vec![];
    // 句子行没有设备名：跟着它所在那段录音
    let mut seg_dev: HashMap<String, String> = HashMap::new();
    let rows: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    for r in &rows {
        if r["type"] == "segment" {
            if let Some(id) = r["id"].as_str() {
                // 手表录的归手表；没戴手表时 iPhone 自己录的（device 是 iphone-…）归那台 iPhone
                let given = r["device"].as_str().unwrap_or("watch");
                seg_dev.insert(id.into(), device_dir(if given.starts_with("iphone") { "iphone" } else { "watch" }, given, host));
            }
        }
    }
    for r in rows {
        let source = json!(format!("relay:{host}"));
        match r["type"].as_str() {
            Some("segment") => {
                let (Some(id), Some(at)) = (r["id"].as_str(), utc(&r["start"])) else { continue };
                let mut f = Map::new();
                f.insert("seg_id".into(), json!(id));
                f.insert("start".into(), json!(iso(at)));
                f.insert("end".into(), utc_or_null(&r["end"]));
                for k in ["audio_sec", "speech_sec", "battery"] {
                    f.insert(k.into(), r[k].clone());
                }
                f.insert("source".into(), source);
                out.push(Item { device: seg_dev[id].clone(), event_type: "speech.segment", at, key: format!("seg:{id}"), fields: f });
            }
            Some("utt") => {
                let (Some(seg), Some(at), Some(text)) = (r["seg"].as_str(), utc(&r["start"]), r["text"].as_str()) else { continue };
                let speaker = r["speaker"].as_str().unwrap_or("未辨");
                let end = utc_or_null(&r["end"]);
                let key = format!("utt:{seg}:{:016x}", fnv(&format!("{}|{}|{speaker}|{text}", iso(at), end)));
                let device = seg_dev.get(seg).cloned().unwrap_or_else(|| device_dir("watch", "watch", host));
                let mut f = Map::new();
                f.insert("seg_id".into(), json!(seg));
                f.insert("start".into(), json!(iso(at)));
                f.insert("end".into(), end);
                f.insert("speaker".into(), json!(speaker));
                f.insert("text".into(), json!(text));
                f.insert("source".into(), source);
                out.push(Item { device, event_type: "speech.utterance", at, key, fields: f });
            }
            _ => {}
        }
    }
    out
}

/// places/<日>.jsonl（iPhone 停留点：id、arrive、depart、lat、lon、acc、name、label、source、device、guesses）→ 事件。同一个 id 的每一版各一条
pub fn parse_places(text: &str, host: &str) -> Vec<Item> {
    let mut out = vec![];
    for line in text.lines() {
        let Ok(r) = serde_json::from_str::<Value>(line) else { continue };
        let Some(id) = r["id"].as_str() else { continue };
        let Some(at) = utc(&r["arrive"]).or_else(|| utc(&r["depart"])) else { continue };
        // 停留点可能是 iPhone 记的，也可能是手表录音时取点算的（device 以 watch 开头），各归各的设备目录
        let given = r["device"].as_str().unwrap_or("iphone");
        let device = device_dir(if given.starts_with("watch") { "watch" } else { "iphone" }, given, host);
        let mut f = Map::new();
        f.insert("place_id".into(), json!(id));
        f.insert("arrive".into(), utc_or_null(&r["arrive"]));
        f.insert("depart".into(), utc_or_null(&r["depart"]));
        for k in ["lat", "lon", "acc", "name", "label", "rev"] {
            f.insert(k.into(), r[k].clone());
        }
        f.insert("place_source".into(), r["source"].clone());
        // 是哪家店的候选（手机用高德算的）：有才写，免得旧记录的 key 变了被当成新的一版
        if r["guesses"].is_array() {
            f.insert("guesses".into(), r["guesses"].clone());
        }
        f.insert("source".into(), json!(format!("relay:{host}")));
        // 内容变了（补上离开时间、地名）才算新的一版
        let key = format!("place:{id}:{:016x}", fnv(&Value::Object(f.clone()).to_string()));
        out.push(Item { device, event_type: "place.visit", at, key, fields: f });
    }
    out
}

/// 某台设备已写过的去重键
fn load_seen(log: &EventLog) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(dirs) = fs::read_dir(log.segments_dir()) else { return out };
    for d in dirs.filter_map(|e| e.ok()) {
        let Ok(bytes) = fs::read(d.path().join("events.jsonl")) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines() {
            if !line.contains("\"relay_key\"") {
                continue;
            }
            if let Some(k) = serde_json::from_str::<Value>(line).ok().and_then(|v| v["relay_key"].as_str().map(String::from)) {
                out.insert(k);
            }
        }
    }
    out
}

#[derive(Default)]
pub struct Relay {
    writers: HashMap<String, (EventLog, HashSet<String>)>,
    pub last_summary: String,
    /// 写进去的设备目录名加的后缀。手机上用（"-m"）：手机自己算出的事件单独放，不和电脑同步来的同名目录混写；
    /// 读的地方按停留点 id、段 id、开口时刻＋原文去重
    pub device_suffix: String,
}

#[derive(Default, Debug)]
pub struct PullNote {
    /// 拉了几天的文件
    pub days: usize,
    /// 新写了几条事件
    pub added: usize,
    /// 服务器上最近一天
    pub latest_day: Option<String>,
}

fn agent() -> ureq::Agent {
    // 根证书用系统的（同 beayy-sync）
    beayy_sync::http_agent(ureq::Agent::config_builder()
        .proxy(None)
        .tls_config(beayy_core::tls!())
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build())
}

fn state_path(root: &Path) -> std::path::PathBuf {
    root.join("relay").join("state.json")
}

impl Relay {
    /// 写进去，返回新写的条数
    pub fn ingest(&mut self, root: &Path, mut items: Vec<Item>) -> usize {
        items.sort_by_key(|x| x.at);
        let mut added = 0;
        for it in items {
            let device = format!("{}{}", it.device, self.device_suffix);
            let entry = self.writers.entry(device.clone()).or_insert_with(|| {
                let log = EventLog::new(root.to_path_buf(), device.clone());
                let seen = load_seen(&log);
                (log, seen)
            });
            if !entry.1.insert(it.key.clone()) {
                continue;
            }
            let mut f = it.fields;
            f.insert("relay_key".into(), json!(it.key));
            f.insert("recorded_at".into(), json!(iso(Utc::now())));
            entry.0.write(it.event_type, f, Some(it.at));
            added += 1;
        }
        added
    }

    /// 拉一轮：没设置服务器就什么都不做
    pub fn pull(&mut self, root: &Path) -> Result<PullNote, String> {
        let Some(server) = server() else {
            self.last_summary = tr("没设置录音服务器");
            return Ok(PullNote::default());
        };
        let r = self.pull_from(root, &server);
        self.last_summary = match &r {
            Ok(n) => tr_with("新增 {n} 条，服务器最近一天 {day}", &[("n", &n.added.to_string()), ("day", n.latest_day.as_deref().unwrap_or("—"))]),
            Err(e) => e.clone(),
        };
        r
    }

    fn pull_from(&mut self, root: &Path, server: &str) -> Result<PullNote, String> {
        let token = beayy_core::secret_get(SECRET).ok_or("还没有录音服务器的口令：到设置里存一下")?;
        let auth = format!("Bearer {token}");
        let ag = agent();
        let get = |path: &str| -> Result<String, String> {
            let mut r = ag.get(&format!("{server}{path}")).header("Authorization", &auth).call().map_err(|e| format!("连不上录音服务器：{e}"))?;
            match r.status().as_u16() {
                200 => r.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| e.to_string()),
                401 => Err("录音服务器口令不对".into()),
                s => Err(format!("录音服务器返回 {s}")),
            }
        };
        let days: Value = serde_json::from_str(&get("/v1/days")?).map_err(|e| format!("录音服务器返回的不是 JSON：{e}"))?;
        let mut state: BTreeMap<String, Value> = fs::read_to_string(state_path(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let host = host_of(server);
        let mut note = PullNote::default();
        for kind in KINDS {
            let Some(m) = days[kind].as_object() else { continue };
            for (day, size) in m {
                if !(day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-')) {
                    continue;
                }
                if note.latest_day.as_deref().map(|l| day.as_str() > l).unwrap_or(true) {
                    note.latest_day = Some(day.clone());
                }
                let k = format!("{kind}/{day}");
                if state.get(&k) == Some(size) {
                    continue;
                }
                let text = get(&format!("/v1/{kind}/{day}"))?;
                let items = if kind == "transcripts" { parse_transcripts(&text, &host) } else { parse_places(&text, &host) };
                note.added += self.ingest(root, items);
                note.days += 1;
                state.insert(k, size.clone());
            }
        }
        let p = state_path(root);
        let _ = fs::create_dir_all(p.parent().unwrap());
        let _ = fs::write(&p, serde_json::to_string_pretty(&state).unwrap_or_default());
        Ok(note)
    }

    /// 调试用：把本地的一个 jsonl 文件当成服务器上的这种数据写进去（服务器地址按设置里的算设备 id）
    pub fn import_file(&mut self, root: &Path, kind: &str, file: &Path) -> Result<usize, String> {
        let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
        let host = host_of(&server().unwrap_or_default());
        let items = if kind == "places" { parse_places(&text, &host) } else { parse_transcripts(&text, &host) };
        Ok(self.ingest(root, items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = r#"{"type": "segment", "id": "watch-20261016T035529Z", "start": "2026-10-16T11:55:29+08:00", "end": "2026-10-16T12:05:29+08:00", "device": "watch", "battery": 0.9, "audio_sec": 600.1, "speech_sec": 68.6}
{"type": "utt", "seg": "watch-20261016T035529Z", "start": "2026-10-16T11:56:26+08:00", "end": "2026-10-16T11:56:27+08:00", "speaker": "我", "text": "好的。"}
"#;
    const P: &str = r#"{"id":"visit-20261016T014500Z","device":"iphone-1A2B3C4D","kind":"place.visit","source":"visit","arrive":"2026-10-16T09:45:00+08:00","depart":null,"lat":31.2,"lon":121.4,"acc":35,"name":"徐汇 · 某大厦","label":null,"rev":1}
{"id":"visit-20261016T014500Z","device":"iphone-1A2B3C4D","kind":"place.visit","source":"visit","arrive":"2026-10-16T09:45:00+08:00","depart":"2026-10-16T12:10:00+08:00","lat":31.2,"lon":121.4,"acc":35,"name":"徐汇 · 某大厦","label":null,"rev":2}
"#;

    #[test]
    fn parse_and_dedup() {
        let items = parse_transcripts(T, "audio.example.com");
        assert_eq!(items.len(), 2);
        assert!(items[0].device.starts_with("watch-") && items[0].device.len() == 14);
        assert_eq!(items[1].fields["start"], "2026-10-16T03:56:26.000Z");
        let pl = parse_places(P, "audio.example.com");
        assert_eq!(pl[0].device, "iphone-1A2B3C4D");
        assert_ne!(pl[0].key, pl[1].key);

        let root = std::env::temp_dir().join(format!("beayy-relay-test-{}", std::process::id()));
        let mut r = Relay::default();
        assert_eq!(r.ingest(&root, items.clone()), 2);
        assert_eq!(r.ingest(&root, items.clone()), 0);
        // 重启后从文件里认出写过的
        let mut r2 = Relay::default();
        assert_eq!(r2.ingest(&root, items), 0);
        assert_eq!(r2.ingest(&root, pl), 2);
        let _ = fs::remove_dir_all(root);
    }
}
