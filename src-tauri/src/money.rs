//! 钱：在账单（bills.rs 的原始层、统一层）上面算解释层和汇总。方案见 报告/方案_20261104_金钱管理.md。
//! - 合计按账单的收/支（和两家账单自带的合计一致），往下先按交易类型拆，消费再按消费类型拆。
//! - 交易类型（钱怎么动的，每笔都有）：微信看交易类型；支付宝看交易分类（它把交易类型和消费类型混在一列），
//!   只有网页版、没有交易分类的按说明猜。
//! - 消费类型（花在什么上，只给消费），依次看：你手改的 → 你定的（同一个对方都这样）→ 支付宝账单自带的
//!   → 内置关键词 → AI 分的（只发对方名和说明）→ 账单写的「其他」；都没有是「未分类」。
//! - 都是读的时候现算：手改存 bills/overrides.json，你定的规则 bills/rules.json，AI 分的 bills/ai.json，改了马上生效，不碰账单本身。
//! - 每月的「钱」总结（AI 写）存 bills/insights/YYYY-MM.md，材料只给汇总，不给逐笔流水。

use crate::bills::{self, Bill};
use beayy_core::{data_root, default_root};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

pub const TTYPES: [&str; 10] = ["消费", "收入", "转账", "红包", "亲友代付", "退款", "还款", "理财", "账户存取", "其他"];
/// 消费类型：支付宝交易分类里属于消费的那些
pub const CTYPES: [&str; 24] = [
    "餐饮美食", "日用百货", "服饰装扮", "美容美发", "家居家装", "数码电器", "运动户外", "母婴亲子", "宠物", "交通出行", "爱车养车", "住房物业",
    "酒店旅游", "文化休闲", "教育培训", "医疗健康", "生活服务", "公共服务", "商业服务", "充值缴费", "保险", "互助保障", "公益捐赠", "其他",
];
/// 转出去给人的几类（概览里和消费分开看）
const TO_PEOPLE: [&str; 3] = ["转账", "红包", "亲友代付"];
/// 内置关键词（对方名＋说明里找），靠前的先认：打车先于餐饮（美团也做打车）
const KEYWORDS: &[(&str, &[&str])] = &[
    ("交通出行", &["滴滴", "打车", "地铁", "公交", "12306", "铁路", "航空", "机票", "加油", "停车", "高速", "哈啰", "青桔", "曹操出行", "T3出行", "出行"]),
    ("酒店旅游", &["酒店", "宾馆", "携程", "飞猪", "去哪儿", "民宿", "景区", "门票", "Hotel"]),
    ("充值缴费", &["电费", "水费", "燃气", "话费", "宽带", "中国移动", "中国联通", "中国电信", "供电"]),
    ("医疗健康", &["医院", "药房", "药店", "大药", "诊所", "医保", "体检", "口腔"]),
    ("商业服务", &["阿里云", "腾讯云", "云服务", "域名", "iCloud", "OpenAI", "Anthropic", "ChatGPT"]),
    ("文化休闲", &["电影", "影城", "影院", "KTV", "网咖", "网吧", "游戏", "点券", "书店", "演出", "爱奇艺", "腾讯视频", "优酷", "哔哩"]),
    ("公共服务", &["出入境", "财政", "税务", "政务", "交警", "公积金"]),
    ("运动户外", &["健身", "体育", "球馆", "游泳", "瑜伽"]),
    ("美容美发", &["理发", "美发", "美容", "美甲"]),
    ("住房物业", &["房租", "物业", "租房", "自如", "链家"]),
    ("教育培训", &["课程", "培训", "学费"]),
    ("餐饮美食", &["美团", "饿了么", "外卖", "肯德基", "麦当劳", "瑞幸", "星巴克", "咖啡", "奶茶", "茶饮", "餐厅", "饭店", "面馆", "火锅", "烧烤", "蜜雪", "喜茶", "霸王茶姬", "必胜客", "汉堡", "食堂", "小吃", "餐饮", "luckin", "Luckin", "coffee", "Coffee", "COFFEE", "Starbucks"]),
    ("日用百货", &["超市", "便利", "罗森", "全家", "7-11", "ELEVEN", "盒马", "永辉", "物美", "京东", "拼多多"]),
];

fn dir() -> PathBuf {
    data_root().join("bills")
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
struct Class {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ttype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ctype: Option<String>,
}

/// 你定的：同一个对方以后都这样
#[derive(Serialize, Deserialize, Clone)]
struct UserRule {
    counterparty: String,
    #[serde(flatten)]
    class: Class,
}

fn read<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    fs::read(dir().join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write<T: Serialize>(name: &str, v: &T) -> Result<(), String> {
    let p = dir().join(name);
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(tmp, p).map_err(|e| e.to_string())
}

fn ai_key(b: &Bill) -> String {
    format!("{}\u{1}{}", b.counterparty, b.item)
}

/// 一笔加上算出来的：交易类型、消费类型（各自是谁定的）、退了多少、计入支出收入多少、账户、渠道、服务费
pub struct View<'a> {
    pub b: &'a Bill,
    pub ttype: String,
    pub ttype_by: &'static str,
    pub ctype: String,
    pub ctype_by: &'static str,
    pub refund: i64,
    pub out: i64,
    pub inc: i64,
    pub counted: bool,
    pub account: String,
    pub credit: bool,
    pub discount: String,
    pub channel: String,
    pub fee: i64,
}

/// 同一个人的几种写法并成一个：微信「备注 (昵称)」「备注（说明）」「发给备注」、支付宝「备注(**名)」都取备注
fn person(name: &str) -> String {
    let mut s = name.trim_matches(|c: char| c.is_whitespace() || c.is_control()).trim_start_matches("发给").to_string();
    if let Some(i) = s.find(" (") {
        s.truncate(i);
    }
    if s.ends_with('）') {
        if let Some(i) = s.find('（') {
            s.truncate(i);
        }
    }
    if s.ends_with(')') {
        if let Some(i) = s.rfind('(').filter(|i| s[*i..].contains('*')) {
            s.truncate(i);
        }
    }
    let s = s.trim().to_string();
    if s.is_empty() { name.trim().to_string() } else { s }
}

impl View<'_> {
    /// 对方：转账、红包、亲友代付按人并（见 person）
    pub fn who(&self) -> String {
        let b = self.b;
        let name = b.counterparty.trim_matches(|c: char| c.is_whitespace() || c.is_control());
        if name.is_empty() {
            b.item.clone()
        } else if TO_PEOPLE.contains(&self.ttype.as_str()) {
            person(name)
        } else {
            name.to_string()
        }
    }
    /// 该细分却没分：没有消费类型，或只有账单上写的「其他」
    fn unsorted(&self) -> bool {
        self.counted && self.ttype == "消费" && (self.ctype.is_empty() || self.ctype == "其他" && self.ctype_by == "bill")
    }
    fn json(&self) -> Value {
        let b = self.b;
        json!({
            "id": b.id, "src": b.src, "time": b.time, "dir": b.dir, "fen": b.fen, "refund_fen": self.refund, "counted": self.counted, "is_refund": b.is_refund,
            "ttype": self.ttype, "ttype_by": self.ttype_by, "ctype": self.ctype, "ctype_by": self.ctype_by, "who": self.who(),
            "counterparty": b.counterparty, "item": b.item, "account": self.account, "credit": self.credit, "discount": self.discount,
            "channel": self.channel, "fee": self.fee, "status": b.status, "note": b.note, "kind": b.kind,
        })
    }
}

