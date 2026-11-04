//! 10 分钟摘要：把 Beayy 采到的电脑事件按分段写成摘要，格式与 Codex 电脑历史的 10min-memory-summary 相同。
//! 从原 脚本/自采摘要.py 移植（10-16 删除），拼包、去重、提示词、输出格式逐项一致：80 个分段拼出的包与 Python 版逐字相同。
//!
//! 输入：`<数据根>/devices/mac-*/segments/<UTC 整 10 分>/events.jsonl`（格式见 app/README.md；多台 Mac 同段合并）
//! 输出：`<数据根>/summaries/YYYY-MM-DDTHH-MM-SS-xxxx-10min-memory-summary.md`（UTC）
//! 摘要间隔（settings.json 的 summary_minutes：10／20／30／60，默认 10）调长后，一份摘要管几个分段，
//! 文件名里写它管多长（-30min-），读的一方按文件名算止时刻；采集的分段照旧是 10 分钟。
//!
//! 每个已结束、还没写过摘要的分段：
//!   1. 代码算时长表：切换区间 ∩ 在电脑前，心跳兜底（同 脚本/beayy时长.py）；早期没有 presence 的分段只按切换算；
//!   2. 去重：同一「app＋窗口」的文字只留比上一次（含上一分段）新增的行；
//!   3. 拼包：时长表＋新增文字＋键盘输入/发送（操作者、按键原文）＋点击元素，压到约 1 万字以内；
//!   4. 调网关（chat.completions）只写描述，数字和 applications 由代码填。
//!   有手表录音、停留点（beayy-remote 拉来的 devices/watch-*、iphone-* 里的 speech.utterance、place.visit）时，
//!   这 10 分钟的对话原文和所在地点也拼进包，标明来源；手表在录时等这一段的转写拉回来再写（最多等 40 分钟）。
//!
//! 设置：`<数据根>/settings.json` 的 gateway（任何 OpenAI 兼容接口）、summary_model，没填用 beayy_core::DEFAULT_GATEWAY／DEFAULT_MODEL（模型也空就不写摘要）。
//! 密钥：环境变量 BEAYY_API_KEY（或旧名 LIFE_RECORD_API_KEY）＞ 钥匙串（服务 com.fengli.beayy，账户 api_key）。

use chrono::{DateTime, Duration, Local, NaiveDateTime, Timelike, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const GATEWAY: &str = beayy_core::DEFAULT_GATEWAY;
const MODEL: &str = beayy_core::DEFAULT_MODEL;
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// 中文约 0.7 token/字，1 万字约 7 千 token，加提示词和输出约 8 千
const PACK_CHARS: usize = 10000;
/// 对话原文另算，最多这么多字
const SPEECH_CHARS: usize = 3000;
/// 同一个人 20 秒内接着说的并成一行（同 脚本/日数据包.py）
const SPEECH_MERGE: i64 = 20;
/// 手表在录时，分段结束后最多等这么久转写（录音服务器每 10 分钟转一轮、这边每 10 分钟拉一次）
const SPEECH_WAIT_MIN: i64 = 40;
/// 整段都不在电脑前的固定标题：不调模型，直接写；界面把连续的这种段合成一行
pub const IDLE_TITLE: &str = "不在电脑前";
/// 摘要间隔能选的（分钟）：都能整除 60，一份摘要不会跨小时
pub const EVERY: &[i64] = &[10, 20, 30, 60];
const NOISE: &[&str] = &["app.launch", "permission.changed", "permission.requested", "tap.started", "app.quit", "app.paused", "app.resumed", "relay.sync", "relay.error"];
// 时长（同 脚本/beayy时长.py）
const IDLE_THRESHOLD: i64 = 180;
const MAX_GAP: i64 = 180;
const POINT_TYPES: &[&str] = &["app.activated", "window.changed", "heartbeat", "ui.text", "app.launch"];
const LEAVE_TYPES: &[&str] = &["system.sleep", "screen.sleep", "screen.locked", "app.quit", "app.paused"];
const BACK_TYPES: &[&str] = &["system.wake", "screen.wake", "screen.unlocked", "app.resumed"];

static SEG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-00Z$").unwrap());
/// 界面杂项：Claude 的「运行中」「3.4k 个 token」「上下文 47%」、相对时间
static JUNK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[\d.,]+k? 个 token|上下文 \d+%|\d+%|\d+\s*(?:秒|分钟|小时|天)前|.*运行中|处理中…|思考中)$").unwrap());

type Ev = Map<String, Value>;

fn s<'a>(e: &'a Ev, k: &str) -> &'a str {
    e.get(k).and_then(Value::as_str).unwrap_or("")
}

fn ty(e: &Ev) -> &str {
    s(e, "event_type")
}

fn ts(e: &Ev) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s(e, "timestamp")).map(|t| t.with_timezone(&Utc)).unwrap_or_default()
}

/// Python 的 s[:n]：按字符截
fn cut(x: &str, n: usize) -> String {
    x.chars().take(n).collect()
}

fn clen(x: &str) -> usize {
    x.chars().count()
}

/// Python 的 str.splitlines：\r\n 算一个换行，末尾的换行不产生空行
fn split_lines(x: &str) -> Vec<&str> {
    let is_break = |c: char| matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}');
    let mut out = vec![];
    let mut start = 0;
    let mut it = x.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if is_break(c) {
            out.push(&x[start..i]);
            let mut next = i + c.len_utf8();
            if c == '\r' && it.peek().map(|&(_, n)| n == '\n').unwrap_or(false) {
                it.next();
                next += 1;
            }
            start = next;
        }
    }
    if start < x.len() {
        out.push(&x[start..]);
    }
    out
}

/// 一段时间的电脑活动：在电脑前的区间、前台窗口区间（起、止、app 名、bundle id）、切换次数
pub struct Activity {
    pub spans: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    pub windows: Vec<(DateTime<Utc>, DateTime<Utc>, String, String)>,
    pub switches: usize,
}

pub struct Summarizer {
    pub root: PathBuf,
}

fn seg_start(name: &str) -> DateTime<Utc> {
    NaiveDateTime::parse_from_str(name, "%Y-%m-%dT%H-%M-%SZ").map(|t| t.and_utc()).unwrap_or_default()
}

fn seg_name(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H-%M-00Z").to_string()
}

fn idle_text(mins: i64) -> String {
    format!("这 {mins} 分钟没有键鼠操作（离开、锁屏或睡眠）。")
}

/// 摘要文件管多长（分钟）：文件名里的 -10min-、-30min-；不是分段摘要的给 None
pub fn summary_minutes(file: &str) -> Option<i64> {
    let k = file.strip_suffix("-memory-summary.md")?.rsplit('-').next()?;
    k.strip_suffix("min")?.parse().ok().filter(|m| *m > 0)
}

fn hms(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M:%S").to_string()
}

/// 人 / 代理：点击、按键看 actor.kind，输入段看 actor_kind
fn actor(e: &Ev) -> String {
    let kind_of = |k: &str| e.get(k).and_then(|a| a.get("kind")).and_then(Value::as_str).unwrap_or("");
    let mut kind = s(e, "actor_kind");
    if kind.is_empty() {
        kind = kind_of("actor");
    }
    if kind.is_empty() {
        kind = kind_of("submit_actor");
    }
    if kind == "agent" {
        let mut procs: Vec<String> = match e.get("agent_procs").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            _ => vec![e.get("actor").and_then(|a| a.get("source_proc")).and_then(Value::as_str).unwrap_or("").to_string()],
        };
        procs.retain(|p| !p.is_empty());
        return if procs.is_empty() { "代理".into() } else { format!("代理·{}", procs.join("/")) };
    }
    if kind == "human" { "人".into() } else { "?".into() }
}

fn text_of(e: &Ev) -> String {
    if let Some(a) = e.get("lines").and_then(Value::as_array) {
        return a.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect::<Vec<_>>().join("\n");
    }
    s(e, "text").to_string()
}

fn is_text_event(e: &Ev) -> bool {
    let t = ty(e);
    t == "keyboard.text_input"
        || (!NOISE.contains(&t)
            && !t.starts_with("keyboard.")
            && !t.starts_with("mouse.")
            && t != "app.activated"
            && t != "heartbeat"
            && !text_of(e).trim().is_empty())
}

/// 同一「app＋窗口」只留新出现的行。用上一分段的文字预热，跨段也去重。
/// 界面文字（非键盘事件）另去掉杂项行：短于 4 字、等于窗口标题、JUNK；行首的「# 」去掉再比。
#[derive(Default)]
struct Dedup {
    seen: HashMap<(String, String), HashSet<String>>,
}

impl Dedup {
    fn feed(&mut self, e: &Ev) -> Vec<String> {
        let key = (s(e, "app_name").to_string(), s(e, "window_title").to_string());
        let ui = !ty(e).starts_with("keyboard.");
        let title = s(e, "window_title");
        let seen = self.seen.entry(key).or_default();
        let mut new = vec![];
        for line in split_lines(&text_of(e)) {
            let mut x = line.trim().to_string();
            if ui {
                x = x.trim_start_matches(['#', ' ']).trim().to_string();
                if clen(&x) < 4 || x == title || JUNK.is_match(&x) {
                    continue;
                }
            }
            if !x.is_empty() && x != "[REDACTED]" && !seen.contains(&x) {
                seen.insert(x.clone());
                new.push(x);
            }
        }
        new
    }
}

fn el_name(el: Option<&Value>) -> String {
    for k in ["title", "description", "value"] {
        let v = el.and_then(|e| e.get(k)).and_then(Value::as_str).unwrap_or("").trim();
        if !v.is_empty() {
            return cut(v, 60);
        }
    }
    String::new()
}

/// 整段都不在电脑前：有在场或心跳记录，但没有一条是 active，也没有打字和发送。
/// 电脑整夜没睡时每分钟一条离开状态的心跳，以前也会调一次模型，写出「本段无可见操作」
fn is_idle(events: &[Ev]) -> bool {
    let tracked = events.iter().any(|e| ty(e) == "presence" || ty(e) == "heartbeat");
    tracked
        && !events.iter().any(|e| {
            ((ty(e) == "presence" || ty(e) == "heartbeat") && s(e, "state") == "active")
                || ty(e) == "keyboard.text_input"
                || ty(e) == "keyboard.submit"
        })
}

/// Python 的 round：四舍六入五成双
fn py_round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

impl Summarizer {
    pub fn new(root: PathBuf) -> Self {
        Summarizer { root }
    }

    fn devices(&self) -> PathBuf {
        self.root.join("devices")
    }

    fn out_dir(&self) -> PathBuf {
        self.root.join("summaries")
    }

    fn backoff_path(&self) -> PathBuf {
        self.root.join("summary_backoff.json")
    }

    /// 模型连着失败时还要等多久（秒），不用等返回 None
    fn backoff_wait(&self) -> Option<i64> {
        let v: Value = fs::read_to_string(self.backoff_path()).ok().and_then(|t| serde_json::from_str(&t).ok())?;
        let left = v["until"].as_i64()? - Utc::now().timestamp();
        (left > 0).then_some(left)
    }

