//! 日总结、周总结、月总结：app 自己用记录写，不依赖外部脚本。
//! 数字（在电脑前、手机、领域和项目、类型、规矩守没守住、去了哪、说了多少话）由代码算好填进去；模型只写一句话、做成了什么、偏离、明天。
//! 写到 Beayy/daily/日总结_YYYYMMDD.md、周总结_YYYYMMDD.md（日期取周一）、月总结_YYYYMM.md。每天 21:30 写当天，次日 0:15 以后用完整的一天重写；
//! 每 10 分钟看一遍最近 7 天：缺的补上（app 没开、模型出错），数据晚到的（手机录音、屏幕使用时间隔几天才同步过来）重写；
//! 上一周过完写周总结，那周的日总结重写过就跟着重写；上个月过完写月总结（用那个月的周总结和每天的一句话），同样跟着重写。每次的结果记在 daily/state.json，失败按次数往后推着重试。页面上也可以随时生成。

use beayy_core::default_root;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use regex::Regex;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// 日总结的写法变了就加一：已有的最近 7 天按新写法重写一遍（2：材料里加了去了哪、对话）
const VERSION: i64 = 2;

pub fn path_of(date: NaiveDate) -> PathBuf {
    default_root().join("daily").join(format!("日总结_{}.md", date.format("%Y%m%d")))
}

pub fn week_path(monday: NaiveDate) -> PathBuf {
    default_root().join("daily").join(format!("周总结_{}.md", monday.format("%Y%m%d")))
}

pub fn month_path(first: NaiveDate) -> PathBuf {
    default_root().join("daily").join(format!("月总结_{}.md", first.format("%Y%m")))
}

pub fn first_of(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// 这个月的最后一天
pub fn last_of(first: NaiveDate) -> NaiveDate {
    let next = if first.month() == 12 { NaiveDate::from_ymd_opt(first.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1) };
    next.unwrap_or(first) - Duration::days(1)
}

/// 读一份总结：本机写的为准，没有就用别的设备同步来的（手机上全靠这个）
fn read_md(p: &PathBuf) -> Option<String> {
    crate::data::daily_md(&p.file_name()?.to_string_lossy())
}

pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.format("%u").to_string().parse::<i64>().unwrap_or(1) - 1)
}

// ---------- 每次写的结果：daily/state.json {"days": {日期: …}, "weeks": {周一: …}} ----------

fn state_path() -> PathBuf {
    default_root().join("daily").join("state.json")
}

fn read_state() -> Value {
    fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}))
}

fn note(kind: &str, key: &str, entry: Value) {
    let mut st = read_state();
    st[kind][key] = entry;
    let _ = fs::create_dir_all(state_path().parent().unwrap());
    let _ = fs::write(state_path(), serde_json::to_vec_pretty(&st).unwrap_or_default());
}

fn log_line(line: &str) {
    let logs = default_root().join("logs");
    let _ = fs::create_dir_all(&logs);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(logs.join("daily.log")) {
        let _ = writeln!(f, "{} {line}", Local::now().format("%Y-%m-%d %H:%M"));
    }
}

/// 记一次成功或失败。失败隔 30 分钟、1、2、4 小时……再试，最多 6 小时；这一天没记录的不算失败，等数据变了再试
fn record(kind: &str, key: &str, bytes: u64, r: &Result<String, String>) {
    let now = Utc::now().timestamp();
    let entry = match r {
        Ok(md) => {
            log_line(&format!("{key}：写好了（{} 字）", md.chars().count()));
            serde_json::json!({"v": VERSION, "bytes": bytes, "at": now})
        }
        Err(e) => {
            log_line(&format!("{key}：{e}"));
            let fails = read_state()[kind][key]["fails"].as_i64().unwrap_or(0) + 1;
            let empty = e.starts_with("这一天没有记录") || e.starts_with("这周还没有日总结") || e.starts_with("这个月还没有日总结");
            let wait = if empty { 0 } else { (1800 * (1i64 << (fails - 1).min(4))).min(6 * 3600) };
            serde_json::json!({"error": e, "fails": fails, "retry_at": now + wait, "empty": empty, "bytes": bytes, "failed_at": now})
        }
    };
    note(kind, key, entry);
}