/// 机器认的交易类型（不看手改和你定的规则）
fn ttype_auto(b: &Bill) -> &'static str {
    if b.is_refund {
        return "退款";
    }
    let has = |k: &str| b.item.contains(k);
    if b.src == "wechat" {
        let t = b.kind.as_str();
        return if t.contains("退款") {
            "退款"
        } else if t.starts_with("转账") {
            "转账"
        } else if t.starts_with("微信红包") {
            "红包"
        } else if t.contains("亲属卡") {
            "亲友代付"
        } else if t.contains("还款") {
            "还款"
        } else if t.contains("零钱通") || t.contains("理财") {
            "理财"
        } else if t.contains("提现") || t.contains("充值") {
            "账户存取"
        } else if b.dir == "out" || t.contains("消费") || t.contains("付款") {
            "消费"
        } else if b.dir == "in" {
            "收入"
        } else {
            "其他"
        };
    }
    match b.raw.get("交易分类").map(String::as_str).unwrap_or("") {
        "转账红包" => if has("红包") { "红包" } else { "转账" },
        "亲友代付" => "亲友代付",
        "退款" => "退款",
        "信用借还" => "还款",
        "投资理财" => "理财",
        "账户存取" => "账户存取",
        "收入" => "收入",
        "" => {
            // 只有网页版：按说明猜
            if has("红包") {
                "红包"
            } else if b.item.starts_with("转账") || has("自愿赠与") || b.raw.get("交易来源地").map(|s| s == "支付宝网站").unwrap_or(false) && b.item.is_empty() {
                "转账"
            } else if has("还款") {
                "还款"
            } else if b.counterparty.contains("余额宝") || has("余额宝") || has("收益") || has("基金") {
                "理财"
            } else if has("提现") || has("充值") {
                "账户存取"
            } else if b.dir == "in" {
                "收入"
            } else if b.dir == "none" {
                "其他"
            } else {
                "消费"
            }
        }
        _ => "消费",
    }
}

fn keyword_ctype(b: &Bill) -> Option<&'static str> {
    let text = format!("{} {}", b.counterparty, b.item);
    KEYWORDS.iter().find(|(_, ks)| ks.iter().any(|k| text.contains(k))).map(|(c, _)| *c)
}

fn account_of(method: &str) -> (String, bool, String) {
    let mut parts = method.split('&');
    let main = parts.next().unwrap_or("").trim().to_string();
    let rest: Vec<&str> = parts.map(str::trim).filter(|s| !s.is_empty()).collect();
    let credit = ["花呗", "信用卡", "信用购", "VISA", "Visa", "MasterCard", "白条", "月付"].iter().any(|k| main.contains(k));
    (main, credit, rest.join("、"))
}

fn fee_of(b: &Bill) -> i64 {
    let col = b.raw.get("服务费（元）").and_then(|v| v.parse::<f64>().ok()).map(|v| (v * 100.0).round() as i64).unwrap_or(0);
    if col > 0 {
        return col;
    }
    let re = regex::Regex::new(r"服务费[¥￥]?\s*([\d.]+)").unwrap();
    re.captures(&b.note).and_then(|c| c[1].parse::<f64>().ok()).map(|v| (v * 100.0).round() as i64).unwrap_or(0)
}

pub struct Ctx {
    all: BTreeMap<String, Bill>,
    overrides: HashMap<String, Class>,
    rules: Vec<UserRule>,
    ai: HashMap<String, String>,
    refunds: HashMap<String, i64>,
}

pub fn ctx() -> Ctx {
    let all = bills::load(&bills::store_path());
    let refunds = bills::refunds(&all);
    Ctx { overrides: read("overrides.json"), rules: read("rules.json"), ai: read("ai.json"), refunds, all }
}

impl Ctx {
    pub fn view<'a>(&'a self, b: &'a Bill) -> View<'a> {
        let ov = self.overrides.get(&b.id);
        let rule = self.rules.iter().find(|r| !b.counterparty.is_empty() && r.counterparty == b.counterparty).map(|r| &r.class);
        let (ttype, ttype_by) = if let Some(t) = ov.and_then(|o| o.ttype.clone()) {
            (t, "manual")
        } else if let Some(t) = rule.and_then(|r| r.ttype.clone()) {
            (t, "rule")
        } else {
            (ttype_auto(b).to_string(), "bill")
        };
        // 你改的、你定的在账单自带的前面（支付宝的分类常不准：阿里云算日用百货）；账单写「其他」等于没分
        let (mut ctype, mut ctype_by) = (String::new(), "");
        if ttype == "消费" {
            let bill_cat = b.raw.get("交易分类").filter(|c| CTYPES.contains(&c.as_str())).cloned();
            if let Some(c) = ov.and_then(|o| o.ctype.clone()) {
                (ctype, ctype_by) = (c, "manual");
            } else if let Some(c) = rule.and_then(|r| r.ctype.clone()) {
                (ctype, ctype_by) = (c, "rule");
            } else if let Some(c) = bill_cat.clone().filter(|c| c != "其他") {
                (ctype, ctype_by) = (c, "bill");
            } else if let Some(c) = keyword_ctype(b) {
                (ctype, ctype_by) = (c.to_string(), "keyword");
            } else if let Some(c) = self.ai.get(&ai_key(b)) {
                (ctype, ctype_by) = (c.clone(), "ai");
            } else if let Some(c) = bill_cat {
                (ctype, ctype_by) = (c, "bill");
            }
        }
        let refund = if b.dir == "out" { self.refunds.get(&b.id).copied().unwrap_or(0) } else { 0 };
        let (out, inc) = bills::counted(b, refund);
        let method = if b.method.is_empty() { b.raw.get("收/付款方式").cloned().unwrap_or_default() } else { b.method.clone() };
        let (account, credit, discount) = account_of(&method);
        View {
            b,
            ttype,
            ttype_by,
            ctype,
            ctype_by,
            refund,
            out,
            inc,
            counted: !b.is_refund && b.dir != "none",
            account,
            credit,
            discount,
            channel: b.raw.get("交易来源地").cloned().unwrap_or_default(),
            fee: fee_of(b),
        }
    }

    pub fn views(&self) -> Vec<View<'_>> {
        let mut v: Vec<View> = self.all.values().map(|b| self.view(b)).collect();
        v.sort_by(|a, b| b.b.time.cmp(&a.b.time));
        v
    }
}

// ---------- 月份 ----------

fn month_add(m: &str, k: i32) -> String {
    let (y, mo): (i32, i32) = (m[..4].parse().unwrap_or(2026), m[5..7].parse().unwrap_or(1));
    let t = y * 12 + mo - 1 + k;
    format!("{:04}-{:02}", t.div_euclid(12), t.rem_euclid(12) + 1)
}

fn this_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

/// 精确到分（给模型的材料）
fn yuan(f: i64) -> String {
    format!("{:.2}", f as f64 / 100.0)
}

/// 给人看的：一万以上「14.3 万元」，以下「4,326.90 元」
fn money(f: i64) -> String {
    let v = f as f64 / 100.0;
    if v.abs() >= 10000.0 {
        return format!("{:.1} 万元", v / 10000.0);
    }
    let s = format!("{:.2}", v.abs());
    let (int, dec) = s.split_at(s.len() - 3);
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}{out}{dec} 元", if v < 0.0 { "−" } else { "" })
}

/// 和平均比（base：「前三个月平均」「前三个月同期平均」）：翻倍以上写「是…的 N 倍」，否则「多／少 X%」
fn versus(cur: i64, avg: i64, base: &str) -> String {
    let r = cur as f64 / avg as f64;
    if r >= 10.0 { format!("是{base}的 {r:.0} 倍") } else if r >= 2.0 { format!("是{base}的 {r:.1} 倍") } else { format!("比{base}{} {:.0}%", if r >= 1.0 { "多" } else { "少" }, (r - 1.0).abs() * 100.0) }
}