    /// 记一次失败：下一次隔 10、20、40……分钟再试，最多 2 小时（10-28 同步服务出错时，一段一天重试了 98 次）
    fn backoff_fail(&self) {
        let v: Value = fs::read_to_string(self.backoff_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let fails = v["fails"].as_i64().unwrap_or(0) + 1;
        let wait = (600 * (1i64 << (fails - 1).min(4))).min(7200);
        let _ = fs::write(self.backoff_path(), json!({"fails": fails, "until": Utc::now().timestamp() + wait}).to_string());
    }

    /// 各台电脑（Mac、Windows）的分段目录名（已排序、去重）
    pub fn segment_names(&self) -> Vec<String> {
        let mut out = BTreeSet::new();
        for dev in read_dir_sorted(&self.devices()) {
            let n = dev.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if !is_computer(&n) {
                continue;
            }
            for seg in read_dir_sorted(&dev.join("segments")) {
                let name = seg.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                if SEG_RE.is_match(&name) && seg.join("events.jsonl").is_file() {
                    out.insert(name);
                }
            }
        }
        out.into_iter().collect()
    }

    fn seg_files(&self, name: &str) -> Vec<PathBuf> {
        read_dir_sorted(&self.devices())
            .into_iter()
            .filter(|d| d.file_name().map(|x| is_computer(&x.to_string_lossy())).unwrap_or(false))
            .map(|d| d.join("segments").join(name).join("events.jsonl"))
            .filter(|p| p.is_file())
            .collect()
    }

    /// 各台电脑同一分段的事件合在一起，按解析后的时间排（旧事件带 +08:00、新的是 UTC）；坏行跳过
    fn load(&self, name: &str) -> Vec<Ev> {
        self.load_files(self.seg_files(name))
    }

    /// 只读本机这台电脑的事件：写 10 分钟摘要用。同一个 10 分钟两台电脑都在用时，各写各的，时间线上分开显示
    fn load_own(&self, name: &str) -> Vec<Ev> {
        let f = self.devices().join(beayy_core::local_device()).join("segments").join(name).join("events.jsonl");
        self.load_files(if f.is_file() { vec![f] } else { vec![] })
    }

    fn load_files(&self, files: Vec<PathBuf>) -> Vec<Ev> {
        let mut out = vec![];
        for p in files {
            let bytes = fs::read(&p).unwrap_or_default();
            for line in String::from_utf8_lossy(&bytes).lines() {
                if let Ok(Value::Object(e)) = serde_json::from_str::<Value>(line) {
                    if DateTime::parse_from_rfc3339(s(&e, "timestamp")).is_ok() {
                        out.push(e);
                    }
                }
            }
        }
        out.sort_by_key(ts);
        out
    }

    // ---------- 1. 时长表 ----------

    /// 某时段在电脑前的区间和前台窗口区间求交（同 脚本/beayy时长.py 的 query），返回 [(起, 止, app)]
    fn windows(&self, start: DateTime<Utc>, end: DateTime<Utc>, names: &[String]) -> Vec<(DateTime<Utc>, DateTime<Utc>, String)> {
        self.compute(start, end, names).windows.into_iter().map(|(a, b, n, _)| (a, b, n)).collect()
    }

    /// 某时段的电脑活动（页面的实时读数、规矩对照用）
    pub fn activity(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Activity {
        self.compute(start, end, &self.segment_names())
    }

    fn compute(&self, start: DateTime<Utc>, end: DateTime<Utc>, names: &[String]) -> Activity {
        let mut ev = vec![];
        for n in names {
            let slot = seg_start(n);
            if slot + Duration::minutes(10) <= start || slot >= end {
                continue;
            }
            ev.extend(self.load(n).into_iter().filter(|e| ts(e) >= start && ts(e) < end + Duration::minutes(10)));
        }
        ev.sort_by_key(ts);
        let stop = end.min(Utc::now());

        // 在电脑前的区间
        let mut spans = vec![];
        let mut cur: Option<DateTime<Utc>> = None;
        for e in &ev {
            let t = ts(e);
            let typ = ty(e);
            if typ == "presence" {
                if s(e, "state") == "active" {
                    cur = cur.or(Some(t));
                } else if let Some(c) = cur {
                    let idle = e.get("idle_seconds").and_then(Value::as_f64).unwrap_or(IDLE_THRESHOLD as f64);
                    spans.push((c, c.max(t - Duration::milliseconds((idle * 1000.0) as i64))));
                    cur = None;
                }
            } else if LEAVE_TYPES.contains(&typ) && cur.is_some() {
                spans.push((cur.unwrap(), t));
                cur = None;
            } else if BACK_TYPES.contains(&typ)
                || (typ == "heartbeat" && s(e, "state") == "active" && s(e, "bundle_id") != "com.apple.loginwindow")
            {
                cur = cur.or(Some(t));
            }
        }
        if let Some(c) = cur {
            let last = ev.last().map(ts).unwrap_or(c);
            spans.push((c, stop.min(last + Duration::seconds(60))));
        }
        let spans: Vec<_> = spans.into_iter().map(|(a, b)| (a.max(start), b.min(end))).filter(|(a, b)| b > a).collect();

        // 前台窗口点
        let points: Vec<(DateTime<Utc>, (String, String))> = ev
            .iter()
            .filter(|e| POINT_TYPES.contains(&ty(e)) && !s(e, "app_name").is_empty() && s(e, "bundle_id") != "com.apple.loginwindow")
            .map(|e| (ts(e), (s(e, "app_name").to_string(), s(e, "bundle_id").to_string())))
            .collect();
        let switches = ev.iter().filter(|e| ty(e) == "app.activated" && ts(e) >= start && ts(e) < end).count();
        let mut intervals = vec![];
        for w in points.windows(2) {
            let ((t0, d0), (t1, _)) = (&w[0], &w[1]);
            if (*t1 - *t0).num_milliseconds() <= MAX_GAP * 1000 {
                intervals.push((*t0, *t1, d0.clone()));
            } else {
                intervals.push((*t0, *t0 + Duration::seconds(60), d0.clone()));
            }
        }
        if let Some((t0, d0)) = points.last() {
            intervals.push((*t0, stop.min(*t0 + Duration::seconds(60)), d0.clone()));
        }
        let mut out = vec![];
        for (a0, b0, d) in &intervals {
            for (a, b) in &spans {
                if b <= a0 || a >= b0 {
                    continue;
                }
                out.push(((*a0).max(*a), (*b0).min(*b), d.0.clone(), d.1.clone()));
            }
        }
        Activity { spans, windows: out, switches }
    }

    /// 各 app 前台秒数。有 presence/heartbeat 的分段：切换区间 ∩ 在电脑前，心跳兜底；
    /// 早期分段没有这两类事件，退回只按 app.activated 区间算（开头沿用往前最近一次切换，不扣空闲）。
    fn durations(&self, name: &str, events: &[Ev], names: &[String], mins: i64) -> (Vec<(String, f64)>, HashMap<String, String>, &'static str) {
        let start = seg_start(name);
        let end = start + Duration::minutes(mins);
        let mut bundles: HashMap<String, String> = HashMap::new();
        for e in events {
            if !s(e, "app_name").is_empty() && !s(e, "bundle_id").is_empty() {
                bundles.insert(s(e, "app_name").into(), s(e, "bundle_id").into());
            }
        }
        let mut sec: Vec<(String, f64)> = vec![];
        let add = |sec: &mut Vec<(String, f64)>, k: &str, v: f64| match sec.iter_mut().find(|(n, _)| n == k) {
            Some(x) => x.1 += v,
            None => sec.push((k.to_string(), v)),
        };
        if events.iter().any(|e| ty(e) == "presence" || ty(e) == "heartbeat") {
            for (a, b, app) in self.windows(start - Duration::minutes(30), end, names) {
                let (a, b) = (a.max(start), b.min(end));
                if b > a {
                    add(&mut sec, &app, (b - a).num_microseconds().unwrap_or(0) as f64 / 1e6);
                }
            }
            let here: f64 = sec.iter().map(|x| x.1).sum();
            let all = (mins * 60) as f64;
            if here < all - 5.0 {
                sec.push(("（不在电脑前或没在采）".into(), all - here));
            }
            sec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            return (sec, bundles, "按切换与在电脑前区间求交，心跳兜底");
        }
        let mut cur: Option<Ev> = None;
        let prev: Vec<&String> = names.iter().filter(|n| n.as_str() < name).collect();
        for p in prev.iter().rev().take(6) {
            // 往前最多找 1 小时
            if let Some(a) = self.load_own(p).into_iter().filter(|e| ty(e) == "app.activated").last() {
                cur = Some(a);
                break;
            }
        }
        let mut t = start;
        let acts: Vec<&Ev> = events.iter().filter(|e| ty(e) == "app.activated").collect();
        for i in 0..=acts.len() {
            let e = acts.get(i).copied();
            let t2 = e.map(|e| ts(e).min(end)).unwrap_or(end);
            if t2 > t {
                let d = (t2 - t).num_microseconds().unwrap_or(0) as f64 / 1e6;
                match &cur {
                    Some(c) => {
                        let k = [s(c, "app_name"), s(c, "bundle_id")].into_iter().find(|x| !x.is_empty()).unwrap_or("?").to_string();
                        add(&mut sec, &k, d);
                        bundles.entry(k).or_insert_with(|| s(c, "bundle_id").to_string());
                    }
                    None => add(&mut sec, "（未知）", d),
                }
            }
            if let Some(e) = e {
                cur = Some(e.clone());
                t = t.max(t2);
            }
        }
        sec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        (sec, bundles, "只按切换事件算，未扣空闲")
    }

    // ---------- 3. 拼包 ----------

    /// 拼好的包、时长表、applications。mins＝这份摘要管多长（从 name 这个分段起），events 是这段时间本机的事件
    pub fn build_pack(&self, name: &str, events: &[Ev], names: &[String], mins: i64) -> (String, Vec<String>, i64) {
        let start = seg_start(name);
        let (rows, bundles, how) = self.durations(name, events, names, mins);
        let mut dd = Dedup::default();
        if let Some(prev) = names.iter().filter(|n| n.as_str() < name).last() {
            for e in self.load_own(prev) {
                if is_text_event(&e) {
                    dd.feed(&e);
                }
            }
        }

        // 先收成 (时刻, 种类, 正文, 窗口)，界面文字最后按预算截；正文里的「@窗口」和上一条同一窗口时写成「@同上」
        let mut items: Vec<(String, &'static str, String, String)> = vec![];
        let mut clicks: Vec<(String, String, String, String, String)> = vec![];
        let mut others: Vec<(String, usize)> = vec![];
        let mut apps: Vec<String> = vec![];
        let mut urls: HashSet<String> = HashSet::new();
        let mut last_submit: Option<String> = None;
        for e in events {
            let t = ty(e);
            let hm = hms(ts(e));
            let mut wh = s(e, "app_name").to_string();
            let wt = s(e, "window_title");
            if !wt.is_empty() && wt != wh {
                wh += &format!("｜{}", cut(wt, 60));
            }
            let b = s(e, "bundle_id");
            if !b.is_empty() && !apps.iter().any(|a| a == b) {
                apps.push(b.into());
            }
            let u = s(e, "url");
            let url = if u.starts_with("http") && !urls.contains(u) { Some(u.to_string()) } else { None };
            if let Some(u) = &url {
                urls.insert(u.clone());
            }
            if t == "app.activated" {
                items.push((hm, "", format!("切到 {wh}"), wh.clone()));
            } else if t == "keyboard.submit" {
                let text = text_of(e);
                if last_submit.as_deref() == Some(text.as_str()) {
                    continue; // 同一条发送偶尔记两次
                }
                last_submit = Some(text.clone());
                let typed = s(e, "typed");
                let tail = if typed.is_empty() { String::new() } else { format!("（按键原文 {}）", cut(typed, 80)) };
                items.push((hm, "", format!("[{}] 发送 @{wh}：{}{tail}", actor(e), cut(&text, 500)), wh.clone()));
                dd.feed(e);
            } else if is_text_event(e) {
                let new = dd.feed(e);
                if t == "keyboard.text_input" {
                    if !new.is_empty() {
                        items.push((hm, "", format!("[{}] 输入 @{wh}：", actor(e)) + &cut(&new.join(" / "), 1500), wh.clone()));
                    } else if !s(e, "typed").is_empty() {
                        items.push((hm, "", format!("[{}] 输入 @{wh}：（读不到输入框，按键原文 {}）", actor(e), cut(s(e, "typed"), 120)), wh.clone()));
                    }
                } else if !new.is_empty() || url.is_some() {
                    let u = url.as_ref().map(|u| format!("（{}）", cut(u, 200))).unwrap_or_default();
                    items.push((hm, "ui", format!("界面 @{wh}{u}：{}", new.join(" / ")), wh.clone()));
                }
            } else if t == "keyboard.shortcut" {
                items.push((hm, "", format!("[{}] 快捷键 {} @{wh}", actor(e), s(e, "keys")), wh.clone()));
            } else if t == "mouse.click" {
                let n = el_name(e.get("element"));
                if !n.is_empty() {
                    let role = e.get("element").and_then(|x| x.get("role")).and_then(Value::as_str).unwrap_or("");
                    clicks.push((hm, actor(e), wh, role.strip_prefix("AX").unwrap_or(role).to_string(), n));
                }
            } else if t == "presence" {
                items.push((hm, "", if s(e, "state") == "idle" { "离开电脑".into() } else { "回到电脑前".into() }, String::new()));
            } else if let Some(label) = match t {
                "system.sleep" => Some("睡眠"),
                "system.wake" => Some("唤醒"),
                "screen.sleep" => Some("屏幕关"),
                "screen.wake" => Some("屏幕开"),
                "screen.locked" => Some("锁屏"),
                "screen.unlocked" => Some("解锁"),
                _ => None,
            } {
                items.push((hm, "", label.into(), String::new()));
            } else if let Some(u) = &url {
                items.push((hm, "", format!("网址 @{wh}：{}", cut(u, 200)), wh.clone()));
            } else if !NOISE.contains(&t) && !["keyboard.return", "heartbeat", "window.changed"].contains(&t) {
                match others.iter_mut().find(|(k, _)| k == t) {
                    Some(x) => x.1 += 1,
                    None => others.push((t.to_string(), 1)),
                }
            }
        }

        // 点击：连续落在同一 app＋窗口的合成一行
        let mut click_lines: Vec<((String, String), Vec<String>, String)> = vec![];
        for (hm, who, wh, role, n) in clicks {
            let label = format!("{role}「{n}」");
            match click_lines.last_mut() {
                Some(last) if last.0 == (who.clone(), wh.clone()) => {
                    if last.1.last() != Some(&label) {
                        last.1.push(label); // 连点同一处只记一次
                    }
                }
                _ => click_lines.push(((who, wh), vec![label], hm)),
            }
        }
        for ((who, wh), its, hm) in click_lines {
            let text = format!("[{who}] 点击 @{wh}：") + &its.iter().take(30).cloned().collect::<Vec<_>>().join("、");
            items.push((hm, "", text, wh));
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));

        let end = start + Duration::minutes(mins);
        let mut head = vec![
            format!(
                "时段：{}–{}（北京时间）",
                start.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
                end.with_timezone(&Local).format("%H:%M")
            ),
            String::new(),
            format!("前台时长（代码{how}）："),
        ];
        head.extend(rows.iter().map(|(k, v)| format!("- {k}：{} 秒", py_round(*v))));
        if !others.is_empty() {
            head.push(String::new());
            head.push("其他事件计数：".to_string() + &others.iter().map(|(k, v)| format!("{k}×{v}")).collect::<Vec<_>>().join("，"));
        }
        head.push(String::new());
        head.push("时间线（[人]＝用户本人键鼠，[代理]＝AI 等程序合成的键鼠；「界面」＝窗口里新出现的文字；「@同上」＝和上一条同一个窗口）：".into());

        // 超预算先压界面文字，再压其他长行，最后丢中间（丢掉的前后各自从头写窗口，「@同上」不会指到丢掉的那条）
        let lines = |its: &[(String, &'static str, String, String)], ui_cap: usize, cap: usize| -> Vec<String> {
            let mut prev = "";
            its.iter()
                .map(|(hm, k, x, wh)| {
                    let x = if !wh.is_empty() && wh == prev && clen(wh) > 8 { x.replacen(&format!("@{wh}"), "@同上", 1) } else { x.clone() };
                    if !wh.is_empty() {
                        prev = wh;
                    }
                    format!("{hm} {}", cut(&x, if *k == "ui" { ui_cap } else { cap }))
                })
                .collect()
        };
        let tail = self.speech_and_place(start, end);
        // 间隔调长后包的上限按时长的平方根放宽：1 小时约 2.4 万字（6 份 10 分钟的上限是 6 万），省 token，又不至于丢掉中间一大段
        let budget = (PACK_CHARS as f64 * (mins as f64 / 10.0).sqrt()) as usize;
        let join = |parts: Vec<Vec<String>>| head.iter().cloned().chain(parts.into_iter().flatten()).chain(tail.iter().cloned()).collect::<Vec<_>>().join("\n");
        let (mut ui_cap, mut cap) = (2000, 2000);
        let mut pack = String::new();
        for (u, c) in [(2000, 2000), (1000, 2000), (500, 600), (250, 300), (120, 300)] {
            (ui_cap, cap) = (u, c);
            pack = join(vec![lines(&items, u, c)]);
            if clen(&pack) <= budget {
                break;
            }
        }
        // 从正中间往两边一次丢 10 条，直到放得下（同原来：至少留 20 条）
        let mut drop = 0;
        while clen(&pack) > budget && items.len() - drop > 20 {
            drop = (drop + 10).min(items.len());
            let a = (items.len() - drop) / 2;
            pack = join(vec![lines(&items[..a], ui_cap, cap), vec!["……（中间略）".into()], lines(&items[a + drop..], ui_cap, cap)]);
        }
        for (k, _) in &rows {
            if let Some(b) = bundles.get(k) {
                if !b.is_empty() && !apps.contains(b) {
                    apps.push(b.clone());
                }
            }
        }
        let active = rows.iter().filter(|(k, _)| !k.starts_with('（')).map(|(_, v)| v).sum::<f64>().round() as i64;
        (pack, apps, active)
    }

    /// 某种设备（watch-、iphone-）在 [from, to] 这几个分段里的某类事件
    fn device_events(&self, prefix: &str, ty_: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Ev> {
        let (lo, hi) = (beayy_core::segment_key(from), beayy_core::segment_key(to));
        let mut out = vec![];
        for dev in read_dir_sorted(&self.devices()) {
            if !dev.file_name().map(|x| x.to_string_lossy().starts_with(prefix)).unwrap_or(false) {
                continue;
            }
            for seg in read_dir_sorted(&dev.join("segments")) {
                let n = seg.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                if n < lo || n > hi {
                    continue;
                }
                let bytes = fs::read(seg.join("events.jsonl")).unwrap_or_default();
                for line in String::from_utf8_lossy(&bytes).lines().filter(|l| l.contains(ty_)) {
                    if let Ok(Value::Object(e)) = serde_json::from_str::<Value>(line) {
                        if ty(&e) == ty_ {
                            out.push(e);
                        }
                    }
                }
            }
        }
        out.sort_by_key(ts);
        out
    }

    /// 摘要包末尾：这段时间在哪（iPhone、手表的停留点）、手表录到的对话原文（同一个人连着说的并成一行）
    fn speech_and_place(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<String> {
        let mut out = vec![];
        let t = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|x| DateTime::parse_from_rfc3339(x).ok()).map(|x| x.with_timezone(&Utc));
        // 地点：同一个停留点取最后一版，和这 10 分钟有重叠的
        let mut visits: Vec<(DateTime<Utc>, Option<DateTime<Utc>>, String)> = vec![];
        let mut latest: HashMap<String, Ev> = HashMap::new();
        // iPhone 和手表都可能记；手表的只在 iPhone 前后两天一条都没有时才用（以 iPhone 为准）
        for prefix in ["iphone-", "watch-"] {
            let have = !latest.is_empty();
            for e in self.device_events(prefix, "place.visit", start - Duration::days(2), end) {
                if prefix == "watch-" && have {
                    break;
                }
                latest.insert(s(&e, "place_id").to_string(), e);
            }
        }
        for e in latest.values() {
            let (a, z) = (t(e.get("arrive")).or(t(e.get("depart"))), t(e.get("depart")));
            let Some(a) = a else { continue };
            if a < end && z.map(|z| z > start).unwrap_or(true) {
                let name = [s(e, "label"), s(e, "name")].into_iter().find(|x| !x.is_empty()).unwrap_or("某处（没有地名）").to_string();
                visits.push((a, z, name));
            }
        }
        visits.sort_by_key(|v| v.0);
        if !visits.is_empty() {
            out.push(String::new());
            let hm = |x: DateTime<Utc>| x.with_timezone(&Local).format("%H:%M").to_string();
            let what = visits.iter().map(|(a, z, n)| format!("「{n}」（{} 起{}）", hm(*a), z.map(|z| format!("，{} 离开", hm(z))).unwrap_or_default())).collect::<Vec<_>>().join("、");
            out.push(format!("地点（停留点）：{what}"));
        }
        // 对话：开口时刻在这 10 分钟里的
        let mut lines: Vec<(DateTime<Utc>, DateTime<Utc>, String, String)> = vec![];
        let mut seen = HashSet::new();
        for e in self.device_events("watch-", "speech.utterance", start, end) {
            let Some(a) = t(e.get("start")).filter(|a| *a >= start && *a < end) else { continue };
            let z = t(e.get("end")).unwrap_or(a);
            let (who, text) = (s(&e, "speaker").to_string(), s(&e, "text").to_string());
            if !seen.insert((a, text.clone())) {
                continue; // 两台设备录到同一句
            }
            match lines.last_mut() {
                Some(l) if l.2 == who && (a - l.1).num_seconds() <= SPEECH_MERGE => {
                    l.3 += &text;
                    l.1 = z;
                }
                _ => lines.push((a, z, who, text)),
            }
        }
        if !lines.is_empty() {
            let me: Vec<String> = lines.iter().map(|l| l.2.clone()).filter(|w| !w.starts_with("他人") && w != "未辨").collect::<BTreeSet<_>>().into_iter().collect();
            let legend = if me.is_empty() { String::new() } else { format!("「{}」是用户本人，", me.join("」「")) };
            out.push(String::new());
            out.push(format!("对话（手表录音转写，可能有错字和同音字；{legend}「他人N」是当天编号的别人，「未辨」是太短没认出）："));
            let mut body: Vec<String> = lines.iter().map(|(a, _, w, x)| format!("{} [{w}] {x}", hms(*a))).collect();
            for cap in [400, 200, 100] {
                if body.iter().map(|x| clen(x)).sum::<usize>() <= SPEECH_CHARS {
                    break;
                }
                body = lines.iter().map(|(a, _, w, x)| format!("{} [{w}] {}", hms(*a), cut(x, cap))).collect();
            }
            let mut cut_at = None;
            while body.iter().map(|x| clen(x)).sum::<usize>() > SPEECH_CHARS && body.len() > 10 {
                let mid = body.len() / 2;
                body.drain(mid - 2..mid + 2);
                cut_at = Some(mid - 2);
            }
            if let Some(i) = cut_at {
                body.insert(i, "……（中间略）".into());
            }
            out.extend(body);
        }
        out
    }

    /// 手表在录、这一段的转写还没拉回来：先不写摘要。最近 3 小时没有录音（没戴、没开）就不等
    fn speech_pending(&self, slot_end: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        if now - slot_end > Duration::minutes(SPEECH_WAIT_MIN) {
            return false;
        }
        let covered = self
            .device_events("watch-", "speech.segment", now - Duration::hours(3), now)
            .iter()
            .filter_map(|e| e.get("end").and_then(Value::as_str).and_then(|x| DateTime::parse_from_rfc3339(x).ok()))
            .map(|x| x.with_timezone(&Utc))
            .max();
        covered.map(|c| c < slot_end).unwrap_or(false)
    }

    // ---------- 4. 调模型 ----------

    fn settings(&self) -> (String, String) {
        let raw = self.raw_settings();
        let get = |k: &str, d: &str| raw.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or(d).to_string();
        (get("gateway", GATEWAY), get("summary_model", MODEL))
    }

    /// AI 总结用哪种语言写：settings.json 的 summary_language（zh／en）；没选＝跟随界面语言（界面启动时报的 ui_lang），再没有就中文
    pub fn out_lang(&self) -> &'static str {
        let s = self.raw_settings();
        let pick = |k: &str| s.get(k).and_then(Value::as_str).filter(|v| *v == "zh" || *v == "en");
        if pick("summary_language").or(pick("ui_lang")) == Some("en") { "en" } else { "zh" }
    }

    fn raw_settings(&self) -> Map<String, Value> {
        fs::read_to_string(self.root.join("settings.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// 用官方模型（settings.json 的 model_mode＝official）：经同步服务代调，按账号开通，不用自己的密钥
    fn official(&self) -> bool {
        self.raw_settings().get("model_mode").and_then(Value::as_str) == Some("official")
    }

    fn call_model(&self, pack: &str, prior: &str, known: &[String], mins: i64) -> Result<(Map<String, Value>, Value), String> {
        let mut user = String::new();
        if !known.is_empty() {
            user += &format!("已有的项目名（同一件事请沿用这里的写法）：{}\n\n", known.join("、"));
        }
        if !prior.is_empty() {
            user += &format!("上一段摘要：{prior}\n\n");
        }
        user += pack;
        self.chat("segment", &format!("{}{ACTIVITY_GUIDE}", PROMPT.replace("10 分钟", &format!("{mins} 分钟"))), &user)
    }

    /// 给别处用：问一次模型，要一个 JSON 对象回来
    pub fn ask(&self, system: &str, user: &str) -> Result<Map<String, Value>, String> {
        self.chat("other", system, user).map(|x| x.0)
    }

    /// 同 ask，用量记在 kind 名下（daily、weekly、monthly……，设置里看模型用量按它分）
    pub fn ask_as(&self, kind: &str, system: &str, user: &str) -> Result<Map<String, Value>, String> {
        self.chat(kind, system, user).map(|x| x.0)
    }

    /// 要一段文字回来（不套 JSON），温度自己定：棒喝用，温度低了天天一个样，温度一高模型又常忘了套 JSON
    pub fn ask_text(&self, kind: &str, system: &str, user: &str, temperature: f64) -> Result<String, String> {
        self.chat_t(kind, system, user, temperature, false).map(|x| x.0["text"].as_str().unwrap_or("").to_string())
    }

    /// 记一次模型用量：logs/llm_usage.jsonl 一行 {at, kind, model, official, in, out}
    fn note_usage(&self, kind: &str, model: &str, usage: &Value) {
        let n = |k: &str| usage.get(k).and_then(Value::as_i64).unwrap_or(0);
        let line = json!({"at": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true), "kind": kind, "model": model,
            "official": self.official(), "in": n("prompt_tokens"), "out": n("completion_tokens")});
        let dir = self.root.join("logs");
        let _ = fs::create_dir_all(&dir);
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("llm_usage.jsonl")) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }

    /// 调网关，要一个 JSON 对象回来；返回（对象，用量）。kind 是这次干什么用的，记用量用
    fn chat(&self, kind: &str, system: &str, user: &str) -> Result<(Map<String, Value>, Value), String> {
        self.chat_t(kind, system, user, 0.3, true)
    }

    /// json＝false 时不解析，原文放在 {"text": …} 里回来
    fn chat_t(&self, kind: &str, system: &str, user: &str, temperature: f64, json: bool) -> Result<(Map<String, Value>, Value), String> {
        let (gateway, model) = self.settings();
        // 官方：地址是同步服务的代理，认证用账号令牌（模型由服务器定）；自己的网关：地址、模型、密钥都用设置里的
        let (url, auth) = if self.official() {
            beayy_sync::official_llm(&beayy_core::data_root()).ok_or("用官方模型要先登录账号：设置 → 账号")?
        } else {
            if model.is_empty() {
                return Err("还没设模型：到设置 → 连接 → 模型 里填".into());
            }
            (format!("{}/chat/completions", gateway.trim_end_matches('/')), format!("Bearer {}", api_key()?))
        };
        // AI 总结的语言（设置 → 通用）：给人看的那几种（10 分钟摘要、小时摘要、日周月总结）照设置写；项目名归并、打标签这些内部用的不管
        let system = if self.out_lang() == "en" && ["segment", "hourly", "daily", "weekly", "monthly"].contains(&kind) {
            format!("{system}\n{EN_NOTE}")
        } else {
            system.to_string()
        };
        let body = json!({"model": model, "temperature": temperature, "messages": [
            {"role": "system", "content": system}, {"role": "user", "content": user}]});
        let agent: ureq::Agent = beayy_sync::http_agent(ureq::Agent::config_builder()
            .proxy(None)
            .tls_config(beayy_core::tls!())
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            // 出错时也读返回体：官方模型的没开通、额度用完写在里面
            .http_status_as_error(false)
            .build());
        let fence = Regex::new(r"^```(?:json)?\s*|\s*```$").unwrap();
        let mut err = String::new();
        for _ in 0..3 {
            // 网关偶发 5xx、模型偶发不出 JSON：重试
            let r = agent
                .post(&url)
                .header("Content-Type", "application/json")
                .header("User-Agent", UA)
                .header("Authorization", &auth)
                .send_json(&body);
            let r: Result<Value, String> = r.map_err(|e| e.to_string()).and_then(|mut resp| resp.body_mut().read_json::<Value>().map_err(|e| e.to_string()));
            // 官方模型没开通、停用、额度用完：服务器给的是人话，直接报，不重试
            if let Ok(v) = &r {
                if v.get("choices").is_none() {
                    if let Some(e) = v["error"].as_str() {
                        return Err(e.to_string());
                    }
                }
            }
            match r {
                Ok(v) => {
                    // 没出 JSON 重试的那几次也花了 token，一样记
                    let usage = v.get("usage").cloned().unwrap_or(json!({}));
                    self.note_usage(kind, v["model"].as_str().unwrap_or(&model), &usage);
                    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_string();
                    let text = fence.replace_all(&text, "").to_string();
                    if !json {
                        if text.is_empty() {
                            err = "模型什么都没返回".into();
                            continue;
                        }
                        return Ok((Map::from_iter([("text".to_string(), Value::String(text))]), usage));
                    }
                    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
                        err = format!("模型没返回 JSON：{}", cut(&text, 200));
                        continue;
                    };
                    match serde_json::from_str::<Value>(&text[a..=b]) {
                        Ok(Value::Object(j)) => return Ok((j, usage)),
                        Ok(_) => err = "模型返回的不是对象".into(),
                        Err(e) => err = e.to_string(),
                    }
                }
                Err(e) => err = e,
            }
        }
        Err(format!("模型调用失败：{err}"))
    }

    // ---------- 5. 写 Markdown ----------

    /// 管到这个分段的摘要文件：从它起的，或从同一小时更早的分段起、管得够长的（间隔调长后一份管几个分段）
    pub fn existing(&self, name: &str) -> Vec<PathBuf> {
        let t = seg_start(name);
        read_dir_sorted(&self.out_dir())
            .into_iter()
            .filter(|p| {
                let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                let Some(mins) = summary_minutes(&f) else { return false };
                (0..6).any(|j| mins > 10 * j && f.starts_with(&format!("{}-", seg_name(t - Duration::minutes(10 * j)).trim_end_matches('Z'))))
            })
            .collect()
    }

    /// 摘要间隔（分钟）：settings.json 的 summary_minutes，只认 EVERY 里的，默认 10
    pub fn every(&self) -> i64 {
        self.raw_settings().get("summary_minutes").and_then(Value::as_i64).filter(|m| EVERY.contains(m)).unwrap_or(10)
    }

    fn prior_summary(&self, name: &str, names: &[String]) -> String {
        let re = Regex::new(r"(?m)^description: (.*)$").unwrap();
        if let Some(prev) = names.iter().filter(|n| n.as_str() < name).last() {
            for p in self.existing(prev) {
                if let Some(c) = fs::read_to_string(&p).ok().and_then(|t| re.captures(&t).map(|c| c[1].to_string())) {
                    return c;
                }
            }
        }
        String::new()
    }

    fn render(&self, segs: &[String], j: &Map<String, Value>, apps: &[String], active: i64) -> String {
        let one = |v: Option<&Value>| -> String {
            let x = match v {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                Some(o) => o.to_string(),
            };
            x.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        let mut md = vec![
            "---".to_string(),
            format!("title: {}", one(j.get("title"))),
            format!("description: {}", one(j.get("description"))),
            format!("applications: [{}]", apps.join(", ")),
            format!("active_sec: {active}"),
            format!("parts: {}", Value::Array(norm_parts(j.get("parts")))),
            "---".into(),
            String::new(),
        ];
        // 10-30 起模型不再写 memory_summary、prior_context（和「经过」重复、没人读，省输出 token）；不写空段，免得读的一方把下一节当成它
        let memory = match j.get("memory_summary") {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Null) | None => String::new(),
            Some(o) => o.to_string(),
        };
        if !memory.is_empty() {
            md.extend(["## Memory summary".into(), String::new(), memory, String::new()]);
        }
        let prior = one(j.get("prior_context"));
        if !prior.is_empty() {
            md.extend(["### Relevant prior context".into(), String::new(), prior, String::new()]);
        }
        let ctx: Vec<String> = j.get("context").and_then(Value::as_array).map(|a| a.iter().map(|c| one(Some(c))).filter(|c| !c.is_empty()).collect()).unwrap_or_default();
        if !ctx.is_empty() {
            md.push("### Important non-obvious context about the user".into());
            md.push(String::new());
            md.extend(ctx.iter().map(|c| format!("- {c}")));
            md.push(String::new());
        }
        md.push("## Recording summary".into());
        md.push(String::new());
        for sec in j.get("recording").and_then(Value::as_array).cloned().unwrap_or_default() {
            md.push(format!("### {}", one(sec.get("heading"))));
            md.push(String::new());
            for b in sec.get("bullets").and_then(Value::as_array).cloned().unwrap_or_default() {
                md.push(format!("- {}", one(Some(&b))));
            }
            md.push(String::new());
        }
        md.push("## Citations".into());
        md.push(String::new());
        md.extend(segs.iter().flat_map(|n| self.seg_files(n)).map(|p| format!("- {}", p.display())));
        md.push(String::new());
        md.join("\n")
    }

    /// 写一份「不在电脑前」的固定摘要（不调模型），换掉这几段已有的
    fn write_idle(&self, segs: &[String], mins: i64) -> bool {
        let text = idle_text(mins);
        let j: Map<String, Value> = serde_json::from_value(json!({
            "title": IDLE_TITLE, "description": text, "memory_summary": text,
            "prior_context": "", "context": [], "recording": []
        }))
        .unwrap();
        for p in segs.iter().flat_map(|n| self.existing(n)) {
            let _ = fs::remove_file(p);
        }
        let out = self.out_dir().join(format!("{}-{}-{mins}min-memory-summary.md", segs[0].trim_end_matches('Z'), suffix()));
        fs::write(&out, self.render(segs, &j, &[], 0)).is_ok()
    }

    /// 把已经写过、但整段不在电脑前的分段换成固定摘要（以前每段都调了模型）。返回换掉的段数
    pub fn redo_idle(&self) -> usize {
        let mut n = 0;
        for name in self.segment_names() {
            // 只换正好管这 10 分钟的；管几段的那种（间隔调长后写的）里别的段可能有活动
            let old: Vec<PathBuf> = self.existing(&name).into_iter().filter(|p| p.file_name().map(|f| f.to_string_lossy().starts_with(name.trim_end_matches('Z')) && summary_minutes(&f.to_string_lossy()) == Some(10)).unwrap_or(false)).collect();
            if old.is_empty() {
                continue;
            }
            let already = old.iter().any(|p| fs::read_to_string(p).map(|t| t.contains(&format!("title: {IDLE_TITLE}\n"))).unwrap_or(false));
            if !already && is_idle(&self.load(&name)) && self.write_idle(std::slice::from_ref(&name), 10) {
                n += 1;
            }
        }
        n
    }

    /// 某个分段拼好的包（调试用，不调模型）
    pub fn dry_run(&self, name: &str) -> String {
        let names = self.segment_names();
        let events = self.load(name);
        self.build_pack(name, &events, &names, 10).0
    }

    /// 用现在的提示词给某个分段问一次模型，返回（模型给的 JSON，用量），不写文件（调试用：改提示词后和已有摘要对比）
    pub fn try_segment(&self, name: &str) -> Result<(Map<String, Value>, Value), String> {
        let names = self.segment_names();
        let events = self.load_own(name);
        let (pack, _, _) = self.build_pack(name, &events, &names, 10);
        self.call_model(&pack, &self.prior_summary(name, &names), &self.known_projects(), 10)
    }

    /// 写摘要。`pick` 为空＝本机记过、已结束、还没写过的分段（同步来的别的电脑的分段由那台自己写，免得重复调模型），
    /// 按摘要间隔并成一份：同一个间隔（如 14:00–14:30）里的分段等间隔结束后一起写，问一次模型；
    /// 间隔中途改过、前面几段已经有摘要的，从第一段没写的写到间隔结束。
    /// 指定分段名时已写过的也重写（每段单独写 10 分钟的）。每份一行日志交给 `log`。
    pub fn run(&self, pick: &[String], mut log: impl FnMut(String)) -> usize {
        if pick.is_empty() {
            if let Some(left) = self.backoff_wait() {
                log(format!("模型前几次都失败了，{} 分钟后再试", (left + 59) / 60));
                return 0;
            }
        }
        let names = self.segment_names();
        let now = Utc::now();
        let mine = self.devices().join(beayy_core::local_device()).join("segments");
        // 每份：（这几个分段, 止时刻）
        let mut todo: Vec<(Vec<String>, DateTime<Utc>)> = vec![];
        if pick.is_empty() {
            let every = self.every();
            for n in names.iter().filter(|n| self.existing(n).is_empty() && mine.join(n.as_str()).join("events.jsonl").is_file()) {
                let t = seg_start(n);
                let end = t - Duration::minutes(t.minute() as i64 % every) + Duration::minutes(every);
                match todo.last_mut() {
                    Some((segs, e)) if *e == end => segs.push(n.clone()),
                    _ => todo.push((vec![n.clone()], end)),
                }
            }
            for (segs, end) in todo.iter_mut() {
                // 间隔里后面有已经写过的段（改间隔前写的）：只写到它前面
                if let Some(c) = names.iter().find(|x| x.as_str() > segs[0].as_str() && seg_start(x) < *end && !self.existing(x).is_empty()) {
                    *end = seg_start(c);
                    segs.retain(|x| seg_start(x) < *end);
                }
            }
            todo.retain(|(_, end)| *end <= now && !self.speech_pending(*end, now));
        } else {
            todo = pick.iter().map(|n| (vec![n.clone()], seg_start(n) + Duration::minutes(10))).collect();
        }
        let _ = fs::create_dir_all(self.out_dir());
        let known = self.known_projects();
        let mut written = 0;
        for (segs, end) in todo {
            let name = segs[0].clone();
            let mins = (end - seg_start(&name)).num_minutes();
            let mut events: Vec<Ev> = segs.iter().flat_map(|n| self.load_own(n)).collect();
            events.sort_by_key(ts);
            let (pack, apps, active) = self.build_pack(&name, &events, &names, mins);
            if !events.iter().any(|e| !NOISE.contains(&ty(e))) {
                log(format!("{name}：没有活动事件，跳过"));
                continue;
            }
            if is_idle(&events) {
                if self.write_idle(&segs, mins) {
                    written += 1;
                    log(format!("{name}：不在电脑前，不调模型"));
                }
                continue;
            }
            match self.call_model(&pack, &self.prior_summary(&name, &names), &known, mins) {
                Ok((j, usage)) => {
                    let _ = fs::remove_file(self.backoff_path());
                    for p in segs.iter().flat_map(|n| self.existing(n)) {
                        let _ = fs::remove_file(p); // 指定重写时换掉旧的
                    }
                    let out = self.out_dir().join(format!("{}-{}-{mins}min-memory-summary.md", name.trim_end_matches('Z'), suffix()));
                    match fs::write(&out, self.render(&segs, &j, &apps, active)) {
                        Ok(_) => {
                            written += 1;
                            let u = |k: &str| usage.get(k).map(|v| v.to_string()).unwrap_or_else(|| "?".into());
                            log(format!(
                                "{name}：包 {} 字，用量 {}+{} token → {}",
                                clen(&pack),
                                u("prompt_tokens"),
                                u("completion_tokens"),
                                out.file_name().unwrap().to_string_lossy()
                            ));
                        }
                        Err(e) => log(format!("{name}：写文件失败 {e}")),
                    }
                }
                Err(e) => {
                    // 和 Python 版一样：模型失败就停，等一会儿再补
                    log(format!("{name}：{e}"));
                    self.backoff_fail();
                    break;
                }
            }
        }
        written
    }
}

const PROMPT: &str = "你是电脑活动记录员。下面是用户电脑上 10 分钟的活动记录：前台时长、输入与发送的原文、点到的界面元素、界面文字。
请据此写这 10 分钟的摘要，全部用中文，description 里用第二人称「你」，正文用「用户」。
要求：
- 只写材料里看得到的事，不编造；看不到的就不写，不要写「材料未显示」「不确定」这类话。不写具体秒数和次数（数字由程序另列）。
- [代理] 标的是 AI 或程序代为操作，要和用户本人的操作分开写。
- 材料末尾如有「地点」（iPhone 停留点）和「对话」（手表录音转写），也写进摘要，并注明来源，如「（录音）」「（位置）」；转写有错字，意思不明的不硬猜、不逐字引用。没有电脑操作、只有对话的，就写这段对话。
- 只返回一个 JSON 对象，不要代码块，字段：
  title: 15 字以内的标题
  description: 一两句，概括你这 10 分钟在做什么、有什么结果
  context: 3–6 条字符串数组，每条「名称：说明」，写出现的文档、项目、会话、联系人这类以后回看需要的线索
  recording: 1–3 个对象的数组，每个 {\"heading\": 小标题, \"bullets\": [2–5 条具体经过]}
  parts: 0–3 个对象的数组，这 10 分钟的时间花在哪，每个 {\"activity\": 二级类型, \"domain\": 领域, \"project\": 项目, \"share\": 占比 0–1}，占比加起来是 1；人不在电脑前（离开电脑、只有后台程序在跑）返回空数组 []
";

/// 类型（在做什么）：两级，互不重叠、合起来不遗漏，只按行为和产出分（为了什么交给领域）。判定顺序见 ACTIVITY_GUIDE。
/// 2026-10-29 调整：加「决策」（和 AI 讨论方案、审阅产出原先散在调研、事务、规划、整理里，约占两成）；
/// 二级改准：调研、学习按「目的」分 → 检索、阅读按行为分；整理 → 配置维护；社交媒体 → 刷信息流；一级休闲 → 消遣（和领域「休闲」分开）
pub const ACTIVITY_TREE: &[(&str, &[&str])] = &[
    ("创作", &["编码", "写作", "设计", "数据分析"]),
    ("决策", &["方案讨论", "评审验收"]),
    ("沟通", &["即时消息", "邮件", "会议通话"]),
    ("获取", &["检索", "阅读"]),
    ("管理", &["规划", "事务", "配置维护"]),
    ("消遣", &["影音", "刷信息流", "游戏"]),
    ("其他", &["其他"]),
];

/// 项目的一级：领域（为了什么）。二级项目从内容里提取，每个项目只属于一个领域
pub const DOMAINS: &[&str] = &["工作", "成长", "生活", "健康", "关系", "休闲"];

/// 二级类型 → 一级
pub fn activity_group(act: &str) -> &'static str {
    ACTIVITY_TREE.iter().find(|(g, xs)| xs.contains(&act) || *g == act).map(|(g, _)| *g).unwrap_or("其他")
}

fn is_activity(act: &str) -> bool {
    ACTIVITY_TREE.iter().any(|(_, xs)| xs.contains(&act))
}

/// 模型偶尔不照原词填（带编号、括号说明，或用旧叫法），尽量认出来；认不出才算「其他」
pub fn canon_activity(raw: &str) -> String {
    let s: String = raw.split(['（', '(']).next().unwrap_or("").trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ').trim().to_string();
    // 只给了一级（如「创作」）：按一级记，统计时显示成这一类下面的「未细分」
    if is_activity(&s) || ACTIVITY_TREE.iter().any(|(g, _)| *g == s) {
        return s;
    }
    if let Some(x) = ACTIVITY_TREE.iter().flat_map(|(_, xs)| xs.iter()).find(|x| **x != "其他" && s.contains(**x)) {
        return x.to_string();
    }
    // 旧叫法（含 2026-10-29 改名前的二级、一级）归到现在的二级；按顺序匹配，先到先得
    const OLD: &[(&str, &str)] = &[
        ("调研", "检索"), ("学习", "阅读"), ("整理", "配置维护"), ("社交媒体", "刷信息流"), ("休闲", "消遣"),
        ("讨论", "方案讨论"), ("审阅", "评审验收"), ("评审", "评审验收"), ("验收", "评审验收"),
        ("开发", "编码"), ("代码", "编码"), ("调试", "编码"), ("部署", "编码"), ("测试", "编码"), ("数据", "数据分析"), ("会议", "会议通话"), ("通话", "会议通话"),
        ("沟通", "即时消息"), ("聊天", "即时消息"), ("娱乐", "影音"), ("视频", "影音"), ("看剧", "影音"), ("搜索", "检索"), ("浏览", "阅读"), ("文档", "写作"),
        ("配置", "配置维护"), ("安排", "规划"),
    ];
    OLD.iter().find(|(k, _)| s.contains(k)).map(|(_, v)| v.to_string()).unwrap_or_else(|| "其他".into())
}

const ACTIVITY_GUIDE: &str = "    activity 只能填下面的二级类型（如「编码」「方案讨论」），不要只填一级（「创作」「决策」）。只看这段时间人在做什么、产出了什么，不看为了什么。按这个顺序判断，落进第一个符合的：
      1 创作——产出了东西：编码（写代码、调试、测试、部署，包括让 AI 写代码、改代码）、写作（文档、方案、报告、笔记、话术）、设计（界面、原型、图形、样式）、数据分析（写查询、做表格和报表、算数据）
      2 决策——没有直接产出，但形成了判断：方案讨论（和 AI 或自己推敲方案、比较做法、定下怎么做）、评审验收（看 AI 或别人的产出和进展，检查对不对、提修改意见、确认通过）
      3 沟通——对象是具体的人、要对方回应：即时消息（微信、飞书、企业微信等聊天）、邮件、会议通话
      4 获取——只输入信息、没有形成判断：检索（带着具体问题搜索、查文档和帮助页）、阅读（读文章、帖子、论坛、书、教程，没有当下要解决的问题）
      5 管理——安排和维护：规划（日程、待办、排期）、事务（账号、行政、财务、购物、办手续）、配置维护（装软件、改设置、排查电脑和工具的问题、整理文件）
      6 消遣——放松，按形式分：影音（看剧、视频、直播、听音乐）、刷信息流（刷社交媒体、短视频的推荐流）、游戏
      都不是：其他
    人不在电脑前（离开电脑、锁屏、只有后台程序在跑）的部分不算，不要填「其他」。
    domain 是这件事为了什么，只能填：工作（为收入或职业：雇主、客户、自己的产品）、成长（为提升自己）、生活（家庭、财务、住行等个人事务）、健康（身体和心理）、关系（家人朋友、社交）、休闲（纯放松）
    project 是这段在推进的具体项目、产品、客户或事情，用最短的常用叫法（如「官网改版」「张三的合同」）；不要拿聊天会话、文档、窗口的标题当项目名，要写这件事本身；同一件事优先沿用「已有的项目名」里的写法；看不出就写空字符串。
";

/// 总结用英文写时接在提示词后面：规定只能填中文选项的字段照原样，给人看的文字都用英文
const EN_NOTE: &str = "输出语言：英文（这一条优先于上面「全部用中文」的要求）。JSON 的字段名，以及 activity、domain 这类规定只能从中文选项里选的字段，照原样填中文；project 照「已有的项目名」的写法。除此之外所有给人看的文字（标题、描述、经过、线索、一句话、点评、做成了什么、偏离、明天等）都用英文（English）写，对用户用 you。引文用英文原文或通行英译，出处也写英文。";

const ROLLUP_PROMPT: &str = "你是电脑活动记录员。下面是用户电脑上一个小时里分段写的摘要（每行是一段的起止时刻、标题和概括）。请把它们合成这一小时的摘要，全部用中文，用第二人称「你」。
只写摘要里有的事，不编造，不要写「材料未显示」这类话，不写具体分钟数。
只返回一个 JSON 对象，不要代码块，字段：
  title: 15 字以内的标题，写这一小时的主线
  description: 一两句，概括这一小时做了什么、推进到哪
";

const ALIAS_PROMPT: &str = "下面是一个人最近用过的项目名（括号里是出现次数和当时判断的领域）。
1. 有些名字说的是同一件事（同一个产品、项目、客户），只是叫法不同。把它们合并，每组选一个最常用、最简短的作规范名。拿不准的不要合并。
2. 给每个规范名定一个领域，只能填：工作、成长、生活、健康、关系、休闲。
只返回一个 JSON 对象，不要代码块：{\"aliases\": {\"别名\": \"规范名\", ...}, \"domains\": {\"规范名\": \"领域\", ...}}。aliases 只列需要改名的。
";

const TAG_PROMPT: &str = "下面是用户电脑上一段 10 分钟活动的摘要。判断这段时间花在哪。只返回一个 JSON 对象，不要代码块，字段：
  parts: 0–3 个对象的数组，每个 {\"activity\": 二级类型, \"domain\": 领域, \"project\": 项目, \"share\": 占比 0–1}，占比加起来是 1；人不在电脑前（离开电脑、只有后台程序在跑）返回空数组 []
";

/// 设备目录是不是电脑（mac-<主机名>、win-<计算机名>）；iPhone 的 iphone-* 不算，手机另算
pub fn is_computer(device: &str) -> bool {
    device.starts_with("mac-") || device.starts_with("win-")
}

fn api_key() -> Result<String, String> {
    api_key_with_source().map(|(k, _)| k)
}

/// 密钥从哪来：env／keychain／tools_md；都没有返回 None（设置页显示用）
pub fn api_key_source() -> Option<&'static str> {
    api_key_with_source().ok().map(|(_, s)| s)
}

fn api_key_with_source() -> Result<(String, &'static str), String> {
    for var in ["BEAYY_API_KEY", "LIFE_RECORD_API_KEY"] {
        if let Ok(k) = std::env::var(var) {
            if !k.is_empty() {
                return Ok((k, "env"));
            }
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(o) = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "com.fengli.beayy", "-a", "api_key", "-w"])
        .output()
    {
        let k = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if o.status.success() && !k.is_empty() {
            return Ok((k, "keychain"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    if let Ok(k) = fs::read_to_string(key_file()) {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Ok((k, "file"));
        }
    }
    Err("还没有模型密钥：到设置 → 高级 里存一把".to_string())
}

/// 没有钥匙串的系统（Windows）上密钥存在数据目录里的这个文件，未加密（和同步账号的存法一样，标注未加固）
#[cfg(not(target_os = "macos"))]
fn key_file() -> PathBuf {
    beayy_core::default_root().join("secrets").join("api_key")
}

/// 存一把模型密钥：macOS 进钥匙串（服务 com.fengli.beayy，账号 api_key），别的系统写数据目录里的文件
pub fn store_api_key(key: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ok = std::process::Command::new("/usr/bin/security")
            .args(["add-generic-password", "-U", "-s", "com.fengli.beayy", "-a", "api_key", "-w", key])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok { Ok(()) } else { Err("写入钥匙串失败".into()) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let f = key_file();
        fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&f, key).map_err(|e| e.to_string())
    }
}

/// 文件名里的 4 个随机字母
fn suffix() -> String {
    let mut x = (Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64) ^ ((std::process::id() as u64) << 32);
    (0..4)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let c = (x % 52) as u8;
            (if c < 26 { b'a' + c } else { b'A' + c - 26 }) as char
        })
        .collect()
}

fn read_dir_sorted(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

// ---------- 时间花在哪：活动类型＋项目（通用提取，不预设任何人的项目） ----------

/// 模型给的 parts 规整一下：类型不在清单里算「其他」，项目去空白，占比归一，最多 3 条
pub fn norm_parts(v: Option<&Value>) -> Vec<Value> {
    let mut out: Vec<(String, String, String, f64)> = vec![];
    for x in v.and_then(Value::as_array).cloned().unwrap_or_default() {
        let act = canon_activity(x.get("activity").and_then(Value::as_str).unwrap_or(""));
        let dom = x.get("domain").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let dom = if DOMAINS.contains(&dom.as_str()) { dom } else { "工作".to_string() };
        let proj: String = x.get("project").and_then(Value::as_str).unwrap_or("").trim().chars().take(30).collect();
        let share = x.get("share").and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        match out.iter_mut().find(|(a, d, p, _)| *a == act && *d == dom && *p == proj) {
            Some(o) => o.3 += share,
            None => out.push((act, dom, proj, share)),
        }
    }
    out.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());
    out.truncate(3);
    let total: f64 = out.iter().map(|x| x.3).sum();
    let n = out.len() as f64;
    out.into_iter()
        .map(|(a, d, p, sh)| {
            let share = if total > 0.0 { sh / total } else { 1.0 / n };
            json!({"activity": a, "domain": d, "project": p, "share": (share * 100.0).round() / 100.0})
        })
        .collect()
}

/// 摘要文件开头 --- 之间的「键: 值」
pub fn front(md: &str) -> Map<String, Value> {
    let mut m = Map::new();
    let body = md.strip_prefix("---\n").unwrap_or("");
    for line in body.lines() {
        if line == "---" {
            break;
        }
        if let Some((k, v)) = line.split_once(": ") {
            let v = v.trim();
            let val = if k == "parts" || k == "active_sec" { serde_json::from_str(v).unwrap_or(Value::String(v.into())) } else { Value::String(v.into()) };
            m.insert(k.to_string(), val);
        }
    }
    m
}

impl Summarizer {
    fn alias_path(&self) -> PathBuf {
        self.root.join("project_aliases.json")
    }

    fn registry_raw(&self) -> Value {
        fs::read_to_string(self.alias_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
    }

    fn str_map(v: &Value, key: &str) -> HashMap<String, String> {
        v.get(key).and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|x| (k.clone(), x.to_string()))).collect()).unwrap_or_default()
    }

    /// 项目登记表：每个规范名属于哪个领域（第一次出现时由模型定，之后固定；用户手动改的优先）
    pub fn domains(&self) -> HashMap<String, String> {
        let v = self.registry_raw();
        let mut m = Self::str_map(&v, "domains");
        m.extend(Self::str_map(&v, "manual_domains"));
        m
    }

    /// 项目别名表 {别名: 规范名}（模型合并同一件事的不同叫法得来；用户手动合并的优先）。连环别名（A→B→C）解到底
    pub fn aliases(&self) -> HashMap<String, String> {
        let v = self.registry_raw();
        let mut m = Self::str_map(&v, "aliases");
        m.extend(Self::str_map(&v, "manual_aliases"));
        let resolve = |k: &String| {
            let mut cur = k.clone();
            for _ in 0..10 {
                match m.get(&cur) {
                    Some(n) if n != &cur => cur = n.clone(),
                    _ => break,
                }
            }
            cur
        };
        m.keys().map(|k| (k.clone(), resolve(k))).collect()
    }

    /// 用户手动：把项目 from 合并到 into（into 可以是新名字，相当于改名）
    pub fn merge_project(&self, from: &str, into: &str) -> Result<(), String> {
        let (from, into) = (from.trim(), into.trim());
        if from.is_empty() || into.is_empty() || from == into {
            return Err("项目名不对".into());
        }
        self.edit_registry(|v| {
            let m = v.as_object_mut().unwrap().entry("manual_aliases").or_insert(json!({}));
            m[from] = json!(into);
            // into 以前若被合并到别处，取消，免得绕成圈
            if let Some(o) = m.as_object_mut() {
                o.remove(into);
            }
        })
    }

    /// 用户手动：改项目所属领域
    pub fn set_project_domain(&self, project: &str, domain: &str) -> Result<(), String> {
        if !DOMAINS.contains(&domain) {
            return Err("领域只能是：工作、成长、生活、健康、关系、休闲".into());
        }
        self.edit_registry(|v| {
            v.as_object_mut().unwrap().entry("manual_domains").or_insert(json!({}))[project] = json!(domain);
        })
    }

    fn edit_registry(&self, f: impl FnOnce(&mut Value)) -> Result<(), String> {
        let mut v = self.registry_raw();
        if !v.is_object() {
            v = json!({});
        }
        f(&mut v);
        fs::write(self.alias_path(), serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())
    }

    /// 出现了别名表没见过的项目名，就让模型把最近 30 天的项目名整体过一遍，合并同一件事的叫法
    pub fn update_aliases(&self, mut log: impl FnMut(String)) {
        let mut count: HashMap<String, usize> = HashMap::new();
        let mut seen_dom: HashMap<String, String> = HashMap::new();
        let cutoff = Utc::now() - Duration::days(30);
        for p in read_dir_sorted(&self.out_dir()) {
            let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if f.len() < 19 || NaiveDateTime::parse_from_str(&f[..19], "%Y-%m-%dT%H-%M-%S").map(|t| t.and_utc() < cutoff).unwrap_or(true) {
                continue;
            }
            for x in front(&fs::read_to_string(&p).unwrap_or_default()).get("parts").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(pj) = x["project"].as_str().filter(|s| !s.is_empty()) {
                    *count.entry(pj.to_string()).or_default() += 1;
                    if let Some(d) = x["domain"].as_str() {
                        seen_dom.entry(pj.to_string()).or_insert_with(|| d.to_string());
                    }
                }
            }
        }
        let raw: Value = fs::read_to_string(self.alias_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
        let seen: HashSet<String> = raw.get("seen").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if count.len() < 2 || count.keys().all(|k| seen.contains(k)) {
            return;
        }
        let mut names: Vec<(String, usize)> = count.into_iter().collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let list = names.iter().map(|(n, c)| format!("{n}（{c} 次，{}）", seen_dom.get(n).map(String::as_str).unwrap_or("未判断"))).collect::<Vec<_>>().join("\n");
        match self.chat("alias", ALIAS_PROMPT, &list) {
            Ok((j, _)) => {
                let valid: HashSet<&str> = names.iter().map(|x| x.0.as_str()).collect();
                let aliases: Map<String, Value> = j
                    .get("aliases")
                    .and_then(Value::as_object)
                    .map(|m| m.iter().filter(|(k, v)| valid.contains(k.as_str()) && v.as_str().map(|s| !s.is_empty() && s != k.as_str()).unwrap_or(false)).map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                // 领域：已经定过的不改（同一个项目前后一致），新的用模型给的
                let mut domains: Map<String, Value> = raw.get("domains").and_then(Value::as_object).cloned().unwrap_or_default();
                for (k, v) in j.get("domains").and_then(Value::as_object).cloned().unwrap_or_default() {
                    if v.as_str().map(|d| DOMAINS.contains(&d)).unwrap_or(false) && !domains.contains_key(&k) {
                        domains.insert(k, v);
                    }
                }
                log(format!("项目名合并：{}；领域：{}", Value::Object(aliases.clone()), Value::Object(domains.clone())));
                // 用户手动改的原样保留
                let mut out = json!({"aliases": aliases, "domains": domains, "seen": names.iter().map(|x| x.0.clone()).collect::<Vec<_>>()});
                for k in ["manual_aliases", "manual_domains"] {
                    if let Some(m) = raw.get(k) {
                        out[k] = m.clone();
                    }
                }
                let _ = fs::write(self.alias_path(), serde_json::to_string_pretty(&out).unwrap_or_default());
            }
            Err(e) => log(format!("项目名合并：{e}")),
        }
    }

    fn rollup_dir(&self) -> PathBuf {
        self.root.join("summaries-1h")
    }

    /// 最近两周用过的项目名，按出现次数排，给模型沿用（同一件事前后叫法一致）
    fn known_projects(&self) -> Vec<String> {
        let cutoff = Utc::now() - Duration::days(14);
        let mut count: HashMap<String, usize> = HashMap::new();
        for p in read_dir_sorted(&self.out_dir()) {
            let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if f.len() < 19 || NaiveDateTime::parse_from_str(&f[..19], "%Y-%m-%dT%H-%M-%S").map(|t| t.and_utc() < cutoff).unwrap_or(true) {
                continue;
            }
            let fm = front(&fs::read_to_string(&p).unwrap_or_default());
            for x in fm.get("parts").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(pj) = x.get("project").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    *count.entry(pj.to_string()).or_default() += 1;
                }
            }
        }
        // 按别名表归到规范名
        let al = self.aliases();
        let mut canon: HashMap<String, usize> = HashMap::new();
        for (k, c) in count {
            *canon.entry(al.get(&k).cloned().unwrap_or(k)).or_default() += c;
        }
        let mut v: Vec<(String, usize)> = canon.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(30).map(|x| x.0).collect()
    }

    /// 调试用：拿现在的判定说明把最近 days 天的 10 分钟摘要重新打一遍标签，只返回新旧对比，不写文件。
    /// 返回 (分段名, 在电脑前秒数, 旧 parts, 新 parts 或 {"error"})；workers 个线程并发问模型
    pub fn retag_dry(&self, days: i64, workers: usize) -> Vec<(String, i64, Value, Value)> {
        let cutoff = Utc::now() - Duration::days(days);
        let mut jobs = vec![];
        for p in read_dir_sorted(&self.out_dir()) {
            let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if !f.ends_with("-10min-memory-summary.md") || f.len() < 19 || NaiveDateTime::parse_from_str(&f[..19], "%Y-%m-%dT%H-%M-%S").map(|t| t.and_utc() < cutoff).unwrap_or(true) {
                continue;
            }
            let md = fs::read_to_string(&p).unwrap_or_default();
            let fm = front(&md);
            let active = fm.get("active_sec").and_then(Value::as_i64).unwrap_or(0);
            if active == 0 || fm.get("title").and_then(Value::as_str) == Some(IDLE_TITLE) {
                continue;
            }
            // 去掉旧标签再给模型，免得照抄
            let text: String = md.split("## Citations").next().unwrap_or(&md).lines().filter(|l| !l.starts_with("parts:") && !l.starts_with("active_sec:")).collect::<Vec<_>>().join("\n");
            jobs.push((format!("{}Z", &f[..19]), active, fm.get("parts").cloned().unwrap_or(json!([])), text));
        }
        let known = self.known_projects();
        let pre = if known.is_empty() { String::new() } else { format!("已有的项目名（同一件事请沿用这里的写法）：{}\n\n", known.join("、")) };
        let next = std::sync::atomic::AtomicUsize::new(0);
        let out = std::sync::Mutex::new(vec![]);
        std::thread::scope(|sc| {
            for _ in 0..workers.max(1) {
                sc.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some((name, active, old, text)) = jobs.get(i) else { break };
                    let new = match self.chat("tag", &format!("{TAG_PROMPT}{ACTIVITY_GUIDE}"), &format!("{pre}{text}")) {
                        Ok((j, _)) => Value::Array(norm_parts(j.get("parts"))),
                        Err(e) => json!({ "error": e }),
                    };
                    out.lock().unwrap().push((name.clone(), *active, old.clone(), new));
                });
            }
        });
        let mut v = out.into_inner().unwrap();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    /// 给还没有 parts 的旧摘要补上（只用摘要文字问一次模型，便宜），顺带补 active_sec。返回补了几段
    pub fn tag_backfill(&self, mut log: impl FnMut(String)) -> usize {
        let names = self.segment_names();
        let mut known = self.known_projects();
        let mut n = 0;
        for p in read_dir_sorted(&self.out_dir()) {
            let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(mins) = summary_minutes(&f).filter(|_| f.len() >= 19) else { continue };
            let md = fs::read_to_string(&p).unwrap_or_default();
            let fm = front(&md);
            if fm.contains_key("parts") {
                continue;
            }
            let name = format!("{}Z", &f[..19]);
            let idle = fm.get("title").and_then(Value::as_str) == Some(IDLE_TITLE);
            let t = seg_start(&name);
            let mut events: Vec<Ev> = names.iter().filter(|n| seg_start(n) >= t && seg_start(n) < t + Duration::minutes(mins)).flat_map(|n| self.load_own(n)).collect();
            events.sort_by_key(ts);
            let active = if idle || events.is_empty() { 0 } else { self.build_pack(&name, &events, &names, mins).2 };
            let parts = if idle {
                vec![]
            } else {
                let mut user = String::new();
                if !known.is_empty() {
                    user += &format!("已有的项目名（同一件事请沿用这里的写法）：{}\n\n", known.join("、"));
                }
                user += &md.split("## Citations").next().unwrap_or(&md).to_string();
                match self.chat("tag", &format!("{TAG_PROMPT}{ACTIVITY_GUIDE}"), &user) {
                    Ok((j, _)) => {
                        let raw: Vec<String> = j.get("parts").and_then(Value::as_array).map(|a| a.iter().map(|x| x["activity"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                        let n = norm_parts(j.get("parts"));
                        if n.iter().any(|x| x["activity"] == "其他") {
                            log(format!("{name}：模型给的类型 {raw:?}"));
                        }
                        n
                    }
                    Err(e) => {
                        log(format!("{name}：{e}"));
                        break;
                    }
                }
            };
            for x in &parts {
                if let Some(pj) = x["project"].as_str().filter(|s| !s.is_empty()) {
                    if !known.iter().any(|k| k == pj) {
                        known.push(pj.to_string());
                    }
                }
            }
            let new = md.replacen("\n---\n", &format!("\nactive_sec: {active}\nparts: {}\n---\n", Value::Array(parts.clone())), 1);
            if fs::write(&p, new).is_ok() {
                n += 1;
                log(format!("{name}：补标签 {}", Value::Array(parts)));
            }
        }
        n
    }

    /// 小时摘要：一个小时结束、里面有活动的分段都写好了摘要，就把它们合成一段
    /// （整小时都不在电脑前、或只有一份有活动的，不调模型）。写到 summaries-1h/<UTC 小时>-1h.md。返回写了几段
    pub fn rollups(&self, mut log: impl FnMut(String)) -> usize {
        let names = self.segment_names();
        let now = Utc::now();
        let mut hours: Vec<String> = names.iter().map(|n| n[..13].to_string()).collect();
        hours.dedup();
        let _ = fs::create_dir_all(self.rollup_dir());
        let mut n = 0;
        for h in hours {
            let out = self.rollup_dir().join(format!("{h}-00-00-1h.md"));
            let Ok(start) = NaiveDateTime::parse_from_str(&format!("{h}-00-00"), "%Y-%m-%dT%H-%M-%S").map(|t| t.and_utc()) else { continue };
            if out.exists() || start + Duration::minutes(61) > now {
                continue;
            }
            let segs: Vec<&String> = names.iter().filter(|x| x.starts_with(&h)).collect();
            // （这份摘要的起始分段, 管几分钟, 头部字段）；间隔调长后几个分段共用一份，只算一次
            let mut rows: Vec<(String, i64, Map<String, Value>)> = vec![];
            let mut seen = HashSet::new();
            let mut ready = true;
            for seg in &segs {
                match self.existing(seg).first() {
                    Some(p) => {
                        let f = p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                        if seen.insert(f.clone()) {
                            rows.push((format!("{}Z", &f[..19]), summary_minutes(&f).unwrap_or(10), front(&fs::read_to_string(p).unwrap_or_default())));
                        }
                    }
                    None => {
                        // 有活动却还没摘要：等下一轮（没活动的分段本来就不写摘要）
                        if self.load(seg).iter().any(|e| !NOISE.contains(&ty(e))) {
                            ready = false;
                        }
                    }
                }
            }
            if !ready || rows.is_empty() {
                continue;
            }
            // 按秒累计项目和类型
            let mut agg: Vec<(String, String, f64)> = vec![];
            let mut active = 0i64;
            for (_, _, fm) in &rows {
                let sec = fm.get("active_sec").and_then(Value::as_i64).unwrap_or(0);
                active += sec;
                for x in fm.get("parts").and_then(Value::as_array).cloned().unwrap_or_default() {
                    let (a, p) = (x["activity"].as_str().unwrap_or("其他").to_string(), x["project"].as_str().unwrap_or("").to_string());
                    let v = sec as f64 * x["share"].as_f64().unwrap_or(0.0);
                    match agg.iter_mut().find(|y| y.0 == a && y.1 == p) {
                        Some(y) => y.2 += v,
                        None => agg.push((a, p, v)),
                    }
                }
            }
            agg.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            let parts = Value::Array(agg.iter().map(|(a, p, v)| json!({"activity": a, "project": p, "sec": v.round() as i64})).collect());
            let busy: Vec<&Map<String, Value>> = rows.iter().map(|r| &r.2).filter(|fm| fm.get("title").and_then(Value::as_str) != Some(IDLE_TITLE)).collect();
            let g = |fm: &Map<String, Value>, k: &str| fm.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let (title, desc) = if busy.is_empty() {
                (IDLE_TITLE.to_string(), "这一小时没有键鼠操作（离开、锁屏或睡眠）。".to_string())
            } else if busy.len() == 1 {
                // 只有一份有活动的（间隔 1 小时，或这小时只用了一会儿）：直接用它，不再问模型
                (g(busy[0], "title"), g(busy[0], "description"))
            } else {
                let mut user = String::new();
                for (seg, mins, fm) in &rows {
                    let t = seg_start(seg);
                    user += &format!("{}–{} 〔{}〕{}\n", hms(t)[..5].to_string(), hms(t + Duration::minutes(*mins))[..5].to_string(), g(fm, "title"), g(fm, "description"));
                }
                match self.chat("hourly", ROLLUP_PROMPT, &user) {
                    Ok((j, _)) => (
                        j.get("title").and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "),
                        j.get("description").and_then(Value::as_str).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" "),
                    ),
                    Err(e) => {
                        log(format!("{h} 小时摘要：{e}"));
                        break;
                    }
                }
            };
            let md = format!(
                "---\ntitle: {title}\ndescription: {desc}\nactive_sec: {active}\nparts: {parts}\nsegments: [{}]\n---\n",
                rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>().join(", ")
            );
            if fs::write(&out, md).is_ok() {
                n += 1;
                log(format!("{h} 小时摘要：{title}"));
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_like_python() {
        assert_eq!(split_lines("a\nb\r\nc\n"), vec!["a", "b", "c"]);
        assert_eq!(split_lines(""), Vec::<&str>::new());
        assert_eq!(split_lines("a\n\nb"), vec!["a", "", "b"]);
    }

    #[test]
    fn junk_and_round() {
        assert!(JUNK.is_match("3.4k 个 token"));
        assert!(JUNK.is_match("Claude 运行中"));
        assert!(!JUNK.is_match("正常一行字"));
        assert_eq!(py_round(2.5), 2);
        assert_eq!(py_round(3.5), 4);
    }

    /// 2026-10-29 改名前的二级、一级，读历史记录时归到现在的类型
    #[test]
    fn old_activity_names() {
        for (old, new) in [("调研", "检索"), ("学习", "阅读"), ("整理", "配置维护"), ("社交媒体", "刷信息流"), ("审阅", "评审验收"), ("开发", "编码"), ("方案讨论", "方案讨论"), ("决策", "决策")] {
            assert_eq!(canon_activity(old), new, "{old}");
        }
        assert_eq!(canon_activity("休闲"), "消遣");
        assert_eq!(activity_group("消遣"), "消遣");
        assert_eq!(activity_group(&canon_activity("社交媒体")), "消遣");
        assert_eq!(activity_group("评审验收"), "决策");
        assert!(norm_parts(Some(&json!([]))).is_empty());
    }

    /// Windows 版写的事件（win-* 设备，bundle_id 是程序路径）也算进电脑时长
    #[test]
    fn windows_device_counts() {
        let root = std::env::temp_dir().join(format!("beayy-wintest-{}", std::process::id()));
        let seg = root.join("devices/win-TEST/segments/2026-10-16T06-00-00Z");
        fs::create_dir_all(&seg).unwrap();
        let ev = |t: &str, ty: &str, extra: &str| format!("{{\"device\":\"win-TEST\",\"event_type\":\"{ty}\",\"timestamp\":\"2026-10-16T06:0{t}Z\"{extra}}}\n");
        let app = r#","pid":1,"app_name":"msedge","bundle_id":"C:\\Program Files\\msedge.exe","window_title":"新标签页""#;
        let body = ev("0:00", "app.activated", app) + &ev("0:05", "presence", r#","state":"active","idle_seconds":0"#)
            + &ev("1:05", "heartbeat", &format!(r#","state":"active","idle_seconds":2{app}"#)) + &ev("2:05", "heartbeat", &format!(r#","state":"active","idle_seconds":1{app}"#));
        fs::write(seg.join("events.jsonl"), body).unwrap();
        let s = Summarizer::new(root.clone());
        assert_eq!(s.segment_names(), vec!["2026-10-16T06-00-00Z".to_string()]);
        let t0 = DateTime::parse_from_rfc3339("2026-10-16T06:00:00Z").unwrap().with_timezone(&Utc);
        let act = s.activity(t0, t0 + Duration::minutes(10));
        let _ = fs::remove_dir_all(&root);
        assert!(!act.spans.is_empty(), "没算出在电脑前的时间");
        assert_eq!(act.windows.first().map(|w| w.2.as_str()), Some("msedge"));
    }

    /// 摘要间隔 30 分钟：同一个间隔的分段并成一份，文件名写管多长；间隔中途改的，从第一段没写的写到间隔结束
    /// （分段都不在电脑前，走固定摘要，不调模型）
    #[test]
    fn summary_interval() {
        assert_eq!(summary_minutes("2026-10-16T06-00-00-ab12-30min-memory-summary.md"), Some(30));
        assert_eq!(summary_minutes("2026-10-16T06-00-00-ab12-6h-memory-summary.md"), None);
        let root = std::env::temp_dir().join(format!("beayy-every-{}", std::process::id()));
        let dev = root.join("devices").join(beayy_core::local_device()).join("segments");
        for m in ["06-00", "06-10", "06-20", "06-30", "07-00", "07-10", "07-20"] {
            let seg = dev.join(format!("2026-10-16T{m}-00Z"));
            fs::create_dir_all(&seg).unwrap();
            let t = format!("2026-10-16T{}:{}:30Z", &m[..2], &m[3..]);
            fs::write(seg.join("events.jsonl"), format!("{{\"event_type\":\"presence\",\"state\":\"idle\",\"timestamp\":\"{t}\"}}\n")).unwrap();
        }
        fs::write(root.join("settings.json"), r#"{"summary_minutes": 30}"#).unwrap();
        let s = Summarizer::new(root.clone());
        fs::create_dir_all(s.out_dir()).unwrap();
        fs::write(s.out_dir().join("2026-10-16T07-00-00-test-10min-memory-summary.md"), "---\ntitle: 旧的\n---\n").unwrap();
        assert_eq!(s.every(), 30);
        assert_eq!(s.run(&[], |_| {}), 3);
        let files: Vec<String> = read_dir_sorted(&s.out_dir()).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        let starts: Vec<(String, Option<i64>)> = files.iter().map(|f| (f[11..16].to_string(), summary_minutes(f))).collect();
        assert_eq!(starts, vec![("06-00".into(), Some(30)), ("06-30".into(), Some(30)), ("07-00".into(), Some(10)), ("07-10".into(), Some(20))]);
        assert!(s.existing("2026-10-16T06-20-00Z")[0].to_string_lossy().contains("T06-00-00-"));
        assert!(s.existing("2026-10-16T07-20-00Z")[0].to_string_lossy().contains("T07-10-00-"));
        assert!(fs::read_to_string(s.out_dir().join(&files[0])).unwrap().contains("这 30 分钟没有键鼠操作"));
        assert_eq!(s.run(&[], |_| {}), 0, "写过的不再写");
        s.rollups(|_| {});
        let hour = fs::read_to_string(s.rollup_dir().join("2026-10-16T06-00-00-1h.md")).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(hour.contains("segments: [2026-10-16T06-00-00Z, 2026-10-16T06-30-00Z]"), "{hour}");
    }
}