/// 这一天各设备记下的事件有多少字节。数据晚到会让它变大，用来判断要不要重写
fn input_bytes(date: NaiveDate) -> u64 {
    let s = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
    let key = |t: DateTime<Utc>| t.format("%Y-%m-%dT%H-%M-00Z").to_string();
    let (lo, hi) = (key(s), key(s + Duration::days(1)));
    let mut n = 0;
    for dev in fs::read_dir(default_root().join("devices")).into_iter().flatten().flatten() {
        for seg in fs::read_dir(dev.path().join("segments")).into_iter().flatten().flatten() {
            let name = seg.file_name().to_string_lossy().to_string();
            if name >= lo && name < hi {
                n += fs::metadata(seg.path().join("events.jsonl")).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    n
}

// ---------- 规矩：和页面上同一套识别规则 ----------

enum Rule {
    /// 几点以后不用：设备 computer／phone／any
    After { dev: &'static str, h: u32, m: u32 },
    /// 某设备每天不超过多少分钟
    Limit { dev: &'static str, minutes: f64 },
    /// 某 app 每天不超过多少分钟
    App { app: String, minutes: f64 },
}

fn parse_rule(text: &str) -> Option<Rule> {
    let t: String = text.split_whitespace().collect();
    let low = t.to_lowercase();
    let dev = if t.contains("电脑") || low.contains("mac") { "computer" } else if t.contains("手机") || low.contains("iphone") { "phone" } else { "any" };
    let after = Regex::new(r"(\d{1,2})(?:[:：点](\d{2})?)?(?:分)?(?:以后|之后|后)").unwrap();
    if let Some(c) = after.captures(&t) {
        if Regex::new(r"不|别|停|关|禁").unwrap().is_match(&t) {
            let mut h: u32 = c[1].parse().ok()?;
            if h < 12 && Regex::new(r"晚上|夜里|夜间|下午|傍晚").unwrap().is_match(&t) {
                h += 12;
            }
            return Some(Rule::After { dev, h, m: c.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(0) });
        }
    }
    let lim = Regex::new(r"(?i)(?:不超过|少于|最多|以内|不到|≤|<=?)(\d+(?:\.\d+)?)(小时|h|分钟|min|分)").unwrap();
    if let Some(c) = lim.captures(&t) {
        let n: f64 = c[1].parse().ok()?;
        let minutes = if c[2].contains('小') || c[2].eq_ignore_ascii_case("h") { n * 60.0 } else { n };
        let subject = Regex::new(r"每天|一天|每日|不超过|少于|最多|不到").unwrap().split(&t).next().unwrap_or("").to_string();
        let subject = subject.trim_start_matches(['用', '玩', '刷', '看']).to_string();
        if dev != "any" && Regex::new(r"(?i)^(电脑|手机|mac|iphone)?$").unwrap().is_match(&subject) {
            return Some(Rule::Limit { dev, minutes });
        }
        if !subject.is_empty() {
            return Some(Rule::App { app: subject, minutes });
        }
    }
    None
}

fn secs_after(spans: &[(DateTime<Utc>, DateTime<Utc>)], from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    spans.iter().map(|(a, b)| ((*b).min(to) - (*a).max(from)).num_milliseconds().max(0) as f64 / 1000.0).sum()
}

/// 一条规矩今天守没守住：（对照说明，是否守住；None＝还没到点或算不出）
fn check(rule: &str, date: NaiveDate, day: &Value) -> (String, Option<bool>) {
    let Some(r) = parse_rule(rule) else { return ("算不出来，看下面的偏离".into(), None) };
    let live = &day["live"];
    let phone = &day["phone"];
    let iso = |v: &Value| v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Utc));
    let start = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
    let end = start + Duration::days(1);
    match r {
        Rule::After { dev, h, m } => {
            let from = start + Duration::minutes((h * 60 + m) as i64);
            if Utc::now() < from {
                return (format!("还没到 {h:02}:{m:02}"), None);
            }
            let mut sec = 0.0;
            if dev != "phone" {
                let spans: Vec<_> = live["spans"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| Some((iso(&x[0])?, iso(&x[1])?))).collect();
                sec += secs_after(&spans, from, end);
            }
            if dev != "computer" {
                for x in phone["sessions"].as_array().cloned().unwrap_or_default() {
                    if let (Some(a), Some(b)) = (iso(&x["start"]), iso(&x["end"])) {
                        let span = (b - a).num_seconds().max(1) as f64;
                        sec += secs_after(&[(a, b)], from, end) / span * x["sec"].as_f64().unwrap_or(0.0);
                    }
                }
            }
            (format!("{h:02}:{m:02} 以后用了 {} 分钟", (sec / 60.0).round()), Some(sec < 60.0))
        }
        Rule::Limit { dev, minutes } => {
            let sec = if dev == "phone" { phone["total_sec"].as_f64().unwrap_or(0.0) } else { live["active_sec"].as_f64().unwrap_or(0.0) };
            (format!("{} {:.1} 小时 / 上限 {:.1} 小时", if dev == "phone" { "手机" } else { "电脑" }, sec / 3600.0, minutes / 60.0), Some(sec <= minutes * 60.0))
        }
        Rule::App { app, minutes } => {
            let key = app.to_lowercase();
            let hit = |x: &Value| x["name"].as_str().unwrap_or("").to_lowercase().contains(&key) || x["bundle_id"].as_str().unwrap_or("").to_lowercase().contains(&key);
            let mut sec = 0.0;
            for s in phone["sessions"].as_array().cloned().unwrap_or_default() {
                for a in s["apps"].as_array().cloned().unwrap_or_default() {
                    if hit(&a) {
                        sec += a["sec"].as_f64().unwrap_or(0.0);
                    }
                }
            }
            for a in live["apps"].as_array().cloned().unwrap_or_default() {
                if hit(&a) {
                    sec += a["sec"].as_f64().unwrap_or(0.0);
                }
            }
            (format!("{app} {} 分钟 / 上限 {} 分钟", (sec / 60.0).round(), minutes), Some(sec <= minutes * 60.0))
        }
    }
}

const PROMPT: &str = "你是用户的日记助手。下面是用户今天的记录：电脑和手机的时长、时间花在哪些领域和项目、每小时在做什么、自己定的规矩今天的对照结果，有的话还有去了哪（手机停留点）、对话（录音转写）和苹果健康（睡眠、步数、锻炼、心情）。
据此写今天的日总结，全部用中文，用第二人称「你」。line、done、drift、tomorrow 写得具体、平实，不夸、不说教；remark 换一种口吻，见下。只写材料里有的事，不编造，不写材料里没有的数字。
去了哪、对话也算今天做的事：见了谁、谈了什么、在外面待了多久，有内容就写进去；转写有错字，意思不明的不硬猜、不逐字引用。
只返回一个 JSON 对象，不要代码块，字段：
  line: 今天的一句话，30 字以内，抓住今天最主要的事和状态
  quote: 点评开头引的一句话，来源不限：古今中外的诗词、古文、小说、散文、戏剧、电影台词、名人语录都行，挑最贴今天的，别总在几首名诗里打转；只引确实存在、出处确定的，外文用通行的中文译文；不带引号，不带出处
  source: 这句话的出处，写成「作者《作品》」「《书名·篇名》」，语录没有作品的只写人名（外国人用通行中文译名），不带括号
  remark: 点评，60–100 字，像一位博览群书、说话风趣的老友在你日记旁写的批注：借 quote 这句话点评今天，时间花在哪、去了哪些地方、做成了什么、哪里偏离了。用比喻、对仗、反差制造趣味，要有文学色彩，不要写成流水账；只写材料里有的事，不编造、不说教、不刻薄、不吹捧；不要重复 quote，不要在首尾加引号
  done: 2–5 条字符串，今天做成或推进了什么（写结果，不写过程）
  drift: 0–3 条字符串，偏离：没守住的规矩、熬夜、时间明显被不重要的事占掉；没有就空数组
  tomorrow: 1–3 条字符串，明天最值得做的事，从今天没做完的里挑
材料最后如果列了「前后几天的点评已经引过」，quote 换一句：别出自同一篇、同一个人，意思也别相近；那几句如果多是中国古诗文，这次换个门类（现当代文学、外国文学、电影、名人语录等）。
";

// 棒喝（设置里打开才写）：单独问一次模型，换成贴吧老哥的人设，免得和日记助手「不刻薄」的口吻打架。
// 去 AI 味的几条取自 fl-deslop；哲理要从当天的事里说破，不引名言（名言归点评）
const ROAST_HOW: &str = "怎么骂：
- 贴吧老哥的口吻：叫他「老哥」，阴阳怪气、反问、夸张的比喻，先嘲讽再补刀。贴吧梗每段 1–2 个（绷不住了、好家伙、乐、急了、就这、赢麻了、属实、蚌埠住了、6、典这类），用对场合，别堆，别每次都用同一个。
- 口语，短句，一句一刀。带脏字（他妈、操、卧槽、扯淡、狗屁、傻逼这类），全段至少 3 处，骂到点上用，别句句带。
- 每一刀都落在材料里的具体事上：几点、几分钟、哪个 app、哪个项目、哪条规矩。只写材料里有的，不编；材料说了不准、不全的数不拿来骂。
- 最狠的一刀拿他自己打他：他定的规矩、说要做没做的事、他在做的东西（比如他做的产品本身就是管时间、管专注、管待办的，他自己却管不住）、他擅长的本事，跟实际干的摆在一起，让反差自己说话。
- 哲理藏在骂里：挑最该骂的一件，说破他为什么会这样，在躲什么、拿什么换了什么、哪件事是因为有人催才先做的。直接说他在干什么（「你在拿……换……」「你怕的是……」），大白话一句说透，具体到这些事，这一句不用梗；不引名言，不讲人人都懂的道理，不熬鸡汤。
- 骂行为，不骂人格；不碰外貌、身体、健康、家人、伴侣和其他人。干得好的地方照实认一句就过，别夸。
- 不先否定一个说法再给结论。❌「你不是没本事，是怕交卷」 ✅「你怕交卷」；❌「这不是并行推进，是每件事都摸一下」 ✅「你每件事都摸一下，好让自己觉得全在动」。「不是……是……」「不是……而是……」「与其说……不如说」一律不写。
- 不说「这点我认」「我承认」这种裁判腔；不三连排比；不用破折号；不加「说白了」「一句话」「记住」这类报幕。
- 最后一句是一刀，最好就是说破的那句，骂完就停：不给建议，不写「明天先……」「别再……」，不升华、不鼓励。
- 写完自己查一遍上面几条，犯了就改掉再交。
";

const ROAST: &str = "你是在贴吧混了很多年的老哥，跟用户认识很久，嘴毒，说话不留情面。用户自己要你每天给他来一下棒喝，一棒打醒。下面是他今天的记录：电脑和手机的时长、时间花在哪些领域和项目、每小时在做什么、他自己定的规矩今天守没守住，有的话还有去了哪、对话（录音转写）和健康。
读完骂他，骂得狠、骂得准，骂完让他心里一沉、想改。材料最后如果列了「前几天骂过」，换个角度骂，那几段用过的梗这次别再用；同一个毛病连着几天还在犯，点明第几天了，骂得更重。
只返回一个 JSON 对象，不要代码块：{\"roast\": \"……\"}。roast 是 150–250 字的一段话，中文，对他用「你」。
";

const WEEK_ROAST: &str = "你是在贴吧混了很多年的老哥，跟用户认识很久，嘴毒，说话不留情面。用户自己要你每周给他来一下棒喝，一棒打醒。下面是他这一周每天的日总结（有的天可能没有，里面的「棒喝」是你每天骂过的）。
骂这一周的模式：哪个毛病天天犯、哪件事说了一周没动、哪天最离谱、时间实际给了谁。每天骂过的别照抄，往上提一层，骂那个让这些天反复出事的根子。
只返回一个 JSON 对象，不要代码块：{\"roast\": \"……\"}。roast 是 250–400 字，一两段话，中文，对他用「你」；脏字全段至少 4 处。
";

/// 写棒喝（日总结、周总结共用）；出错只是少这一段，总结照写。脏字不够、写了「不是……是……」的，指出来让它重写一次
fn roast_of(sm: &beayy_summary::Summarizer, kind: &str, system: &str, user: &str, min_curse: usize) -> Option<String> {
    let system = format!("{system}{ROAST_HOW}");
    let ask = |u: &str| -> Option<String> {
        let j = sm.ask_as(kind, &system, u).map_err(|e| eprintln!("棒喝没写成：{e}")).ok()?;
        j.get("roast").and_then(Value::as_str).map(|r| r.trim().to_string()).filter(|r| !r.is_empty())
    };
    let first = ask(user)?;
    let faults = roast_faults(&first, min_curse);
    if faults.is_empty() {
        return Some(first);
    }
    Some(ask(&format!("{user}\n\n你上一版：{first}\n上一版的毛病：{}。照「怎么骂」重写。", faults.join("；"))).unwrap_or(first))
}

/// 棒喝的硬毛病：脏字少于 min 处、有先否定再给结论的句子
fn roast_faults(r: &str, min: usize) -> Vec<String> {
    let mut out = vec![];
    let n: usize = ["他妈", "操", "卧槽", "扯淡", "狗屁", "傻逼", "妈的", "屁"].iter().map(|w| r.matches(w).count()).sum::<usize>()
        - r.matches("他妈").count().min(r.matches("妈的").count()) // 「他妈的」别算两处
        - r.matches("狗屁").count(); // 「狗屁」里的「屁」别再算一处
    if n < min {
        out.push(format!("脏字只有 {n} 处，至少要 {min} 处"));
    }
    if let Some(m) = Regex::new(r"不是[^。！？；]{0,24}?[，,；;]\s*(而|就|你)?是").unwrap().find(r) {
        out.push(format!("「{}」是先否定再给结论，直接说结论", m.as_str()));
    }
    out
}

/// 棒喝开着没有（settings.json 的 roast）
fn roast_on() -> bool {
    crate::data::read_settings_raw().get("roast").and_then(Value::as_bool).unwrap_or(false)
}

/// 总结里某一段（## 标题 下面到下一个 ## 之前）；没有或是空的就 None
fn section_of(md: &str, title: &str) -> Option<String> {
    let head = format!("## {title}");
    let body: Vec<&str> = md.lines().skip_while(|l| l.trim_end() != head).skip(1).take_while(|l| !l.starts_with("## ")).collect();
    let t = body.join("\n").trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// 苹果健康：昨晚睡眠一行、活动和锻炼一行、心情一行（有才写）
fn health_lines(h: &Value) -> Vec<String> {
    let mut out = vec![];
    let hm = |v: &Value| v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default();
    let s = &h["sleep"];
    if let Some(sec) = s["asleep_sec"].as_f64() {
        let stages = if s["staged"] == true {
            format!("，深睡 {}、快速眼动 {}、醒着 {}", dur(s["deep_sec"].as_f64().unwrap_or(0.0)), dur(s["rem_sec"].as_f64().unwrap_or(0.0)), dur(s["awake_sec"].as_f64().unwrap_or(0.0)))
        } else {
            String::new()
        };
        out.push(format!("昨晚睡了 {}（{}–{}{stages}）", dur(sec), hm(&s["start"]), hm(&s["end"])));
    }
    let a = &h["activity"];
    let mut act = vec![];
    if let Some(n) = a["steps"].as_i64() {
        act.push(format!("走了 {n} 步"));
    }
    if let Some(n) = a["active_kcal"].as_i64() {
        act.push(format!("活动 {n} 千卡"));
    }
    if let Some(n) = a["exercise_min"].as_i64() {
        act.push(format!("锻炼 {n} 分钟"));
    }
    for w in h["workouts"].as_array().cloned().unwrap_or_default() {
        act.push(format!("{} {} {}", hm(&w["start"]), w["name"].as_str().unwrap_or(""), dur(w["sec"].as_f64().unwrap_or(0.0))));
    }
    if !act.is_empty() {
        out.push(act.join("，"));
    }
    let moods: Vec<String> = h["moods"].as_array().cloned().unwrap_or_default().iter()
        .filter_map(|m| m["valence"].as_f64().map(|v| format!("{} 心情 {v:.1}（-1 很差到 1 很好）", hm(&m["at"]))))
        .collect();
    if !moods.is_empty() {
        out.push(moods.join("，"));
    }
    out
}

fn dur(sec: f64) -> String {
    if sec < 3600.0 {
        format!("{} 分钟", (sec / 60.0).round())
    } else {
        format!("{:.1} 小时", sec / 3600.0)
    }
}

fn hm(v: &Value) -> String {
    v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default()
}

fn secs_between(a: &Value, b: &Value) -> f64 {
    let t = |v: &Value| v.as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    match (t(a), t(b)) {
        (Some(a), Some(b)) => (b - a).num_seconds().max(0) as f64,
        _ => 0.0,
    }
}

/// 去了哪：（读数一行，给模型的逐段）。前一天到的（在家过夜）从 0 点算起
fn places_text(places: &Value, date: NaiveDate) -> Option<(String, Vec<String>)> {
    let visits = places["visits"].as_array().filter(|v| !v.is_empty())?;
    let name = |v: &Value| v["label"].as_str().or(v["name"].as_str()).filter(|x| !x.is_empty()).unwrap_or("没取到名字的地方").to_string();
    let day = Local.from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap()).unwrap();
    let clip = |v: &Value, k: &str| -> Value {
        let t = v[k].as_str().and_then(|x| DateTime::parse_from_rfc3339(x).ok()).map(|t| t.with_timezone(&Local));
        t.map(|t| serde_json::json!(t.clamp(day, day + Duration::days(1)).to_rfc3339())).unwrap_or(Value::Null)
    };
    let sec = |v: &Value| secs_between(&clip(v, "start"), &clip(v, "end"));
    let mut by: Vec<(String, f64)> = vec![];
    for v in visits {
        match by.iter_mut().find(|x| x.0 == name(v)) {
            Some(x) => x.1 += sec(v),
            None => by.push((name(v), sec(v))),
        }
    }
    by.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top: Vec<String> = by.iter().take(5).map(|(n, s)| format!("{n} {}", dur(*s))).collect();
    let line = format!("去了 {} 个地方：{}", by.len(), top.join("、"));
    let rows = visits.iter().take(30).map(|v| format!("{}–{} {}（{}）", hm(&clip(v, "start")), hm(&clip(v, "end")), name(v), dur(sec(v)))).collect();
    Some((line, rows))
}

/// 对话：（读数一行，给模型的逐小时摘录）。一段对话可能连着好几个小时、说话人也分不准，所以按小时摘：
/// 说得多的小时先挑，最多 15 个小时、6000 字，每小时截前面一部分，再按时间排
fn speech_text(speech: &Value) -> Option<(String, Vec<String>)> {
    let convs = speech["conversations"].as_array().filter(|v| !v.is_empty())?;
    let talk = speech["talk_sec"].as_f64().unwrap_or(0.0);
    let mine: f64 = speech["speakers_sec"].as_object().map(|m| m.iter().filter(|(k, _)| !k.starts_with("他人") && *k != "未辨").filter_map(|(_, v)| v.as_f64()).sum()).unwrap_or(0.0);
    let line = format!("录到说话 {}{}", dur(talk), if mine > 0.0 { format!("，其中你 {}", dur(mine)) } else { String::new() });
    let mut hours: std::collections::BTreeMap<String, (f64, String)> = Default::default();
    for l in convs.iter().flat_map(|c| c["lines"].as_array().cloned().unwrap_or_default()) {
        let h = hm(&l["start"]).get(..2).unwrap_or("").to_string();
        let e = hours.entry(h).or_default();
        e.0 += secs_between(&l["start"], &l["end"]);
        if e.1.chars().count() < 400 {
            let who = l["speaker"].as_str().unwrap_or("");
            e.1 += &format!("{}{} ", if who.starts_with("他人") || who == "未辨" { "" } else { "【你】" }, l["text"].as_str().unwrap_or(""));
        }
    }
    let mut pick: Vec<(String, (f64, String))> = hours.into_iter().filter(|(_, (sec, _))| *sec >= 60.0).collect();
    pick.sort_by(|a, b| b.1 .0.partial_cmp(&a.1 .0).unwrap());
    pick.truncate(15);
    pick.sort_by(|a, b| a.0.cmp(&b.0));
    let rows = pick.into_iter().map(|(h, (sec, text))| format!("{h}:00 这一小时说话 {}：{}", dur(sec), text.chars().take(400).collect::<String>().trim())).collect();
    Some((line, rows))
}

/// 写某天的日总结（结果记进 state.json）；返回 Markdown
pub fn generate(date: NaiveDate) -> Result<String, String> {
    let bytes = input_bytes(date);
    let r = write_day(date);
    record("days", &date.format("%Y-%m-%d").to_string(), bytes, &r);
    r
}

fn write_day(date: NaiveDate) -> Result<String, String> {
    let mut day = crate::data::get_day(date.format("%Y-%m-%d").to_string());
    // 有外部数据包的日子 get_day 不算电脑读数（页面用数据包的），这里要用：在电脑前、规矩对照都靠它
    if day["live"].is_null() {
        day["live"] = crate::data::live_computer(date).unwrap_or(Value::Null);
    }
    let live = &day["live"];
    let phone = &day["phone"];
    let work = &day["work"];
    let places = places_text(&day["places"], date);
    let speech = speech_text(&day["speech"]);
    if live.is_null() && phone.is_null() && places.is_none() && speech.is_none() {
        return Err("这一天没有记录".into());
    }
    // 代码算的数字
    let mut stats = vec![];
    if let Some(sec) = live["active_sec"].as_f64() {
        stats.push(format!("在电脑前 {}（{}–{}），切换 app {} 次", dur(sec), live["first"].as_str().unwrap_or(""), live["last"].as_str().unwrap_or(""), live["switches"]));
    }
    if let Some(sec) = phone["total_sec"].as_f64() {
        let top: Vec<String> = phone["apps"].as_array().cloned().unwrap_or_default().iter().take(3).map(|a| format!("{} {}", a["name"].as_str().unwrap_or(""), dur(a["sec"].as_f64().unwrap_or(0.0)))).collect();
        stats.push(format!("手机 {}，拿起 {} 次{}", dur(sec), phone["pickups"], if top.is_empty() { String::new() } else { format!("，最多的是 {}", top.join("、")) }));
    }
    if let Some((line, _)) = &places {
        stats.push(line.clone());
    }
    if let Some((line, _)) = &speech {
        stats.push(line.clone());
    }
    stats.extend(health_lines(&day["health"]));
    let tree = |k: &str| -> Vec<String> {
        work[k].as_array().cloned().unwrap_or_default().iter().map(|g| {
            let kids: Vec<String> = g["children"].as_array().cloned().unwrap_or_default().iter().take(4).map(|c| format!("{} {}", c["name"].as_str().unwrap_or(""), dur(c["sec"].as_f64().unwrap_or(0.0)))).collect();
            format!("{} {}（{}）", g["name"].as_str().unwrap_or(""), dur(g["sec"].as_f64().unwrap_or(0.0)), kids.join("、"))
        }).collect()
    };
    let (domains, acts) = (tree("domains"), tree("activities"));
    let rules: Vec<(String, String, Option<bool>)> = crate::data::get_rules()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|r| r["enabled"] != false)
        .map(|r| {
            let text = r["text"].as_str().unwrap_or("").to_string();
            let (desc, ok) = check(&text, date, &day);
            (text, desc, ok)
        })
        .collect();
    let hours: Vec<String> = day["hours"].as_array().cloned().unwrap_or_default().iter()
        .filter(|h| h["title"] != "不在电脑前")
        .map(|h| {
            let t = h["start"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|t| t.with_timezone(&Local).hour()).unwrap_or(0);
            format!("{t:02}:00 {}：{}", h["title"].as_str().unwrap_or(""), h["description"].as_str().unwrap_or(""))
        })
        .collect();
    // 当前这个小时还没有小时摘要：补上最近的 10 分钟摘要
    let recent: Vec<String> = day["segments"].as_array().cloned().unwrap_or_default().iter().rev()
        .filter(|s| s["kind"] == "10min" && s["source"] == "beayy").take(6)
        .map(|s| format!("{} {}：{}", s["start"].as_str().and_then(|x| DateTime::parse_from_rfc3339(x).ok()).map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default(), s["title"].as_str().unwrap_or(""), s["description"].as_str().unwrap_or("")))
        .collect();

    let mut user = format!("日期：{}\n\n", date.format("%Y-%m-%d"));
    user += &format!("时长：\n- {}\n\n", stats.join("\n- "));
    if !domains.is_empty() {
        user += &format!("为了什么（领域、项目）：\n- {}\n\n在做什么（类型）：\n- {}\n\n", domains.join("\n- "), acts.join("\n- "));
    }
    if !rules.is_empty() {
        user += &format!("规矩：\n{}\n\n", rules.iter().map(|(t, d, ok)| format!("- {t}：{d}{}", match ok { Some(true) => "，守住了", Some(false) => "，没守住", None => "" })).collect::<Vec<_>>().join("\n"));
    }
    user += &format!("每小时：\n{}\n\n最近：\n{}\n", hours.join("\n"), recent.join("\n"));
    if let Some((_, rows)) = &places {
        user += &format!("\n去了哪：\n{}\n", rows.join("\n"));
    }
    if let Some((_, rows)) = &speech {
        user += &format!("\n对话（录音转写，挑说得多的几个小时，每小时截前面一部分；说话人分不准，标【你】的是你说的）：\n{}\n", rows.join("\n"));
    }
    // 前后 7 天的点评引过的句子：别天天引同一句
    let used: Vec<String> = (-7..=7i64).filter(|k| *k != 0)
        .filter_map(|k| fs::read_to_string(path_of(date + Duration::days(k))).ok().and_then(|t| quote_of(&t)))
        .collect();
    if !used.is_empty() {
        user += &format!("\n前后几天的点评已经引过：\n- {}\n", used.join("\n- "));
    }
    let sm = beayy_summary::Summarizer::new(default_root());
    let j = sm.ask_as("daily", PROMPT, &user)?;
    let roast = if roast_on() {
        let before: Vec<String> = (1..=3i64).rev()
            .filter_map(|k| fs::read_to_string(path_of(date - Duration::days(k))).ok().and_then(|t| section_of(&t, "棒喝")).map(|r| format!("{}：{r}", (date - Duration::days(k)).format("%m-%d"))))
            .collect();
        let used = if before.is_empty() { String::new() } else { format!("\n前几天骂过：\n- {}\n", before.join("\n- ")) };
        roast_of(&sm, "daily", ROAST, &format!("{user}{used}"), 3)
    } else {
        None
    };
    let list = |k: &str| -> String {
        let v: Vec<String> = j.get(k).and_then(Value::as_array).cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(|s| format!("- {s}"))).collect();
        if v.is_empty() { "- 没有".into() } else { v.join("\n") }
    };
    let weekday = ["日", "一", "二", "三", "四", "五", "六"][date.format("%w").to_string().parse::<usize>().unwrap_or(0)];
    let mut md = format!(
        "# {} 周{weekday}\n\n> 由 Beayy 用当天的记录生成，数据截至 {}。\n\n## 今天的一句话\n\n{}\n\n## 时间都去哪了\n\n- {}\n",
        date.format("%m-%d"),
        Local::now().format("%m-%d %H:%M"),
        j.get("line").and_then(Value::as_str).unwrap_or("").trim(),
        stats.join("\n- ")
    );
    // 点评放在一句话后面（总结页排在一句话下面，用引号包住）
    let remark = remark_of(&j);
    if !remark.is_empty() {
        md = md.replacen("\n## 时间都去哪了\n", &format!("\n## 点评\n\n{remark}\n\n## 时间都去哪了\n"), 1);
    }
    if !domains.is_empty() {
        md += &format!("\n为了什么：\n\n- {}\n\n在做什么：\n\n- {}\n", domains.join("\n- "), acts.join("\n- "));
    }
    md += &format!("\n## 做成了什么\n\n{}\n", list("done"));
    if !rules.is_empty() {
        md += &format!("\n## 规矩\n\n{}\n", rules.iter().map(|(t, d, ok)| format!("- {} {t}：{d}", match ok { Some(true) => "✓", Some(false) => "✗", None => "·" })).collect::<Vec<_>>().join("\n"));
    }
    md += &format!("\n## 偏离\n\n{}\n", list("drift"));
    // 棒喝接在偏离后面
    if let Some(r) = &roast {
        md += &format!("\n## 棒喝\n\n{r}\n");
    }
    md += &format!("\n## 明天\n\n{}\n", list("tomorrow"));
    let p = path_of(date);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&p, &md).map_err(|e| e.to_string())?;
    Ok(md)
}

// ---------- 周总结：用那一周的日总结写 ----------

const WEEK_PROMPT: &str = "你是用户的日记助手。下面是用户这一周每天的日总结（有的天可能没有）。
据此写这一周的周总结，全部用中文，用第二人称「你」。line、done、pattern、drift、next 写得具体、平实，不夸、不说教；remark 换一种口吻，见下。只写材料里有的事，不编造，不写材料里没有的数字。
只返回一个 JSON 对象，不要代码块，字段：
  line: 这一周的一句话，40 字以内，抓住这周最主要的事和状态
  quote: 点评开头引的一句话，来源不限：古今中外的诗词、古文、小说、散文、戏剧、电影台词、名人语录都行，挑最贴今天的，别总在几首名诗里打转；只引确实存在、出处确定的，外文用通行的中文译文，别和这周日总结里已经引过的重复；不带引号，不带出处
  source: 这句话的出处，写成「作者《作品》」「《书名·篇名》」，语录没有作品的只写人名（外国人用通行中文译名），不带括号
  remark: 这一周的点评，80–120 字，像一位博览群书、说话风趣的老友在你周记旁写的批注：借 quote 这句话点评这一周，时间、去处、做成的事和偏离。用比喻、对仗、反差制造趣味，要有文学色彩，不要写成流水账；只写材料里有的事，不编造、不说教、不刻薄、不吹捧；不要重复 quote，不要在首尾加引号
  done: 3–6 条字符串，这周做成或推进了什么（写结果，不写过程；同一件事跨几天的合成一条）
  pattern: 1–3 条字符串，时间和作息的规律：时间主要花在哪、哪几天偏多偏少、熬夜和手机的情况
  drift: 0–3 条字符串，偏离：反复没守住的规矩、被不重要的事占掉的时间；没有就空数组
  next: 1–3 条字符串，下周最值得做的事，从这周没做完的里挑
";

/// 点评：模型分开给原句、出处、正文，这里拼成「原句」（出处）正文，总结页按这个格式把引文单独排一行
fn remark_of(j: &serde_json::Map<String, Value>) -> String {
    let get = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let quote = get("quote").trim_matches(|c: char| "「」“”\"'『』".contains(c)).trim().to_string();
    let source = get("source").trim_matches(|c: char| "（）()".contains(c)).trim().to_string();
    let body = get("remark").trim_matches(|c: char| "“”\"".contains(c)).trim().to_string();
    match (quote.is_empty(), source.is_empty()) {
        (false, false) => format!("「{quote}」（{source}）{body}"),
        (false, true) => format!("「{quote}」{body}"),
        _ => body,
    }
}

/// 日总结里点评引的那句话（「」里的部分）；没有就 None
fn quote_of(md: &str) -> Option<String> {
    let mut it = md.lines().skip_while(|l| !l.starts_with("## 点评")).skip(1);
    let l = it.find(|l| !l.trim().is_empty())?.trim();
    let rest = l.strip_prefix('「')?;
    rest.split('」').next().map(|q| q.to_string())
}

/// 一句话：日总结里「今天的一句话」下面那行
fn line_of(md: &str) -> Option<String> {
    let mut it = md.lines().skip_while(|l| !l.starts_with("## 今天的一句话")).skip(1);
    it.find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string())
}

/// 写周一是 monday 的那周的周总结（结果记进 state.json）；返回 Markdown
pub fn generate_week(monday: NaiveDate) -> Result<String, String> {
    let r = write_week(monday);
    record("weeks", &monday.format("%Y-%m-%d").to_string(), 0, &r);
    r
}

fn write_week(monday: NaiveDate) -> Result<String, String> {
    let days: Vec<(NaiveDate, String)> = (0..7)
        .map(|k| monday + Duration::days(k))
        .filter_map(|d| fs::read_to_string(path_of(d)).ok().map(|t| (d, t)))
        .collect();
    if days.is_empty() {
        return Err("这周还没有日总结".into());
    }
    let user: String = days.iter().map(|(d, t)| {
        let body: String = t.lines().filter(|l| !l.starts_with('>') && !l.starts_with("# ")).collect::<Vec<_>>().join("\n");
        format!("==== {} 周{} ====\n{}\n\n", d.format("%Y-%m-%d"), ["日", "一", "二", "三", "四", "五", "六"][d.format("%w").to_string().parse::<usize>().unwrap_or(0)], body.trim())
    }).collect();
    let sm = beayy_summary::Summarizer::new(default_root());
    let j = sm.ask_as("weekly", WEEK_PROMPT, &user)?;
    let roast = if roast_on() { roast_of(&sm, "weekly", WEEK_ROAST, &user, 4) } else { None };
    let list = |k: &str| -> String {
        let v: Vec<String> = j.get(k).and_then(Value::as_array).cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(|s| format!("- {s}"))).collect();
        if v.is_empty() { "- 没有".into() } else { v.join("\n") }
    };
    let sunday = monday + Duration::days(6);
    let remark = remark_of(&j);
    let mut md = format!(
        "# {} – {} 周总结\n\n> 由 Beayy 用这一周的 {} 份日总结生成，数据截至 {}。\n\n## 这周的一句话\n\n{}\n\n## 做成了什么\n\n{}\n\n## 时间和作息\n\n{}\n\n## 偏离\n\n{}\n\n## 下周\n\n{}\n",
        monday.format("%m-%d"),
        sunday.format("%m-%d"),
        days.len(),
        Local::now().format("%m-%d %H:%M"),
        j.get("line").and_then(Value::as_str).unwrap_or("").trim(),
        list("done"),
        list("pattern"),
        list("drift"),
        list("next")
    );
    if !remark.is_empty() {
        md = md.replacen("\n## 做成了什么\n", &format!("\n## 点评\n\n{remark}\n\n## 做成了什么\n"), 1);
    }
    if let Some(r) = &roast {
        md = md.replacen("\n## 下周\n", &format!("\n## 棒喝\n\n{r}\n\n## 下周\n"), 1);
    }
    let p = week_path(monday);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&p, &md).map_err(|e| e.to_string())?;
    Ok(md)
}