/// 一个月的最后一天 YYYY-MM-DD
fn month_end(m: &str) -> String {
    chrono::NaiveDate::parse_from_str(&format!("{}-01", month_add(m, 1)), "%Y-%m-%d").map(|d| (d - chrono::Duration::days(1)).format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// 有账单的月份（新的在前）
fn months_of(vs: &[View]) -> Vec<String> {
    let mut m: Vec<String> = vs.iter().map(|v| v.b.time[..7].to_string()).collect();
    m.sort();
    m.dedup();
    m.reverse();
    m
}

// ---------- 给网页的命令 ----------

/// 一段时间（YYYY-MM 一个月，YYYY 一整年；不给＝有账单的最近一个月）的每一笔，加上覆盖、导入记录、类型清单
#[tauri::command(async)]
pub fn get_bills(month: Option<String>) -> Value {
    let c = ctx();
    let vs = c.views();
    let months = months_of(&vs);
    let period = month.filter(|m| m.len() == 7 || m.len() == 4).or_else(|| months.first().cloned()).unwrap_or_else(this_month);
    let mut years: Vec<String> = months.iter().map(|m| m[..4].to_string()).collect();
    years.dedup();
    let unclassified = vs.iter().filter(|v| v.unsorted()).count();
    json!({
        "month": period,
        "months": months,
        "years": years,
        "items": vs.iter().filter(|v| v.b.time.starts_with(&period)).map(View::json).collect::<Vec<_>>(),
        "coverage": bills::coverage(&c.all),
        "imports": bills::imports_view(),
        "ttypes": TTYPES,
        "ctypes": CTYPES,
        "unclassified": unclassified,
    })
}

/// 一笔的全部：算出来的和原表格的每一列
#[tauri::command(async)]
pub fn get_bill(id: String) -> Value {
    let c = ctx();
    let Some(b) = c.all.get(&id) else { return Value::Null };
    let v = c.view(b);
    let mut j = v.json();
    j["raw"] = json!(b.raw);
    j["export"] = json!(b.export);
    j["remembered"] = json!(c.rules.iter().any(|r| !b.counterparty.is_empty() && r.counterparty == b.counterparty));
    j
}

/// 改一笔的交易类型、消费类型（都不给＝撤销手改，回到机器认的）；remember＝同一个对方以后都这样
#[tauri::command(async)]
pub fn set_bill_class(id: String, ttype: Option<String>, ctype: Option<String>, remember: Option<bool>) -> Result<(), String> {
    let c = ctx();
    let b = c.all.get(&id).ok_or("找不到这一笔")?;
    let class = Class { ttype: ttype.filter(|t| TTYPES.contains(&t.as_str())), ctype: ctype.filter(|t| CTYPES.contains(&t.as_str())) };
    let mut ov = c.overrides.clone();
    if class == Class::default() {
        ov.remove(&id);
    } else {
        ov.insert(id.clone(), class.clone());
    }
    write("overrides.json", &ov)?;
    if remember.unwrap_or(false) && !b.counterparty.is_empty() {
        let mut rules = c.rules.clone();
        rules.retain(|r| r.counterparty != b.counterparty);
        if class != Class::default() {
            rules.push(UserRule { counterparty: b.counterparty.clone(), class });
        }
        write("rules.json", &rules)?;
    }
    Ok(())
}

const CLASSIFY_PROMPT: &str = "你帮用户给消费记账分类。下面每行是一笔消费的「对方」和「说明」（来自微信、支付宝账单），按编号给出消费类型。
消费类型只能从这些里选一个：{CTYPES}。拿不准的选「其他」。
只返回一个 JSON 对象，不要代码块：{\"items\": [{\"i\": 编号, \"c\": \"消费类型\"}]}";

/// 规则分不出的消费交给 AI 分：只发对方名和说明（不发金额、时间、账号），同样的对方和说明只问一次，结果存 bills/ai.json
#[tauri::command(async)]
pub fn classify_bills_ai() -> Value {
    let c = ctx();
    let mut todo: Vec<(String, String, String)> = vec![];
    for v in c.views() {
        if v.unsorted() {
            let k = ai_key(v.b);
            if !todo.iter().any(|t| t.0 == k) {
                todo.push((k, v.b.counterparty.clone(), v.b.item.clone()));
            }
        }
    }
    let total = todo.len();
    let sm = beayy_summary::Summarizer::new(default_root());
    let system = CLASSIFY_PROMPT.replace("{CTYPES}", &CTYPES.join("、"));
    let mut ai = c.ai.clone();
    let (mut done, mut error) = (0, None);
    for batch in todo.chunks(60) {
        let user: String = batch.iter().enumerate().map(|(i, (_, who, item))| format!("{i}. 对方：{who}；说明：{item}\n")).collect();
        match sm.ask_as("money", &system, &user) {
            Ok(j) => {
                for it in j.get("items").and_then(Value::as_array).cloned().unwrap_or_default() {
                    let (Some(i), Some(t)) = (it["i"].as_u64(), it["c"].as_str()) else { continue };
                    if let (Some(x), true) = (batch.get(i as usize), CTYPES.contains(&t)) {
                        ai.insert(x.0.clone(), t.to_string());
                        done += 1;
                    }
                }
                let _ = write("ai.json", &ai);
            }
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    json!({"asked": total, "done": done, "error": error})
}

// ---------- 汇总：概览、提醒、预算 ----------

#[derive(Default, Clone, Copy)]
struct Sum {
    out: i64,
    inc: i64,
    consume: i64,
    n: i64,
}

fn sum_of<'a>(it: impl Iterator<Item = &'a View<'a>>) -> Sum {
    let mut s = Sum::default();
    for v in it.filter(|v| v.counted) {
        s.out += v.out;
        s.inc += v.inc;
        s.n += 1;
        if v.ttype == "消费" {
            s.consume += v.out;
        }
    }
    s
}

/// 规矩里的预算：「外卖每月不超过 1500」「餐饮美食每月不超过 3000 元」「消费每月不超过 2 万」
fn parse_budget(text: &str) -> Option<(String, i64)> {
    let re = regex::Regex::new(r"^\s*(.+?)\s*每月(?:不超过|不能超过|最多|少于|控制在|不到)\s*([\d,.]+)\s*(万)?\s*(?:元|块)?\s*(?:以内|以下)?\s*$").unwrap();
    let c = re.captures(text)?;
    let n: f64 = c[2].replace(',', "").parse().ok()?;
    Some((c[1].to_string(), (n * if c.get(3).is_some() { 10000.0 } else { 1.0 } * 100.0).round() as i64))
}

/// 这个名字这个月花了多少：消费类型、交易类型、「消费」「支出」，否则在消费的对方和说明里找这个词
fn spent(vs: &[View], month: &str, name: &str) -> i64 {
    let inm = vs.iter().filter(|v| v.counted && v.b.time.starts_with(month));
    match name {
        "支出" | "花钱" | "总支出" => inm.map(|v| v.out).sum(),
        "消费" => inm.filter(|v| v.ttype == "消费").map(|v| v.out).sum(),
        n if CTYPES.contains(&n) => inm.filter(|v| v.ttype == "消费" && v.ctype == n).map(|v| v.out).sum(),
        n if TTYPES.contains(&n) => inm.filter(|v| v.ttype == n).map(|v| v.out).sum(),
        n => inm.filter(|v| v.ttype == "消费" && (v.b.counterparty.contains(n) || v.b.item.contains(n))).map(|v| v.out).sum(),
    }
}

fn budgets(vs: &[View], month: &str) -> Vec<Value> {
    crate::data::get_rules()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|r| r["enabled"] != false)
        .filter_map(|r| {
            let text = r["text"].as_str()?.to_string();
            let (name, limit) = parse_budget(&text)?;
            let s = spent(vs, month, &name);
            Some(json!({"text": text, "name": name, "limit": limit, "spent": s, "ok": s <= limit}))
        })
        .collect()
}

/// 规矩对照（日总结用）：是预算就算这个月到这天花了多少；不是预算返回 None
pub fn budget_check(text: &str, date: chrono::NaiveDate) -> Option<(String, Option<bool>)> {
    let (name, limit) = parse_budget(text)?;
    let c = ctx();
    let vs = c.views();
    let month = date.format("%Y-%m").to_string();
    let upto = date.format("%Y-%m-%d").to_string();
    let vs: Vec<View> = vs.into_iter().filter(|v| v.b.time[..10] <= *upto).collect();
    let s = spent(&vs, &month, &name);
    Some((format!("{name}这个月到今天 {} / 上限 {}", money(s), money(limit)), Some(s <= limit)))
}

/// 日总结的一行：这一天的钱（没有账单是 None）
pub fn day_line(date: chrono::NaiveDate) -> Option<String> {
    let c = ctx();
    let d = date.format("%Y-%m-%d").to_string();
    let vs: Vec<View> = c.views().into_iter().filter(|v| v.counted && v.b.time.starts_with(&d)).collect();
    if vs.is_empty() {
        return None;
    }
    let s = sum_of(vs.iter());
    let big = vs.iter().filter(|v| v.out > 0).max_by_key(|v| v.out);
    let mut by: BTreeMap<&str, i64> = BTreeMap::new();
    for v in vs.iter().filter(|v| v.out > 0) {
        *by.entry(v.ttype.as_str()).or_default() += v.out;
    }
    let parts: Vec<String> = by.iter().map(|(k, f)| format!("{k} {}", yuan(*f))).collect();
    Some(format!(
        "钱（微信、支付宝账单）：支出 {} 元（{}），收入 {} 元{}",
        yuan(s.out),
        parts.join("、"),
        yuan(s.inc),
        big.map(|v| format!("；最大一笔 {} {} 元", v.who(), yuan(v.out))).unwrap_or_default()
    ))
}

fn insight_path(month: &str) -> PathBuf {
    dir().join("insights").join(format!("{month}.md"))
}

/// 概览：一个月（或一整年）的读数、按交易类型和消费类型拆、近 12 个月趋势、每天、商户、人、账户、固定扣费、深夜消费、提醒、预算、AI 总结
#[tauri::command(async)]
pub fn money_overview(month: Option<String>) -> Value {
    let c = ctx();
    let vs = c.views();
    overview(&vs, month)
}

fn overview(vs: &[View], month: Option<String>) -> Value {
    let months = months_of(vs);
    let period = month.filter(|m| m.len() == 7 || m.len() == 4).or_else(|| months.first().cloned()).unwrap_or_else(this_month);
    let is_month = period.len() == 7;
    let first_month = months.last().cloned().unwrap_or_default();
    let inp: Vec<&View> = vs.iter().filter(|v| v.b.time.starts_with(&period)).collect();
    let tot = sum_of(inp.iter().copied());
    let refund: i64 = inp.iter().filter(|v| v.counted).map(|v| v.refund).sum();
    // 前三个月（只算有账单以后的月份）；这个月的账单只到月中时，和前三个月的同期（每月 1 日到同一天）比
    let prev: Vec<String> = if is_month { (1..=3).map(|k| month_add(&period, -k)).filter(|m| *m >= first_month).collect() } else { vec![] };
    let latest = vs.iter().map(|v| &v.b.time[..10]).max().unwrap_or("").to_string();
    let upto = (is_month && latest.starts_with(&period) && latest < month_end(&period)).then(|| latest.clone());
    let cut = upto.as_ref().map(|d| d[8..10].to_string());
    let base = if upto.is_some() { "前三个月同期平均" } else { "前三个月平均" };
    let pv = |m: &str| -> Vec<&View> { vs.iter().filter(|v| v.b.time.starts_with(m)).collect() };
    let pc = |m: &str| -> Vec<&View> { vs.iter().filter(|v| v.b.time.starts_with(m) && cut.as_deref().map_or(true, |c| &v.b.time[8..10] <= c)).collect() };
    let prev_sums: Vec<Sum> = prev.iter().map(|m| sum_of(pc(m).into_iter())).collect();
    let avg = |f: fn(&Sum) -> i64| -> Option<i64> { (!prev_sums.is_empty()).then(|| prev_sums.iter().map(f).sum::<i64>() / prev_sums.len() as i64) };

    // 按交易类型
    let mut by_t: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for v in inp.iter().filter(|v| v.counted) {
        let e = by_t.entry(v.ttype.clone()).or_default();
        e.0 += v.out;
        e.1 += v.inc;
        e.2 += 1;
    }
    let mut by_ttype: Vec<Value> = by_t.into_iter().map(|(k, (o, i, n))| json!({"name": k, "out": o, "in": i, "n": n})).collect();
    by_ttype.sort_by_key(|x| -(x["out"].as_i64().unwrap_or(0) + x["in"].as_i64().unwrap_or(0)));

    // 近 12 个月：月份时是到这个月为止的 12 个月，整年时是这一年的 12 个月
    let end = if is_month { period.clone() } else { format!("{period}-12") };
    let trend_months: Vec<String> = (0..12).rev().map(|k| month_add(&end, -k)).collect();

    // 按消费类型（和前三个月平均比，带近 12 个月每月多少）
    let cmap = |rows: &[&View]| -> HashMap<String, (i64, i64)> {
        let mut m: HashMap<String, (i64, i64)> = HashMap::new();
        for v in rows.iter().filter(|v| v.counted && v.ttype == "消费") {
            let e = m.entry(v.ctype.clone()).or_default();
            e.0 += v.out;
            e.1 += 1;
        }
        m
    };
    let cur = cmap(&inp);
    let prev_c: Vec<HashMap<String, (i64, i64)>> = prev.iter().map(|m| cmap(&pc(m))).collect();
    let mut by_ctype: Vec<Value> = cur
        .iter()
        .filter(|(_, (f, _))| *f > 0)
        .map(|(k, (f, n))| {
            let a = (!prev_c.is_empty()).then(|| prev_c.iter().map(|m| m.get(k).map(|x| x.0).unwrap_or(0)).sum::<i64>() / prev_c.len() as i64);
            let change = a.filter(|a| *a > 0).map(|a| (*f - a) as f64 / a as f64);
            let series: Vec<i64> = trend_months.iter().map(|m| vs.iter().filter(|v| v.counted && v.ttype == "消费" && v.ctype == *k && v.b.time.starts_with(m.as_str())).map(|v| v.out).sum()).collect();
            json!({"name": k, "fen": f, "n": n, "avg3": a, "change": change, "share": if tot.consume > 0 { *f as f64 / tot.consume as f64 } else { 0.0 }, "series": series})
        })
        .collect();
    by_ctype.sort_by_key(|x| -x["fen"].as_i64().unwrap_or(0));

    // 每月、每天的消费按消费类型拆（图上堆叠着画）
    let by_c = |rows: &[&View]| -> serde_json::Map<String, Value> { cmap(rows).into_iter().filter(|(_, (f, _))| *f > 0).map(|(k, (f, _))| (k, json!(f))).collect() };
    let trend: Vec<Value> = trend_months
        .iter()
        .map(|m| {
            let rows = pv(m);
            let s = sum_of(rows.iter().copied());
            json!({"month": m, "consume": s.consume, "out": s.out, "income": s.inc, "has": months.contains(m), "by": by_c(&rows)})
        })
        .collect();
    // 图上给消费类型上色：全部账单里消费最多的 5 类（「其他」、没分类的不算），换月、换图颜色不变；别的是灰色的「其余」
    let palette: Vec<String> = {
        let mut m: HashMap<&str, i64> = HashMap::new();
        for v in vs.iter().filter(|v| v.counted && v.ttype == "消费" && !v.ctype.is_empty() && v.ctype != "其他") {
            *m.entry(v.ctype.as_str()).or_default() += v.out;
        }
        let mut xs: Vec<(&str, i64)> = m.into_iter().filter(|x| x.1 > 0).collect();
        xs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        xs.into_iter().take(5).map(|x| x.0.to_string()).collect()
    };

    // 每天（月份时）
    let days: Vec<Value> = if is_month {
        let mut d: BTreeMap<String, Vec<&View>> = BTreeMap::new();
        for v in inp.iter().filter(|v| v.counted) {
            d.entry(v.b.time[..10].to_string()).or_default().push(v);
        }
        d.into_iter()
            .map(|(k, rows)| {
                let s = sum_of(rows.iter().copied());
                json!({"date": k, "consume": s.consume, "out": s.out, "by": by_c(&rows)})
            })
            .collect()
    } else {
        vec![]
    };

    // 商户（消费的对方）、人（转账、红包、亲友代付的来往）、账户（钱从哪出）
    let mut merch: HashMap<String, (i64, i64, String)> = HashMap::new();
    let mut people: HashMap<String, (i64, i64, i64)> = HashMap::new();
    let mut acct: HashMap<String, (i64, i64, bool)> = HashMap::new();
    for v in inp.iter().filter(|v| v.counted) {
        if v.ttype == "消费" {
            let e = merch.entry(v.who()).or_insert((0, 0, v.ctype.clone()));
            e.0 += v.out;
            e.1 += 1;
        }
        if TO_PEOPLE.contains(&v.ttype.as_str()) {
            let e = people.entry(v.who()).or_default();
            e.0 += v.out;
            e.1 += v.inc;
            e.2 += 1;
        }
        if v.out > 0 {
            let e = acct.entry(if v.account.is_empty() { "账单上没写".to_string() } else { v.account.clone() }).or_insert((0, 0, v.credit));
            e.0 += v.out;
            e.1 += 1;
        }
    }
    let mut merchants: Vec<(String, (i64, i64, String))> = merch.into_iter().collect();
    merchants.sort_by_key(|x| -x.1 .0);
    let mut people: Vec<(String, (i64, i64, i64))> = people.into_iter().collect();
    people.sort_by_key(|x| -(x.1 .0 + x.1 .1));
    let mut accounts: Vec<(String, (i64, i64, bool))> = acct.into_iter().collect();
    accounts.sort_by_key(|x| -x.1 .0);
    let credit_of = |rows: &[&View]| -> Option<f64> {
        let known: Vec<&&View> = rows.iter().filter(|v| v.counted && v.ttype == "消费" && !v.account.is_empty()).collect();
        let all: i64 = known.iter().map(|v| v.out).sum();
        (all > 0).then(|| known.iter().filter(|v| v.credit).map(|v| v.out).sum::<i64>() as f64 / all as f64)
    };
    let credit = credit_of(&inp);
    let credit_prev = {
        let rows: Vec<&View> = prev.iter().flat_map(|m| pv(m)).collect();
        credit_of(&rows)
    };

    // 深夜消费：23:00–5:00
    let late = |rows: &[&View]| -> (i64, i64) {
        let l: Vec<&&View> = rows.iter().filter(|v| v.counted && v.ttype == "消费" && { let h: u32 = v.b.time[11..13].parse().unwrap_or(12); !(5..23).contains(&h) }).collect();
        (l.len() as i64, l.iter().map(|v| v.out).sum())
    };
    let (late_n, late_fen) = late(&inp);
    let late_prev = (!prev.is_empty()).then(|| prev.iter().map(|m| late(&pc(m)).0).sum::<i64>() / prev.len() as i64);

    // 每月固定扣费：同一个对方的消费，近 6 个月里至少 3 个月有，每月金额稳（波动 25% 以内）、每月不超过 3 笔
    let anchor = if is_month { period.clone() } else { format!("{period}-12").min(this_month()) };
    let win: Vec<String> = (0..6).map(|k| month_add(&anchor, -k)).collect();
    let mut by_who: HashMap<String, BTreeMap<String, (i64, i64)>> = HashMap::new();
    let mut ctype_of: HashMap<String, String> = HashMap::new();
    for v in vs.iter().filter(|v| v.counted && v.ttype == "消费" && v.out > 0 && !v.b.counterparty.is_empty()) {
        let e = by_who.entry(v.who()).or_default().entry(v.b.time[..7].to_string()).or_default();
        e.0 += v.out;
        e.1 += 1;
        ctype_of.entry(v.who()).or_insert(v.ctype.clone());
    }
    let mut recurring = vec![];
    for (who, ms) in &by_who {
        let inwin: Vec<(&String, &(i64, i64))> = ms.iter().filter(|(m, _)| win.contains(m)).collect();
        if inwin.len() < 2 || inwin.iter().any(|(_, (_, n))| *n > 3) {
            continue;
        }
        let vals: Vec<f64> = inwin.iter().map(|(_, (f, _))| *f as f64).collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let sd = (vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt();
        let first_seen = ms.keys().next().cloned().unwrap_or_default();
        let (last_m, last) = inwin.last().map(|(m, (f, _))| ((*m).clone(), *f)).unwrap_or_default();
        let is_new = inwin.len() == 2 && first_seen >= month_add(&anchor, -2) && sd / mean <= 0.1;
        if !(inwin.len() >= 3 && sd / mean <= 0.25) && !is_new {
            continue;
        }
        let before: Vec<f64> = inwin.iter().rev().skip(1).map(|(_, (f, _))| *f as f64).collect();
        let base = if before.is_empty() { mean } else { before.iter().sum::<f64>() / before.len() as f64 };
        let rise = last_m == anchor && last as f64 > base * 1.2 && last as f64 - base >= 1000.0;
        recurring.push(json!({"name": who, "ctype": ctype_of.get(who), "months": inwin.len(), "avg": mean.round() as i64, "last": last, "last_month": last_m, "rise": rise, "new": is_new}));
    }
    recurring.sort_by_key(|x| -x["avg"].as_i64().unwrap_or(0));

    let unclassified = vs.iter().filter(|v| v.unsorted()).count();
    let budgets = if is_month { budgets(vs, &period) } else { vec![] };

    // 提醒：只看一个月的，和前三个月平均比
    let mut alerts: Vec<Value> = vec![];
    if is_month {
        let pct = |x: f64| format!("{:.0}%", x.abs() * 100.0);
        if let Some(a) = avg(|s| s.consume).filter(|a| *a > 0) {
            let ch = (tot.consume - a) as f64 / a as f64;
            if ch.abs() >= 0.3 {
                alerts.push(json!({"level": if ch > 0.0 { "warn" } else { "info" }, "text": format!("这个月消费 {}，{}（{}）", money(tot.consume), versus(tot.consume, a, base), money(a))}));
            }
        }
        // 消费类型：挑比前三个月平均多得最多的 3 类（多 50% 以上、多出 300 元以上）；前三个月没有的新类型 500 元以上提一句
        let mut ups: Vec<(i64, &Value)> = by_ctype
            .iter()
            .filter(|x| !x["name"].as_str().unwrap_or("").is_empty())
            .filter_map(|x| {
                let (f, a) = (x["fen"].as_i64()?, x["avg3"].as_i64()?);
                (a > 0 && f as f64 >= a as f64 * 1.5 && f - a >= 30000).then_some((f - a, x))
            })
            .collect();
        ups.sort_by_key(|x| -x.0);
        for (_, x) in ups.iter().take(3) {
            let (f, a) = (x["fen"].as_i64().unwrap_or(0), x["avg3"].as_i64().unwrap_or(1));
            alerts.push(json!({"level": "warn", "text": format!("{} {}，{}", x["name"].as_str().unwrap_or(""), money(f), versus(f, a, base)), "ctype": x["name"]}));
        }
        for x in by_ctype.iter().filter(|x| x["avg3"].as_i64() == Some(0) && x["fen"].as_i64().unwrap_or(0) >= 50000 && !x["name"].as_str().unwrap_or("").is_empty()).take(2) {
            alerts.push(json!({"level": "info", "text": format!("{} {}，前三个月没有", x["name"].as_str().unwrap_or(""), money(x["fen"].as_i64().unwrap_or(0))), "ctype": x["name"]}));
        }
        let unc: Vec<&&View> = inp.iter().filter(|v| v.unsorted()).collect();
        if !unc.is_empty() {
            alerts.push(json!({"level": "info", "text": format!("{} 笔消费没分好类，共 {}", unc.len(), money(unc.iter().map(|v| v.out).sum())), "action": "classify"}));
        }
        // 大额：比近 6 个月消费的 95% 分位高，且不少于 2000 元
        let mut recent: Vec<i64> = vs.iter().filter(|v| v.counted && v.ttype == "消费" && win.iter().any(|m| v.b.time.starts_with(m))).map(|v| v.out).collect();
        recent.sort();
        let p95 = recent.get(recent.len().saturating_sub(1) * 95 / 100).copied().unwrap_or(0).max(200000);
        let mut big: Vec<&&View> = inp.iter().filter(|v| v.counted && v.ttype == "消费" && v.out >= p95).collect();
        big.sort_by_key(|v| -v.out);
        for v in big.iter().take(3) {
            alerts.push(json!({"level": "info", "text": format!("大额：{} {}（{}）", v.who(), money(v.out), &v.b.time[5..10]), "id": v.b.id}));
        }
        for r in &recurring {
            let name = r["name"].as_str().unwrap_or("");
            if r["rise"] == true {
                alerts.push(json!({"level": "warn", "text": format!("固定扣费涨了：{name} 这个月 {}，之前每月约 {}", money(r["last"].as_i64().unwrap_or(0)), money(r["avg"].as_i64().unwrap_or(0))), "q": name}));
            } else if r["new"] == true {
                alerts.push(json!({"level": "info", "text": format!("新出现的固定扣费：{name}，每月约 {}", money(r["avg"].as_i64().unwrap_or(0))), "q": name}));
            }
        }
        if late_n >= 3 && late_prev.map(|p| late_n as f64 >= p as f64 * 1.5).unwrap_or(true) {
            alerts.push(json!({"level": "warn", "text": format!("23 点以后消费 {late_n} 笔、{}{}", money(late_fen), late_prev.map(|p| if upto.is_some() { format!("，前三个月同期平均 {p} 笔") } else { format!("，前三个月平均每月 {p} 笔") }).unwrap_or_default())}));
        }
        if let (Some(c1), Some(c0)) = (credit, credit_prev) {
            if c1 >= 0.3 && c1 - c0 >= 0.1 {
                alerts.push(json!({"level": "warn", "text": format!("花呗、信用卡付的消费占 {}，前三个月是 {}", pct(c1), pct(c0))}));
            }
        }
        for b in &budgets {
            let (s, l) = (b["spent"].as_i64().unwrap_or(0), b["limit"].as_i64().unwrap_or(1));
            if s > l {
                alerts.push(json!({"level": "warn", "text": format!("预算超了：{} 已花 {} / {}", b["name"].as_str().unwrap_or(""), money(s), money(l))}));
            } else if s as f64 >= l as f64 * 0.8 && period == this_month() {
                alerts.push(json!({"level": "warn", "text": format!("预算快到了：{} 已花 {} / {}", b["name"].as_str().unwrap_or(""), money(s), money(l))}));
            }
        }
    }
    let insight = is_month.then(|| fs::read_to_string(insight_path(&period)).ok()).flatten();

    json!({
        "period": period, "is_month": is_month, "months": months, "upto": upto,
        "totals": {"out": tot.out, "in": tot.inc, "net": tot.inc - tot.out, "rate": (tot.inc > 0).then(|| (tot.inc - tot.out) as f64 / tot.inc as f64),
                   "consume": tot.consume, "n": tot.n, "refund": refund},
        "prev": if prev_sums.is_empty() { Value::Null } else { json!({"months": prev, "out": avg(|s| s.out), "in": avg(|s| s.inc), "consume": avg(|s| s.consume)}) },
        "by_ttype": by_ttype,
        "by_ctype": by_ctype,
        "trend": trend,
        "palette": palette,
        "days": days,
        "merchants": merchants.into_iter().take(15).map(|(k, (f, n, c))| json!({"name": k, "fen": f, "n": n, "ctype": c})).collect::<Vec<_>>(),
        "people": people.into_iter().take(30).map(|(k, (o, i, n))| json!({"name": k, "out": o, "in": i, "n": n})).collect::<Vec<_>>(),
        "accounts": accounts.into_iter().map(|(k, (f, n, c))| json!({"name": k, "fen": f, "n": n, "credit": c})).collect::<Vec<_>>(),
        "credit": credit, "credit_prev": credit_prev,
        "late": {"n": late_n, "fen": late_fen, "prev_n": late_prev},
        "recurring": recurring,
        "alerts": alerts,
        "budgets": budgets,
        "unclassified": unclassified,
        "insight": insight,
    })
}

const INSIGHT_PROMPT: &str = "你是用户的理财顾问，帮用户看懂这个月的钱。材料是这个月微信、支付宝账单的汇总和代码算好的线索，不是逐笔流水。
页面上的图表已经画出了支出、消费、收入、每类花了多少、近 12 个月的走势和每天花了多少，用户看得到这些数。所以：
- 不复述合计，不逐类罗列金额，不把材料改写成句子。
- 写图表看不出来的：变化是怎么来的（多买了几次还是买得更贵，是一两笔大额还是日常在涨）、钱花在什么时候、和作息的关系、哪些是一次性的哪些会反复出现、值得警惕或值得保持的地方。
- 每条先用一句话下结论并加粗，后面一两句给证据和它意味着什么；证据只引用最关键的一两个数：一万以上写「11.4 万元」，以下写整数元，或者百分比、倍数。
- 支出合计按账单的收/支算，里面有转账、红包这些不是消费；说花钱习惯看消费和消费类型，转账、红包按交易类型说，不要另起「转给人」之类的口径。转账金额大但材料看不出用途的，不猜用途。
- 一次性的大额提一句就行，不据此建议设预算；建议针对会反复出现的花销。
- 用第二人称「你」，平实、具体，不夸、不说教，不编造材料里没有的事。数字和汉字之间空一格（7 笔、2000 元）。
用 Markdown，两个小标题：
## 发现
3～4 条，按重要性排，每条一行，用「- 」开头。
## 建议
1～3 条能直接做的事（给某类设每月上限、停掉某个固定扣费、某个时段别下单），每条一行，用「- 」开头。";

/// 给 AI 的线索：图上看不出来、要算的（这个月和前几个月同期比）
fn clues(vs: &[View], month: &str, prev: &[String], upto: Option<&str>) -> String {
    let cut = upto.map(|d| &d[8..10]);
    let consume = |m: &str| -> Vec<&View> {
        vs.iter().filter(|v| v.counted && v.ttype == "消费" && v.out > 0 && v.b.time.starts_with(m) && cut.map_or(true, |c| &v.b.time[8..10] <= c)).collect()
    };
    let cur = consume(month);
    let before: Vec<Vec<&View>> = prev.iter().map(|m| consume(m)).collect();
    let k = before.len().max(1) as i64;
    let total = |xs: &[&View]| xs.iter().map(|v| v.out).sum::<i64>();
    let mut out = String::new();
    // 笔数和每笔多少：多花是买得多还是买得贵
    let (n, f) = (cur.len() as i64, total(&cur));
    let (pn, pf) = (before.iter().map(|x| x.len() as i64).sum::<i64>() / k, before.iter().map(|x| total(x)).sum::<i64>() / k);
    out += &format!("- 消费 {n} 笔，平均每笔 {} 元；前几个月同期平均每月 {pn} 笔，每笔 {} 元\n", f / 100 / n.max(1), pf / 100 / pn.max(1));
    // 日常（单笔 500 元以下）和大额（2000 元以上）分开
    let small = |xs: &[&View]| -> (i64, i64) { let s: Vec<&&View> = xs.iter().filter(|v| v.out < 50000).collect(); (s.len() as i64, s.iter().map(|v| v.out).sum()) };
    let (sn, sf) = small(&cur);
    let (psn, psf) = before.iter().map(|x| small(x)).fold((0, 0), |a, x| (a.0 + x.0, a.1 + x.1));
    out += &format!("- 日常消费（单笔 500 元以下）{sn} 笔 {} 元；前几个月同期平均 {} 笔 {} 元\n", sf / 100, psn / k, psf / k / 100);
    let mut big: Vec<&&View> = cur.iter().filter(|v| v.out >= 200000).collect();
    big.sort_by_key(|v| -v.out);
    if !big.is_empty() {
        let names: Vec<String> = big.iter().take(4).map(|v| format!("{}（{}）{} 元", v.who(), if v.ctype.is_empty() { "未分类" } else { &v.ctype }, v.out / 100)).collect();
        out += &format!("- 2000 元以上的 {} 笔，共 {} 元，占消费的 {}%：{}\n", big.len(), total(&big.iter().map(|v| **v).collect::<Vec<_>>()) / 100, total(&big.iter().map(|v| **v).collect::<Vec<_>>()) * 100 / f.max(1), names.join("、"));
    }
    // 工作日和周末：每天平均花多少（只算日常的）
    use chrono::Datelike;
    let last_day = upto.map(String::from).unwrap_or_else(|| month_end(month).min(chrono::Local::now().format("%Y-%m-%d").to_string()));
    if let (Ok(a), Ok(z)) = (chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d"), chrono::NaiveDate::parse_from_str(&last_day, "%Y-%m-%d")) {
        let (mut wd, mut we) = (0i64, 0i64);
        let mut d = a;
        while d <= z {
            if d.weekday().number_from_monday() >= 6 { we += 1 } else { wd += 1 }
            d += chrono::Duration::days(1);
        }
        let is_we = |v: &View| chrono::NaiveDate::parse_from_str(&v.b.time[..10], "%Y-%m-%d").map(|d| d.weekday().number_from_monday() >= 6).unwrap_or(false);
        let fw: i64 = cur.iter().filter(|v| v.out < 50000 && !is_we(v)).map(|v| v.out).sum();
        let fe: i64 = cur.iter().filter(|v| v.out < 50000 && is_we(v)).map(|v| v.out).sum();
        if wd > 0 && we > 0 {
            out += &format!("- 日常消费每天平均：工作日 {} 元（{wd} 天），周末 {} 元（{we} 天）\n", fw / wd / 100, fe / we / 100);
        }
    }
    // 什么时候花的
    let slots = [("早上 5–11 点", 5, 11), ("中午 11–14 点", 11, 14), ("下午 14–18 点", 14, 18), ("晚上 18–23 点", 18, 23)];
    let hour = |v: &View| v.b.time[11..13].parse::<u32>().unwrap_or(12);
    let mut parts: Vec<String> = slots.iter().map(|(name, a, z)| {
        let xs: Vec<&&View> = cur.iter().filter(|v| (*a..*z).contains(&hour(v))).collect();
        format!("{name} {} 笔 {} 元", xs.len(), xs.iter().map(|v| v.out).sum::<i64>() / 100)
    }).collect();
    let late: Vec<&&View> = cur.iter().filter(|v| !(5..23).contains(&hour(v))).collect();
    let plate = before.iter().map(|x| x.iter().filter(|v| !(5..23).contains(&hour(v))).count() as i64).sum::<i64>() / k;
    parts.push(format!("深夜 23–5 点 {} 笔 {} 元（前几个月同期平均每月 {plate} 笔）", late.len(), late.iter().map(|v| v.out).sum::<i64>() / 100));
    out += &format!("- 什么时候花的：{}\n", parts.join("；"));
    let mut late_c: HashMap<String, i64> = HashMap::new();
    for v in &late {
        *late_c.entry(if v.ctype.is_empty() { "未分类".into() } else { v.ctype.clone() }).or_default() += 1;
    }
    let mut late_c: Vec<(String, i64)> = late_c.into_iter().collect();
    late_c.sort_by_key(|x| -x.1);
    if !late_c.is_empty() {
        out += &format!("- 深夜消费多是：{}\n", late_c.iter().take(3).map(|(c, n)| format!("{c} {n} 笔")).collect::<Vec<_>>().join("、"));
    }
    // 深夜下单的那几晚，和这个月别的晚上比熬得多晚（Beayy 的作息：一晚＝当天 21 点以后＋次日 0–5 点在电脑、手机上的时间）
    if !late.is_empty() {
        let first = format!("{month}-01");
        if let Ok(r) = crate::data::get_range(first, last_day.clone()) {
            let night: HashMap<String, i64> = r["days"].as_array().into_iter().flatten().filter_map(|d| Some((d["date"].as_str()?.to_string(), d["night_sec"].as_i64()?))).collect();
            let of = |v: &View| -> String {
                let d = chrono::NaiveDate::parse_from_str(&v.b.time[..10], "%Y-%m-%d").unwrap_or_default();
                if hour(v) < 5 { (d - chrono::Duration::days(1)).format("%Y-%m-%d").to_string() } else { d.format("%Y-%m-%d").to_string() }
            };
            let nights: std::collections::HashSet<String> = late.iter().map(|v| of(v)).filter(|d| night.contains_key(d)).collect();
            let rest: Vec<i64> = night.iter().filter(|(d, _)| !nights.contains(*d)).map(|(_, s)| *s).collect();
            if !nights.is_empty() && !rest.is_empty() {
                let a = nights.iter().map(|d| night[d]).sum::<i64>() as f64 / nights.len() as f64 / 3600.0;
                let b = rest.iter().sum::<i64>() as f64 / rest.len() as f64 / 3600.0;
                out += &format!("- 深夜下单的 {} 个晚上，21 点以后平均还在电脑、手机上 {a:.1} 小时；这个月其他晚上平均 {b:.1} 小时\n", nights.len());
            }
        }
    }
    // 钱集中在哪几家；这个月第一次出现的商户
    let mut by_who: HashMap<String, i64> = HashMap::new();
    for v in &cur {
        *by_who.entry(v.who()).or_default() += v.out;
    }
    let mut top: Vec<(String, i64)> = by_who.into_iter().collect();
    top.sort_by_key(|x| -x.1);
    let top3: i64 = top.iter().take(3).map(|x| x.1).sum();
    out += &format!("- 消费最多的 3 家占消费的 {}%：{}\n", top3 * 100 / f.max(1), top.iter().take(3).map(|x| x.0.clone()).collect::<Vec<_>>().join("、"));
    let seen: std::collections::HashSet<String> = vs.iter().filter(|v| v.counted && v.ttype == "消费" && v.b.time[..7] < *month).map(|v| v.who()).collect();
    let mut fresh: Vec<&(String, i64)> = top.iter().filter(|x| !seen.contains(&x.0)).collect();
    fresh.sort_by_key(|x| -x.1);
    if !fresh.is_empty() && !seen.is_empty() {
        out += &format!("- 这个月第一次付钱的商户 {} 家，共 {} 元，最多的是 {}\n", fresh.len(), fresh.iter().map(|x| x.1).sum::<i64>() / 100, fresh.iter().take(3).map(|x| x.0.clone()).collect::<Vec<_>>().join("、"));
    }
    // 连着涨的类：这个月 > 上个月 > 上上个月（都按同期）
    if before.len() >= 2 {
        let by = |xs: &[&View]| -> HashMap<String, i64> { let mut m = HashMap::new(); for v in xs { *m.entry(v.ctype.clone()).or_default() += v.out; } m };
        let (c0, c1, c2) = (by(&cur), by(&before[0]), by(&before[1]));
        let mut ups: Vec<String> = c0.iter().filter(|(k, f)| !k.is_empty() && c1.get(*k).map_or(false, |b| *f > b && c2.get(*k).map_or(false, |c| b > c && *c > 0))).map(|(k, f)| format!("{k}（{} → {} → {} 元）", c2[k] / 100, c1[k] / 100, f / 100)).collect();
        ups.sort();
        if !ups.is_empty() {
            out += &format!("- 连着两个月在涨的：{}\n", ups.join("、"));
        }
    }
    out
}

/// 给 AI 的材料：这个月的汇总和线索（调试：beayy --dump money_material YYYY-MM）
pub fn insight_material(month: &str) -> String {
    let month = month.to_string();
    let c = ctx();
    let vs = c.views();
    let o = overview(&vs, Some(month.clone()));
    let t = &o["totals"];
    let f = |v: &Value| yuan(v.as_i64().unwrap_or(0));
    let mut user = format!("月份：{month}\n\n合计：支出 {} 元（其中消费 {}），收入 {} 元，结余 {} 元，{} 笔；退款已扣 {} 元\n", f(&t["out"]), f(&t["consume"]), f(&t["in"]), f(&t["net"]), t["n"], f(&t["refund"]));
    // 账单只到月中：说明到哪天，比的是前三个月的同期
    let base = match o["upto"].as_str() {
        Some(d) => {
            user += &format!("这个月的账单只到 {d}，下面和以前比的都是前三个月同期（每月 1 日到 {} 日）\n", d[8..10].trim_start_matches('0'));
            "前三个月同期平均"
        }
        None => "前三个月平均",
    };
    if !o["prev"].is_null() {
        user += &format!("{base}：支出 {}，消费 {}，收入 {}\n", f(&o["prev"]["out"]), f(&o["prev"]["consume"]), f(&o["prev"]["in"]));
    }
    let lines = |k: &str, fmt: &dyn Fn(&Value) -> String| -> String { o[k].as_array().cloned().unwrap_or_default().iter().map(|x| format!("- {}\n", fmt(x))).collect() };
    user += "\n按交易类型：\n";
    user += &lines("by_ttype", &|x| format!("{} 支出 {} 收入 {}（{} 笔）", x["name"].as_str().unwrap_or(""), f(&x["out"]), f(&x["in"]), x["n"]));
    user += &format!("\n消费按类型（括号里是{base}）：\n");
    user += &lines("by_ctype", &|x| format!("{} {} 元，{} 笔（{}）", x["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("未分类"), f(&x["fen"]), x["n"], x["avg3"].as_i64().map(|a| yuan(a)).unwrap_or("没有".into())));
    let prev: Vec<String> = o["prev"]["months"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(String::from)).collect();
    user += "\n代码算好的线索：\n";
    user += &clues(&vs, &month, &prev, o["upto"].as_str());
    user += "\n转账、红包来往最多的人（给出／收到）：\n";
    user += &o["people"].as_array().cloned().unwrap_or_default().iter().take(3).map(|x| format!("- {} {} / {} 元，{} 笔\n", x["name"].as_str().unwrap_or(""), f(&x["out"]), f(&x["in"]), x["n"])).collect::<String>();
    user += "\n付款账户：\n";
    user += &lines("accounts", &|x| format!("{}{} {} 元", x["name"].as_str().unwrap_or(""), if x["credit"] == true { "（信用）" } else { "" }, f(&x["fen"])));
    if let (Some(a), Some(b)) = (o["credit"].as_f64(), o["credit_prev"].as_f64()) {
        user += &format!("消费里花呗、信用卡付的占 {:.0}%，前三个月 {:.0}%\n", a * 100.0, b * 100.0);
    }
    user += "\n固定扣费：\n";
    user += &lines("recurring", &|x| format!("{} 每月约 {} 元，{} 个月{}{}", x["name"].as_str().unwrap_or(""), f(&x["avg"]), x["months"], if x["rise"] == true { "，这个月涨了" } else { "" }, if x["new"] == true { "，新出现" } else { "" }));
    user += "\n提醒：\n";
    user += &lines("alerts", &|x| x["text"].as_str().unwrap_or("").to_string());
    user += "\n预算：\n";
    user += &lines("budgets", &|x| format!("{}：已花 {} 元", x["text"].as_str().unwrap_or(""), f(&x["spent"])));
    user
}

/// AI 写这个月的「钱」总结（材料只给汇总和线索），存 bills/insights/YYYY-MM.md
#[tauri::command(async)]
pub fn generate_money_insight(month: String) -> Result<String, String> {
    if month.len() != 7 {
        return Err("只写一个月的".into());
    }
    let user = insight_material(&month);
    let sm = beayy_summary::Summarizer::new(default_root());
    let system = if sm.out_lang() == "en" { format!("{INSIGHT_PROMPT}\nWrite everything in English, with headings: Findings / Suggestions.") } else { INSIGHT_PROMPT.to_string() };
    let md = sm.ask_text("money", &system, &user, 0.4)?;
    let md = md.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim().to_string();
    if md.is_empty() {
        return Err("模型没写出东西".into());
    }
    let p = insight_path(&month);
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&p, &md).map_err(|e| e.to_string())?;
    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bill(src: &str, dir: &str, fen: i64, kind: &str, cat: &str, who: &str, item: &str) -> Bill {
        let mut raw = BTreeMap::new();
        if !cat.is_empty() {
            raw.insert("交易分类".to_string(), cat.to_string());
        }
        Bill { id: format!("{src}:{who}{item}{fen}"), src: src.into(), time: "2026-10-01 12:00:00".into(), dir: dir.into(), fen, kind: kind.into(), counterparty: who.into(), item: item.into(), raw, ..Default::default() }
    }

    #[test]
    fn ttypes() {
        assert_eq!(ttype_auto(&bill("wechat", "out", 100, "商户消费", "", "面馆", "面")), "消费");
        assert_eq!(ttype_auto(&bill("wechat", "out", 100, "微信红包（群红包）", "", "群", "")), "红包");
        assert_eq!(ttype_auto(&bill("wechat", "in", 100, "转账-退款", "", "某人", "")), "退款");
        assert_eq!(ttype_auto(&bill("wechat", "none", 100, "零钱提现", "", "银行", "")), "账户存取");
        assert_eq!(ttype_auto(&bill("wechat", "none", 100, "转入零钱通-来自零钱", "", "", "")), "理财");
        assert_eq!(ttype_auto(&bill("wechat", "out", 100, "亲属卡交易", "", "", "")), "亲友代付");
        assert_eq!(ttype_auto(&bill("alipay", "out", 100, "", "转账红包", "某人", "发口令红包")), "红包");
        assert_eq!(ttype_auto(&bill("alipay", "out", 100, "", "转账红包", "某人", "转账")), "转账");
        assert_eq!(ttype_auto(&bill("alipay", "none", 100, "", "信用借还", "花呗", "还款")), "还款");
        assert_eq!(ttype_auto(&bill("alipay", "out", 100, "", "餐饮美食", "店", "饭")), "消费");
        assert_eq!(ttype_auto(&bill("alipay", "in", 100, "", "收入", "公司", "工资")), "收入");
        assert_eq!(ttype_auto(&bill("wechat", "in", 100, "其他", "", "", "")), "收入");
        // 只有网页版
        assert_eq!(ttype_auto(&bill("alipay", "out", 100, "即时到账交易", "", "某人", "转账")), "转账");
        assert_eq!(ttype_auto(&bill("alipay", "out", 100, "即时到账交易", "", "店", "饭")), "消费");
    }

    #[test]
    fn person_names() {
        assert_eq!(person("小舟（周末别打扰） (舟舟)"), "小舟");
        assert_eq!(person("发给小舟 (舟舟)"), "小舟");
        assert_eq!(person("小舟(**舟)"), "小舟");
        assert_eq!(person("\u{7f} 阿岚"), "阿岚");
        assert_eq!(person("余额宝小荷包(旅行)"), "余额宝小荷包(旅行)");
        assert_eq!(person("（备注）"), "（备注）");
    }

    #[test]
    fn budget_parse() {
        assert_eq!(parse_budget("外卖每月不超过 1500"), Some(("外卖".into(), 150000)));
        assert_eq!(parse_budget("餐饮美食每月不超过3000元"), Some(("餐饮美食".into(), 300000)));
        assert_eq!(parse_budget("消费每月不超过 2 万"), Some(("消费".into(), 2000000)));
        assert_eq!(parse_budget("21:00 硬停"), None);
    }

    #[test]
    fn accounts_and_months() {
        assert_eq!(account_of("花呗&碰友日立减"), ("花呗".into(), true, "碰友日立减".into()));
        assert_eq!(account_of("建设银行储蓄卡(1234)"), ("建设银行储蓄卡(1234)".into(), false, String::new()));
        assert_eq!(month_add("2026-01", -1), "2025-12");
        assert_eq!(month_add("2026-11", 2), "2027-01");
        assert_eq!(money(432690), "4,326.90 元");
        assert_eq!(money(14276800), "14.3 万元");
        assert_eq!(money(5), "0.05 元");
        assert_eq!(versus(300, 100, "前三个月平均"), "是前三个月平均的 3.0 倍");
        assert_eq!(versus(150, 100, "前三个月同期平均"), "比前三个月同期平均多 50%");
        assert_eq!(month_end("2026-02"), "2026-02-28");
        assert_eq!(month_end("2026-12"), "2026-12-31");
    }
}
