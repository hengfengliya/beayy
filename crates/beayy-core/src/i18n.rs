//! 界面语言和译文：和网页同一张表 `app/ui/locales/en.js`（键是中文原句，值是英文；写法见 ui/i18n.js）。
//! 只有 Rust 自己显示的文字（菜单栏菜单）用它；返回给网页的报错、标签照旧写中文原句，由网页在显示前查同一张表。
//!
//! 语言：settings.json 的 ui_lang（界面启动时报的、已经按用户选择定好的 zh／en），没有就看系统语言：zh 开头用中文，其余用英文。

use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

const EN_JS: &str = include_str!("../../../ui/locales/en.js");

fn table() -> &'static HashMap<String, Value> {
    static T: OnceLock<HashMap<String, Value>> = OnceLock::new();
    T.get_or_init(|| parse(EN_JS))
}

/// en.js 去掉 `window.I18N_EN = ` 和结尾的 `;`，中间是 JSON
fn parse(js: &str) -> HashMap<String, Value> {
    let body = js.find("window.I18N_EN").and_then(|i| js[i..].find('{').map(|j| i + j)).zip(js.rfind('}')).map(|(a, b)| &js[a..=b]);
    body.and_then(|b| serde_json::from_str(b).ok()).unwrap_or_default()
}

/// 现在的界面语言：zh／en
pub fn lang() -> &'static str {
    let set = std::fs::read_to_string(crate::default_root().join("settings.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    match set.as_ref().and_then(|v| v.get("ui_lang")).and_then(Value::as_str) {
        Some("zh") => "zh",
        Some("en") => "en",
        _ => {
            if sys_locale::get_locale().map(|l| l.to_lowercase().starts_with("zh")).unwrap_or(true) {
                "zh"
            } else {
                "en"
            }
        }
    }
}

/// 一句话的译文：中文界面原样；英文界面查表，没有就退回中文。{名字} 换成 vars 里的值
pub fn tr_with(s: &str, vars: &[(&str, &str)]) -> String {
    let en = lang() == "en";
    let mut out = if en { pick(table().get(s), vars).unwrap_or_else(|| s.to_string()) } else { s.to_string() };
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

pub fn tr(s: &str) -> String {
    tr_with(s, &[])
}

/// 拼好的一句（报错、状态）：先整句查表，查不到就按表里带 {…} 的原句当模板对，对上了把各段值也翻了再填进英文。
/// 和网页的 trMsg（ui/i18n.js）同一套规则
pub fn tr_msg(s: &str) -> String {
    if lang() != "en" || !s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
        return s.to_string();
    }
    if let Some(v) = pick(table().get(s), &[]) {
        return v;
    }
    for (key, en) in templates() {
        if let Some(vals) = key.matches(s) {
            let vals: Vec<String> = vals.iter().map(|v| tr_msg(v)).collect();
            return key.fill(en, &vals);
        }
    }
    s.to_string()
}

/// 表里带占位符的原句，拆成「字面段、占位符名」交替的模板；字面段长的排前面，先对具体的
struct Template {
    lits: Vec<String>,
    names: Vec<String>,
}

fn placeholders(s: &str) -> (Vec<String>, Vec<String>) {
    let (mut lits, mut names, mut rest) = (vec![], vec![], s);
    while let Some(a) = rest.find('{') {
        let Some(b) = rest[a..].find('}').map(|b| a + b) else { break };
        lits.push(rest[..a].to_string());
        names.push(rest[a + 1..b].split(':').next().unwrap_or("").to_string());
        rest = &rest[b + 1..];
    }
    lits.push(rest.to_string());
    (lits, names)
}

impl Template {
    /// 整句对上就返回各占位符的值（每段至少一个字，尽量短）
    fn matches(&self, s: &str) -> Option<Vec<String>> {
        let rest = s.strip_prefix(self.lits[0].as_str())?;
        let mut vals = vec![];
        let mut rest = rest;
        for (i, lit) in self.lits[1..].iter().enumerate() {
            let last = i + 2 == self.lits.len();
            let at = if last {
                if lit.is_empty() { Some(rest.len()) } else { rest.strip_suffix(lit.as_str()).map(str::len) }
            } else {
                rest.char_indices().skip(1).map(|(j, _)| j).find(|&j| rest[j..].starts_with(lit.as_str()))
            }?;
            if at == 0 {
                return None;
            }
            vals.push(rest[..at].to_string());
            rest = &rest[at + lit.len()..];
        }
        rest.is_empty().then_some(vals)
    }

    /// 英文模板里的占位符：有名字的按名字取，空的 {} 按出现的先后取
    fn fill(&self, en: &str, vals: &[String]) -> String {
        let (lits, names) = placeholders(en);
        let mut out = lits[0].clone();
        let mut nth = 0;
        for (name, lit) in names.iter().zip(&lits[1..]) {
            let i = if name.is_empty() {
                let k = self.names.iter().enumerate().filter(|(_, n)| n.is_empty()).nth(nth).map(|(i, _)| i);
                nth += 1;
                k
            } else {
                self.names.iter().position(|n| n == name)
            };
            out += i.and_then(|i| vals.get(i)).map(String::as_str).unwrap_or("");
            out += lit;
        }
        out
    }
}

fn templates() -> &'static Vec<(Template, String)> {
    static T: OnceLock<Vec<(Template, String)>> = OnceLock::new();
    T.get_or_init(|| {
        let mut v: Vec<(Template, String)> = table()
            .iter()
            // 两个占位符挨着的（{e}{BUSY}）对不准，不当模板
            .filter(|(k, _)| k.contains('{') && k.contains('}') && !k.contains("}{"))
            .filter_map(|(k, val)| {
                let en = pick(Some(val), &[])?;
                let (lits, names) = placeholders(k);
                (!names.is_empty()).then_some((Template { lits, names }, en))
            })
            .collect();
        v.sort_by_key(|(t, _)| std::cmp::Reverse(t.lits.iter().map(|l| l.chars().count()).sum::<usize>()));
        v
    })
}

/// 表里的值：字符串，或按 n 分单复数的 {one, other}
fn pick(v: Option<&Value>, vars: &[(&str, &str)]) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Object(m) => {
            let one = vars.iter().find(|(k, _)| *k == "n").map(|(_, v)| *v == "1").unwrap_or(false);
            m.get(if one { "one" } else { "other" }).and_then(Value::as_str).map(String::from)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_parses() {
        let t = table();
        assert!(t.len() > 500, "en.js 读出 {} 条", t.len());
        assert_eq!(pick(t.get("关闭"), &[]).as_deref(), Some("Close"));
    }

    #[test]
    fn template_fill() {
        let (lits, names) = placeholders("上传 {k}：{e}");
        let t = Template { lits, names };
        let v = t.matches("上传 mac/2026：连不上").unwrap();
        assert_eq!(v, vec!["mac/2026".to_string(), "连不上".to_string()]);
        assert_eq!(t.fill("Upload {k}: {e}", &v), "Upload mac/2026: 连不上");
        let (lits, names) = placeholders("第 {} 段，共 {} 段");
        let t = Template { lits, names };
        assert_eq!(t.fill("{} of {}", &t.matches("第 3 段，共 9 段").unwrap()), "3 of 9");
        assert!(t.matches("第 3 段").is_none());
    }
}