// ---------- 月总结：用那个月的周总结和每天的一句话写 ----------

const MONTH_PROMPT: &str = "你是用户的日记助手。下面是用户这一个月的周总结，和每天日总结里的一句话（有的天可能没有）。
据此写这个月的月总结，全部用中文，用第二人称「你」。line、done、pattern、drift、next 写得具体、平实，不夸、不说教；remark 换一种口吻，见下。只写材料里有的事，不编造，不写材料里没有的数字。
只返回一个 JSON 对象，不要代码块，字段：
  line: 这个月的一句话，40 字以内，抓住这个月最主要的事和状态
  quote: 点评开头引的一句话，来源不限：古今中外的诗词、古文、小说、散文、戏剧、电影台词、名人语录都行，挑最贴这个月的，别总在几首名诗里打转；只引确实存在、出处确定的，外文用通行的中文译文，别和这个月周总结里已经引过的重复；不带引号，不带出处
  source: 这句话的出处，写成「作者《作品》」「《书名·篇名》」，语录没有作品的只写人名（外国人用通行中文译名），不带括号
  remark: 这个月的点评，100–150 字，像一位博览群书、说话风趣的老友在你月记旁写的批注：借 quote 这句话点评这个月，时间花在哪、去了哪、做成的事和偏离、前后几周的变化。用比喻、对仗、反差制造趣味，要有文学色彩，不要写成流水账；只写材料里有的事，不编造、不说教、不刻薄、不吹捧；不要重复 quote，不要在首尾加引号
  done: 3–8 条字符串，这个月做成或推进了什么（写结果，不写过程；跨几周的同一件事合成一条）
  pattern: 1–4 条字符串，时间和作息的规律：时间主要花在哪、前后几周怎么变、熬夜和手机的情况
  drift: 0–4 条字符串，偏离：反复没守住的规矩、被不重要的事占掉的时间；没有就空数组
  next: 1–3 条字符串，下个月最值得做的事，从这个月没做完的里挑
