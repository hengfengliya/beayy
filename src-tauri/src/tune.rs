//! 设置 → AI 的「摘要关注点」「总结写法」：用没存的草稿在自己的数据上试一次（不写文件），和让 AI 把大白话改成要求。
//! 要求本身存在 settings.json 的 summary_focus、summary_style，自定义段存在 summary_focus_section、summary_style_section（{title, ask}），
//! 由 beayy_summary 在问模型时接上（见 custom_note）。

use beayy_core::default_root;
use chrono::{Duration, Local};
use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;

static FRONT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(title|description): (.*)$").unwrap());
static CTX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?ms)^### Important non-obvious context about the user\n+(.+?)(?:\n#|\z)").unwrap());
static REC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?ms)^## Recording summary\n+(.+?)(?:\n## |\z)").unwrap());

/// 摘要 Markdown 里页面要显示的几样（和 data.rs 读摘要的口径一致）
fn seg_view(md: &str) -> Value {
    let head = md.split("\n---").next().unwrap_or("");
    let front = |k: &str| FRONT.captures_iter(head).find(|c| &c[1] == k).map(|c| c[2].to_string()).unwrap_or_default();
    let ctx: Vec<String> = CTX.captures(md).map(|c| c[1].lines().filter_map(|l| l.strip_prefix("- ")).map(|l| l.trim().to_string()).collect()).unwrap_or_default();
    json!({
        "title": front("title"), "description": front("description"), "context": ctx,
        "recording": REC.captures(md).map(|c| c[1].trim().to_string()).unwrap_or_default(),
        "custom": beayy_summary::custom_of(md).map(|(title, items)| json!({"title": title, "items": items})),
    })
}

/// 摘要关注点预览：最近一份有内容的摘要，原来的和按草稿重写的
#[tauri::command(async)]
pub fn preview_focus(text: String, section: Value) -> Result<Value, String> {
    let sm = beayy_summary::Summarizer::new(default_root()).with_draft("summary_focus", json!(text)).with_draft("summary_focus_section", section);
    let (old, new, start, mins) = sm.preview_segment()?;
    Ok(json!({ "start": start.to_rfc3339(), "end": (start + Duration::minutes(mins)).to_rfc3339(), "before": seg_view(&old), "after": seg_view(&new) }))
}

/// 总结写法预览：按草稿重写昨天的日总结（昨天没有材料就用今天），和已有的一起回来；不写棒喝
#[tauri::command(async)]
pub fn preview_style(text: String, section: Value) -> Result<Value, String> {
    let sm = beayy_summary::Summarizer::new(default_root()).with_draft("summary_style", json!(text)).with_draft("summary_style_section", section);
    let today = Local::now().date_naive();
    let mut last = String::new();
    for date in [today - Duration::days(1), today] {
        match crate::daily::compose_day(date, &sm, false) {
            Ok(md) => {
                let before = std::fs::read_to_string(crate::daily::path_of(date)).ok();
                return Ok(json!({ "date": date.format("%Y-%m-%d").to_string(), "before": before, "after": md }));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

const WRITE_FOCUS: &str = "用户在调 Beayy（自动记录电脑活动的 app）每 10 分钟一份的活动摘要：摘要默认写这段时间在做什么、用了什么、有什么结果。下面是用户用大白话说的，希望摘要另外多留、写细哪些内容。
把它改写成给写摘要的模型看的要求：3–6 条，每条一行、以「- 」开头，具体到要留什么、留多细（比如要不要保留原话）、写进哪里（经过 recording 或线索 context）。
不要求改摘要的字段和格式；用户没提的不加。用户用什么语言写，就用什么语言写。
用户想把某类内容在摘要里单独列成一栏（比如「想法」「待办」）的，另给这一栏的名字（6 字以内）和这一栏写什么（一句话）；没提就留空。
只返回一个 JSON 对象，不要代码块：{\"rules\": \"- …\\n- …\"（没有别的要求就空字符串）, \"section_title\": \"\", \"section_ask\": \"\"}";

const WRITE_STYLE: &str = "用户在调 Beayy（自动记录电脑活动的 app）的日总结、周总结、月总结：默认有一句话、引一句名言的文学点评、做成了什么、偏离、下一步，语气像老友写批注。下面是用户用大白话说的，希望总结怎么写。
把它改写成给写总结的模型看的要求：3–6 条，每条一行、以「- 」开头，写清侧重什么、语气、详略、哪几样不要（比如不要引言和点评）。日、周、月共用这几条，所以说「这段时间」，不说「今天」。
用户没提的不加。用户用什么语言写，就用什么语言写。
用户想在总结里单独多一栏（比如「新想法」「待跟进」）的，另给这一栏的名字（6 字以内）和这一栏写什么（一句话）；没提就留空。
只返回一个 JSON 对象，不要代码块：{\"rules\": \"- …\\n- …\"（没有别的要求就空字符串）, \"section_title\": \"\", \"section_ask\": \"\"}";

/// 让 AI 把大白话改成要求：kind 是 focus（摘要关注点）或 style（总结写法）。返回 {text, title, ask}（后两样是自定义段，没有为空）
#[tauri::command(async)]
pub fn ai_write_note(kind: String, wish: String) -> Result<Value, String> {
    let wish = wish.trim();
    if wish.is_empty() {
        return Err("先用自己的话写想要什么".into());
    }
    let system = if kind == "focus" { WRITE_FOCUS } else { WRITE_STYLE };
    let j = beayy_summary::Summarizer::new(default_root()).ask_as("other", system, wish)?;
    let get = |k: &str| j.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (text, title) = (get("rules"), beayy_summary::section_title(&get("section_title")));
    if text.is_empty() && title.is_empty() {
        return Err("模型没给出内容，再试一次".into());
    }
    Ok(json!({ "text": text, "title": title, "ask": if title.is_empty() { String::new() } else { get("section_ask") } }))
}