";

/// 这个月里的几周（周一在上个月的也算，周一在下个月的不算）
fn mondays_of(first: NaiveDate) -> Vec<NaiveDate> {
    let last = last_of(first);
    let mut out = vec![];
    let mut m = monday_of(first);
    while m <= last {
        out.push(m);
        m += Duration::days(7);
    }
    out
}

/// 写 first 所在那个月的月总结（结果记进 state.json）；返回 Markdown
pub fn generate_month(first: NaiveDate) -> Result<String, String> {
    let first = first_of(first);
    let r = write_month(first);
    record("months", &first.format("%Y-%m").to_string(), 0, &r);
    r
}

fn write_month(first: NaiveDate) -> Result<String, String> {
    let last = last_of(first);
    let lines: Vec<(NaiveDate, String)> = (0..=(last - first).num_days())
        .map(|k| first + Duration::days(k))
        .filter_map(|d| fs::read_to_string(path_of(d)).ok().and_then(|t| line_of(&t)).map(|l| (d, l)))
        .collect();
    if lines.is_empty() {
        return Err("这个月还没有日总结".into());
    }
    let mut user = String::new();
    for m in mondays_of(first) {
        if let Ok(t) = fs::read_to_string(week_path(m)) {
            let body: String = t.lines().filter(|l| !l.starts_with('>') && !l.starts_with("# ")).collect::<Vec<_>>().join("\n");
            user += &format!("==== {} – {} 这一周 ====\n{}\n\n", m.format("%m-%d"), (m + Duration::days(6)).format("%m-%d"), body.trim());
        }
    }
    user += "==== 每天的一句话 ====\n";
    for (d, l) in &lines {
        user += &format!("{} 周{}：{l}\n", d.format("%m-%d"), ["日", "一", "二", "三", "四", "五", "六"][d.format("%w").to_string().parse::<usize>().unwrap_or(0)]);
    }
    let sm = beayy_summary::Summarizer::new(default_root());
    let j = sm.ask_as("monthly", MONTH_PROMPT, &user)?;
    let list = |k: &str| -> String {
        let v: Vec<String> = j.get(k).and_then(Value::as_array).cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(|s| format!("- {s}"))).collect();
        if v.is_empty() { "- 没有".into() } else { v.join("\n") }
    };
    let remark = remark_of(&j);
    let mut md = format!(
        "# {} 月总结\n\n> 由 Beayy 用这个月的 {} 份日总结生成，数据截至 {}。\n\n## 这个月的一句话\n\n{}\n\n## 做成了什么\n\n{}\n\n## 时间和作息\n\n{}\n\n## 偏离\n\n{}\n\n## 下个月\n\n{}\n",
        first.format("%Y-%m"),
        lines.len(),
        Local::now().format("%m-%d %H:%M"),
        j.get("line").and_then(Value::as_str).unwrap_or("").trim(),
        list("done"),
        list("pattern"),
        list("drift"),
        list("next")
    );
    if !remark.is_empty() {
        md = md.replacen("\n## 做成了什么\n", &format!("\n## 点评\n\n{remark}\n\n## 做成了什么\n"), 1);
    }
    let p = month_path(first);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&p, &md).map_err(|e| e.to_string())?;
    Ok(md)
}

// ---------- 页面用的命令 ----------

/// 页面上「现在生成」
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn generate_daily(date: String) -> Result<String, String> {
    let d = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    generate(d)
}

/// 这一天的日总结最近一次没写成：{error, at}；写成了或没试过返回 null
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn daily_status(date: String) -> Value {
    let e = &read_state()["days"][&date];
    if e["error"].is_string() && !path_of(NaiveDate::parse_from_str(&date, "%Y-%m-%d").unwrap_or_default()).exists() {
        serde_json::json!({"error": e["error"], "at": e["failed_at"]})
    } else {
        Value::Null
    }
}

/// 某天所在那周：{monday, sunday, summary_md, error, days: [{date, line}]}
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn get_week(date: String) -> Result<Value, String> {
    let d = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let monday = monday_of(d);
    let days: Vec<Value> = (0..7).map(|k| {
        let x = monday + Duration::days(k);
        let line = read_md(&path_of(x)).and_then(|t| line_of(&t));
        serde_json::json!({"date": x.format("%Y-%m-%d").to_string(), "line": line})
    }).collect();
    let key = monday.format("%Y-%m-%d").to_string();
    let e = &read_state()["weeks"][&key];
    Ok(serde_json::json!({
        "monday": key,
        "sunday": (monday + Duration::days(6)).format("%Y-%m-%d").to_string(),
        "summary_md": read_md(&week_path(monday)),
        "error": if e["error"].is_string() { e["error"].clone() } else { Value::Null },
        "days": days,
    }))
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn generate_weekly(date: String) -> Result<String, String> {
    let d = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    generate_week(monday_of(d))
}

/// 某天所在那个月：{first, last, summary_md, error, weeks: [{monday, sunday, line}]}
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn get_month(date: String) -> Result<Value, String> {
    let first = first_of(NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?);
    let weeks: Vec<Value> = mondays_of(first).into_iter().map(|m| {
        let line = read_md(&week_path(m)).and_then(|t| {
            let mut it = t.lines().skip_while(|l| !l.starts_with("## 这周的一句话")).skip(1);
            it.find(|l| !l.trim().is_empty()).map(|l| l.trim().to_string())
        });
        serde_json::json!({"monday": m.format("%Y-%m-%d").to_string(), "sunday": (m + Duration::days(6)).format("%Y-%m-%d").to_string(), "line": line})
    }).collect();
    let key = first.format("%Y-%m").to_string();
    let e = &read_state()["months"][&key];
    Ok(serde_json::json!({
        "first": first.format("%Y-%m-%d").to_string(),
        "last": last_of(first).format("%Y-%m-%d").to_string(),
        "summary_md": read_md(&month_path(first)),
        "error": if e["error"].is_string() { e["error"].clone() } else { Value::Null },
        "weeks": weeks,
    }))
}

#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn generate_monthly(date: String) -> Result<String, String> {
    let d = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    generate_month(d)
}

// ---------- 定时 ----------

fn modified(p: &PathBuf) -> Option<DateTime<Local>> {
    fs::metadata(p).and_then(|m| m.modified()).ok().map(DateTime::<Local>::from)
}

/// 上次失败的还没到重试时间
fn waiting(e: &Value) -> bool {
    e["retry_at"].as_i64().map(|t| t > Utc::now().timestamp()).unwrap_or(false)
}

/// 前几天要不要（重）写：缺的、按旧写法写的、当天晚上写的（那天还没过完）、之后又来了不少数据的
fn day_due(d: NaiveDate, st: &Value) -> bool {
    let e = &st["days"][d.format("%Y-%m-%d").to_string()];
    let bytes = input_bytes(d);
    let old = e["bytes"].as_u64().unwrap_or(0);
    // 别的设备晚到的（电脑自己那天的在 0:15 重写时已经齐了）
    let grew = bytes > old + 32 * 1024;
    if e["error"].is_string() {
        // 这一天没记录：数据来了再试；别的错：到点再试
        return if e["empty"] == true { grew } else { !waiting(e) };
    }
    if !path_of(d).exists() || e["v"].as_i64().unwrap_or(0) < VERSION {
        return true;
    }
    let at = e["at"].as_i64().and_then(|t| DateTime::from_timestamp(t, 0)).map(|t| t.with_timezone(&Local).date_naive());
    at.map(|a| a <= d).unwrap_or(true) || grew
}

/// 过完的周要不要（重）写：那周有日总结，周总结没写、或之后那周的日总结又重写过
fn week_due(monday: NaiveDate, st: &Value) -> bool {
    let e = &st["weeks"][monday.format("%Y-%m-%d").to_string()];
    if e["error"].is_string() && waiting(e) {
        return false;
    }
    let newest = (0..7).filter_map(|k| modified(&path_of(monday + Duration::days(k)))).max();
    match (newest, modified(&week_path(monday))) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(n), Some(w)) => n > w,
    }
}

/// 过完的月要不要（重）写：那个月有日总结，月总结没写、或之后那个月的日总结、周总结又重写过
fn month_due(first: NaiveDate, st: &Value) -> bool {
    let e = &st["months"][first.format("%Y-%m").to_string()];
    if e["error"].is_string() && waiting(e) {
        return false;
    }
    let last = last_of(first);
    let mut files: Vec<PathBuf> = (0..=(last - first).num_days()).map(|k| path_of(first + Duration::days(k))).collect();
    files.extend(mondays_of(first).into_iter().map(week_path));
    let newest = files.iter().filter_map(modified).max();
    match (newest, modified(&month_path(first))) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(n), Some(m)) => n > m,
    }
}

/// 命令行 --daily catch-up：一轮轮补到没有要写的（每轮都会写的说明有问题，最多 10 轮）
pub fn catch_up_all() {
    for _ in 0..10 {
        if catch_up(Local::now().date_naive()) == 0 {
            break;
        }
    }
}

/// 补：最近 7 天的日总结、最近两周的周总结。一轮最多写 2 份，剩下的下一轮；返回这轮试着写了几份
fn catch_up(today: NaiveDate) -> usize {
    let st = read_state();
    let mut n = 0;
    for k in 1..=7 {
        let d = today - Duration::days(k);
        if n < 2 && day_due(d, &st) {
            let _ = generate(d);
            n += 1;
        }
    }
    if n > 0 {
        return n; // 日总结刚重写过，周总结下一轮再看
    }
    let this = monday_of(today);
    for w in [this - Duration::days(7), this - Duration::days(14)] {
        if n < 2 && week_due(w, &st) {
            let _ = generate_week(w);
            n += 1;
        }
    }
    if n > 0 {
        return n; // 周总结刚重写过，月总结下一轮再看
    }
    // 上个月：过完以后写；月初的几天上个月最后一周的周总结还会重写，跟着重写
    let prev = first_of(first_of(today) - Duration::days(1));
    if month_due(prev, &st) {
        let _ = generate_month(prev);
        n += 1;
    }
    n
}

/// 定时：每天 21:30 写当天；每 10 分钟看一遍前几天和前两周
pub fn start(paused: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        if paused.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let now = Local::now();
        let today = now.date_naive();
        if (now.hour(), now.minute()) >= (21, 30) && !path_of(today).exists() && !waiting(&read_state()["days"][today.format("%Y-%m-%d").to_string()]) {
            let _ = generate(today);
        }
        // 0:15 以前前一天可能还在记（熬夜），不急着重写
        if now.minute() % 10 == 3 && (now.hour(), now.minute()) >= (0, 15) {
            let _ = catch_up(today);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remark_parts() {
        let j = serde_json::json!({"quote": "“行到水穷处，坐看云起时”", "source": "（王维《终南别业》）", "remark": "你今天……"});
        let r = remark_of(j.as_object().unwrap());
        assert_eq!(r, "「行到水穷处，坐看云起时」（王维《终南别业》）你今天……");
        assert_eq!(quote_of(&format!("## 今天的一句话\n\n一句\n\n## 点评\n\n{r}\n")).as_deref(), Some("行到水穷处，坐看云起时"));
        assert_eq!(remark_of(serde_json::json!({"remark": "只有正文"}).as_object().unwrap()), "只有正文");
    }

    #[test]
    fn sections() {
        let md = "# 11-02\n\n## 偏离\n\n- 一条\n\n## 棒喝\n\n骂了一段\n第二行\n\n## 明天\n\n- 打电话\n";
        assert_eq!(section_of(md, "棒喝").as_deref(), Some("骂了一段\n第二行"));
        assert_eq!(section_of(md, "点评"), None);
    }

    #[test]
    fn roast_check() {
        assert!(roast_faults("操，你他妈的又切了 700 次，扯淡。", 3).is_empty());
        assert_eq!(roast_faults("你他妈的又切了 700 次。", 3).len(), 1);
        assert_eq!(roast_faults("操他妈扯淡。你不是没本事，是怕交卷", 3).len(), 1);
        assert_eq!(roast_faults("操他妈扯淡。这不是并行推进，而是每件事都摸一下", 3).len(), 1);
    }
}
