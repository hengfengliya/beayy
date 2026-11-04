//! 收支：导入微信、支付宝导出的账单文件（两家都没有给个人读账单的接口），存在 `<数据根>/bills/bills.json`，按月汇总给「收支」页。
//! 认三种文件：微信的 xlsx（旧版是 csv，表头一样）；支付宝手机 app 导的「交易明细」csv、网页版导的「交易记录明细查询」csv（都是 GBK）。
//! - 同一笔按交易号认（支付宝两种格式的交易号是同一个），重复导入不重复记；以导出时间晚的那份为准
//!   （状态会从「等待确认收货」变成「交易成功」），晚的那份缺的字段从早的补。
//! - 支出按退款后的净额算：网页版有「成功退款」列；app 版的退款单独一行，交易号是「原单号_…」，挂回原单；
//!   「交易关闭」算全退；微信看原单的状态（「已全额退款」「已退款(￥6.38)」「对方已退还」）。退款行自己不算收入。
//! - 不计收支、中性交易（还花呗、转余额宝、提现）照样存，不算进收支。
//! 解析规则参考了 Personal-Bill（MIT，https://github.com/zhu123279/Personal-Bill）。

use beayy_core::data_root;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Bill {
    /// `alipay:<交易号>`、`wechat:<交易单号>`
    pub id: String,
    pub src: String,
    /// 账单上的本地时刻 `YYYY-MM-DD HH:MM:SS`
    pub time: String,
    /// out 支出、in 收入、none 不计收支
    pub dir: String,
    /// 金额（分）
    pub fen: i64,
    /// 这一行上写着的已退金额（分）
    #[serde(default)]
    pub refund_fen: i64,
    /// 是退款那一行（不算收入）
    #[serde(default)]
    pub is_refund: bool,
    /// 退款行对应的原单 id（支付宝能对上；微信对不上，空着）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// 账单上的交易类型（微信：商户消费、微信红包、转账……）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub counterparty: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// 商家订单号
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub order: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// 来自哪份账单的导出时刻（重复导入时晚的为准）
    #[serde(default)]
    pub export: String,
}

fn store_path() -> PathBuf {
    data_root().join("bills").join("bills.json")
}

pub fn load(p: &Path) -> BTreeMap<String, Bill> {
    let v: Vec<Bill> = fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    v.into_iter().map(|b| (b.id.clone(), b)).collect()
}

fn save(p: &Path, all: &BTreeMap<String, Bill>) -> Result<(), String> {
    let mut v: Vec<&Bill> = all.values().collect();
    v.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

// ---------- 读文件 ----------

/// 一行 csv（支付宝的字段一般不带引号，但带了也认）
fn csv_line(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' if q && it.peek() == Some(&'"') => {
                cur.push('"');
                it.next();
            }
            '"' => q = !q,
            ',' if !q => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn decode(bytes: &[u8]) -> String {
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GB18030.decode(b).0.into_owned(),
    }
}

fn xlsx_rows(bytes: Vec<u8>) -> Result<Vec<Vec<String>>, String> {
    use calamine::{Data, Reader, Xlsx};
    let mut wb: Xlsx<_> = Xlsx::new(std::io::Cursor::new(bytes)).map_err(|e| format!("打不开这个表格：{e}"))?;
    let range = wb.worksheet_range_at(0).ok_or("表格里没有工作表")?.map_err(|e| e.to_string())?;
    Ok(range
        .rows()
        .map(|r| {
            r.iter()
                .map(|c| match c {
                    Data::DateTime(d) => d.as_datetime().map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default(),
                    c => c.to_string(),
                })
                .collect()
        })
        .collect())
}

fn table(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "zip" {
        return Err("这是压缩包，先解压（密码在微信支付、支付宝发给你的消息里），再导入里面的文件".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("读不了：{e}"))?;
    if bytes.starts_with(b"PK") {
        return xlsx_rows(bytes);
    }
    Ok(decode(&bytes).lines().map(csv_line).collect())
}

fn clean(s: &str) -> String {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t == "/" { String::new() } else { t.to_string() }
}

/// 「¥1,234.50」→ 123450 分
fn fen(s: &str) -> Option<i64> {
    let t: String = s.chars().filter(|c| !matches!(c, '¥' | '￥' | ',' | ' ' | '\t')).collect();
    let v: f64 = t.trim_start_matches('-').parse().ok()?;
    Some((v * 100.0).round() as i64)
}

fn dir_of(s: &str) -> &'static str {
    match s {
        "支出" => "out",
        "收入" => "in",
        _ => "none",
    }
}

/// 一串文字里第一个 `YYYY-MM-DD HH:MM:SS`
fn datetime_in(s: &str) -> Option<String> {
    let re = regex::Regex::new(r"\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}").unwrap();
    re.find(s).map(|m| m.as_str().replace('T', " "))
}

/// 微信原单状态里的已退金额：「已全额退款」「对方已退还」＝全退，「已退款(￥6.38)」「已退款￥6.38」＝那个数
fn wechat_refund(status: &str, total: i64) -> i64 {
    if status.contains("全额退款") || status.contains("已退还") {
        return total;
    }
    let re = regex::Regex::new(r"退款\D*?([\d,]+(?:\.\d+)?)").unwrap();
    re.captures(status).and_then(|c| fen(&c[1])).unwrap_or(0).min(total)
}

pub struct Parsed {
    pub src: &'static str,
    pub format: &'static str,
    pub export: String,
    pub bills: Vec<Bill>,
    pub skipped: usize,
}

pub fn parse_rows(rows: &[Vec<String>], fallback_export: &str) -> Result<Parsed, String> {
    let hi = rows
        .iter()
        .position(|r| r.first().map(|c| matches!(clean(c).as_str(), "交易时间" | "交易号")).unwrap_or(false))
        .ok_or("认不出这个文件：只认微信、支付宝导出的账单")?;
    let head: Vec<String> = rows[hi].iter().map(|c| clean(c)).collect();
    let col = |name: &str| head.iter().position(|h| h == name);
    let (src, format) = if col("交易单号").is_some() {
        ("wechat", "微信")
    } else if col("交易订单号").is_some() {
        ("alipay", "支付宝")
    } else if col("交易号").is_some() && col("交易创建时间").is_some() {
        ("alipay", "支付宝网页版")
    } else {
        return Err("认不出这个文件：只认微信、支付宝导出的账单".into());
    };
    let export = rows
        .iter()
        .filter(|r| r.iter().any(|c| c.contains("导出时间")))
        .find_map(|r| datetime_in(&r.join(" ")))
        .unwrap_or_else(|| fallback_export.to_string());
    let pick = |names: &[&str]| names.iter().find_map(|n| col(n));
    let c_id = pick(&["交易单号", "交易订单号", "交易号"]);
    let c_time = pick(&["交易时间", "交易创建时间"]);
    let c_amt = pick(&["金额(元)", "金额", "金额（元）"]);
    let (Some(c_id), Some(c_time), Some(c_amt)) = (c_id, c_time, c_amt) else {
        return Err("账单少了交易号、时间或金额这几列".into());
    };
    let date_re = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}").unwrap();
    let (mut bills, mut skipped) = (vec![], 0);
    for r in &rows[hi + 1..] {
        let get = |c: Option<usize>| c.and_then(|i| r.get(i)).map(|s| clean(s)).unwrap_or_default();
        let time = datetime_in(&get(Some(c_time))).unwrap_or_default();
        let id = get(Some(c_id));
        if !date_re.is_match(&time) {
            // 表尾的合计、空行
            continue;
        }
        let Some(amount) = fen(&get(Some(c_amt))) else {
            skipped += 1;
            continue;
        };
        if id.is_empty() {
            skipped += 1;
            continue;
        }
        let status = get(pick(&["当前状态", "交易状态"]));
        let category = get(col("交易分类"));
        let item = get(pick(&["商品", "商品说明", "商品名称"]));
        let kind = get(pick(&["交易类型", "类型"]));
        let mut b = Bill {
            id: format!("{src}:{id}"),
            src: src.into(),
            time,
            dir: dir_of(&get(col("收/支"))).into(),
            fen: amount,
            counterparty: get(col("交易对方")),
            method: get(pick(&["支付方式", "收/付款方式"])),
            order: get(pick(&["商户单号", "商家订单号"])),
            note: get(col("备注")),
            export: export.clone(),
            ..Default::default()
        };
        if src == "wechat" {
            b.is_refund = kind.ends_with("退款");
            if !b.is_refund {
                b.refund_fen = wechat_refund(&status, amount);
            }
        } else {
            // 支付宝退款行：交易号「原单号_…」，状态「退款成功」或分类「退款」
            if let Some((p, _)) = id.split_once('_') {
                if status.contains("退款") || category == "退款" || item.starts_with("退款") {
                    b.is_refund = true;
                    b.parent = format!("alipay:{p}");
                }
            }
            b.refund_fen = get(col("成功退款（元）")).parse::<f64>().map(|v| (v * 100.0).round() as i64).unwrap_or(0).min(amount);
        }
        b.status = status;
        b.category = category;
        b.item = item;
        b.kind = kind;
        bills.push(b);
    }
    Ok(Parsed { src, format, export, bills, skipped })
}

/// 同一笔两份：导出晚的为准，它缺的字段从早的补；已退金额只增不减
fn merge(old: &Bill, new: &Bill) -> Bill {
    let (mut a, b) = if new.export >= old.export { (new.clone(), old) } else { (old.clone(), new) };
    for (x, y) in [
        (&mut a.category, &b.category),
        (&mut a.kind, &b.kind),
        (&mut a.counterparty, &b.counterparty),
        (&mut a.item, &b.item),
        (&mut a.method, &b.method),
        (&mut a.status, &b.status),
        (&mut a.order, &b.order),
        (&mut a.note, &b.note),
        (&mut a.parent, &b.parent),
    ] {
        if x.is_empty() {
            *x = y.clone();
        }
    }
    a.refund_fen = a.refund_fen.max(b.refund_fen);
    a.is_refund |= b.is_refund;
    a
}

/// 并进已有的：返回（新增，有变化）
pub fn merge_into(all: &mut BTreeMap<String, Bill>, bills: Vec<Bill>) -> (usize, usize) {
    let (mut added, mut changed) = (0, 0);
    for b in bills {
        match all.get(&b.id) {
            None => {
                added += 1;
                all.insert(b.id.clone(), b);
            }
            Some(old) => {
                let m = merge(old, &b);
                if &m != old {
                    changed += 1;
                    all.insert(m.id.clone(), m);
                }
            }
        }
    }
    (added, changed)
}

// ---------- 算账 ----------

/// 每一笔实际退了多少（分）：行上写的、挂回来的退款行加起来，取大的；关闭、失败的支出算全退
pub fn refunds(all: &BTreeMap<String, Bill>) -> HashMap<String, i64> {
    let mut kids: HashMap<&str, i64> = HashMap::new();
    for b in all.values().filter(|b| b.is_refund && !b.parent.is_empty()) {
        *kids.entry(b.parent.as_str()).or_default() += b.fen;
    }
    all.values()
        .map(|b| {
            let mut r = b.refund_fen.max(kids.get(b.id.as_str()).copied().unwrap_or(0));
            if b.dir == "out" && (b.status.contains("关闭") || b.status.contains("失败")) {
                r = b.fen;
            }
            (b.id.clone(), r.min(b.fen))
        })
        .collect()
}

/// 这一笔算进支出、收入的金额（分）：（支出净额，收入）
pub fn counted(b: &Bill, refund: i64) -> (i64, i64) {
    if b.is_refund {
        return (0, 0);
    }
    match b.dir.as_str() {
        "out" => (b.fen - refund, 0),
        "in" => (0, b.fen),
        _ => (0, 0),
    }
}

fn local_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn mtime_local(p: &Path) -> String {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|_| local_now())
}

// ---------- 给网页的命令 ----------

/// 导入几份账单：每份的结果（来源、几笔、新增几笔、更新几笔，或者错在哪）
#[tauri::command(async)]
pub fn import_bills(paths: Vec<String>) -> Value {
    let p = store_path();
    let mut all = load(&p);
    let mut files = vec![];
    let (mut added, mut changed) = (0, 0);
    for f in &paths {
        let path = Path::new(f);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match table(path).and_then(|rows| parse_rows(&rows, &mtime_local(path))) {
            Ok(parsed) => {
                let n = parsed.bills.len();
                let (a, c) = merge_into(&mut all, parsed.bills);
                added += a;
                changed += c;
                files.push(json!({"name": name, "src": parsed.src, "format": parsed.format, "export": parsed.export, "rows": n, "added": a, "updated": c, "skipped": parsed.skipped}));
            }
            Err(e) => files.push(json!({"name": name, "error": e})),
        }
    }
    if added + changed > 0 {
        if let Err(e) = save(&p, &all) {
            return json!({"files": files, "added": 0, "updated": 0, "error": format!("存不下：{e}")});
        }
    }
    json!({"files": files, "added": added, "updated": changed})
}

/// 选账单文件（可以多选）；点了取消是空的
#[tauri::command(async)]
pub fn pick_bill_files() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    let o = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "set fs to choose file with prompt \"选微信、支付宝导出的账单\" of type {\"csv\", \"xlsx\", \"public.comma-separated-values-text\", \"org.openxmlformats.spreadsheetml.sheet\"} with multiple selections allowed",
            "-e",
            "set out to \"\"",
            "-e",
            "repeat with f in fs\nset out to out & POSIX path of f & linefeed\nend repeat",
            "-e",
            "return out",
        ])
        .output();
    #[cfg(target_os = "windows")]
    let o = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Multiselect = $true; $d.Filter = 'csv, xlsx|*.csv;*.xlsx'; if ($d.ShowDialog() -eq 'OK') { $d.FileNames -join \"`n\" }",
        ])
        .output();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let o: std::io::Result<std::process::Output> = Err(std::io::Error::other("这个系统没有选文件窗口，把文件拖进窗口"));
    let o = o.map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Ok(vec![]);
    }
    Ok(String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// 一个月的收支：合计、每天、去向（按对方）、明细，以及有账单的月份和每个来源覆盖到哪天
#[tauri::command(async)]
pub fn get_bills(month: Option<String>) -> Value {
    let all = load(&store_path());
    let re = refunds(&all);
    let mut months: Vec<String> = all.values().map(|b| b.time[..7].to_string()).collect();
    months.sort();
    months.dedup();
    months.reverse();
    let month = month.filter(|m| m.len() == 7).or_else(|| months.first().cloned()).unwrap_or_else(|| chrono::Local::now().format("%Y-%m").to_string());

    let mut cover: BTreeMap<String, (String, String, String, usize)> = BTreeMap::new();
    for b in all.values() {
        let e = cover.entry(b.src.clone()).or_insert((b.time.clone(), b.time.clone(), b.export.clone(), 0));
        e.0 = e.0.clone().min(b.time.clone());
        e.1 = e.1.clone().max(b.time.clone());
        e.2 = e.2.clone().max(b.export.clone());
        e.3 += 1;
    }
    let coverage: Value = cover.into_iter().map(|(k, (first, last, export, n))| (k, json!({"first": first, "last": last, "export": export, "count": n}))).collect::<serde_json::Map<_, _>>().into();

    let (mut spend, mut income, mut refund, mut n) = (0i64, 0i64, 0i64, 0usize);
    let mut days: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut to: HashMap<String, (i64, usize)> = HashMap::new();
    let mut items = vec![];
    let mut rows: Vec<&Bill> = all.values().filter(|b| b.time.starts_with(&month)).collect();
    rows.sort_by(|a, b| b.time.cmp(&a.time));
    for b in rows {
        let r = re.get(&b.id).copied().unwrap_or(0);
        let (out, inc) = counted(b, r);
        let counts = !b.is_refund && b.dir != "none";
        if counts {
            n += 1;
            spend += out;
            income += inc;
            if b.dir == "out" {
                refund += r;
            }
            let d = days.entry(b.time[..10].to_string()).or_default();
            d.0 += out;
            d.1 += inc;
            if out > 0 {
                let who = if b.counterparty.is_empty() { b.item.clone() } else { b.counterparty.clone() };
                let t = to.entry(who).or_default();
                t.0 += out;
                t.1 += 1;
            }
        }
        items.push(json!({
            "id": b.id, "src": b.src, "time": b.time, "dir": b.dir, "fen": b.fen, "refund_fen": if b.dir == "out" { r } else { 0 },
            "counted": counts, "is_refund": b.is_refund,
            "category": if b.category.is_empty() { &b.kind } else { &b.category },
            "counterparty": b.counterparty, "item": b.item, "method": b.method, "status": b.status, "note": b.note,
        }));
    }
    let mut to: Vec<(String, (i64, usize))> = to.into_iter().collect();
    to.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    json!({
        "month": month,
        "months": months,
        "spend_fen": spend, "income_fen": income, "refund_fen": refund, "count": n,
        "days": days.into_iter().map(|(d, (o, i))| json!({"date": d, "spend_fen": o, "income_fen": i})).collect::<Vec<_>>(),
        "to": to.into_iter().take(10).map(|(name, (f, c))| json!({"name": name, "spend_fen": f, "count": c})).collect::<Vec<_>>(),
        "items": items,
        "coverage": coverage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Vec<String>> {
        text.lines().map(csv_line).collect()
    }

    const ALIPAY_APP: &str = "导出时间：[2026-03-04 11:38:59]\n\
------------------------支付宝支付科技有限公司  电子客户回单------------------------\n\
交易时间,交易分类,交易对方,对方账号,商品说明,收/支,金额,收/付款方式,交易状态,交易订单号,商家订单号,备注,\n\
2026-02-20 10:57:23,退款,某店,/,退款-耳机,不计收支,29.85,余额宝,退款成功,2026022023001_32531\t,T200P1\t,,\n\
2026-02-20 08:39:24,日用百货,某店,/,耳机,支出,29.85,余额宝,交易关闭,2026022023001\t,T200P1\t,,\n\
2026-02-21 12:00:00,餐饮美食,咖啡店,/,咖啡,支出,32.00,余额宝,等待确认收货,2026022100002\t,M1\t,,\n\
2026-02-22 09:00:00,投资理财,余额宝,/,转入,不计收支,95.10,银行卡,交易成功,2026022200003\t,\t,,\n\
2026-02-23 09:00:00,转账红包,朋友,/,收款,收入,100.00,账户余额,交易成功,2026022300004\t,\t,,\n";

    const ALIPAY_WEB: &str = "支付宝交易记录明细查询\n\
---------------------------------交易记录明细列表------------------------------------\n\
交易号                  ,商家订单号               ,交易创建时间              ,付款时间                ,最近修改时间              ,交易来源地     ,类型              ,交易对方            ,商品名称                ,金额（元）   ,收/支     ,交易状态    ,服务费（元）   ,成功退款（元）  ,备注                  ,资金状态     ,\n\
2026022100002\t,M1 \t,2026-02-21 12:00:00 ,2026-02-21 12:00:05 ,2026-02-22 12:00:00 ,淘宝        ,支付宝担保交易         ,咖啡店            ,咖啡                ,32.00     ,支出      ,交易成功    ,0.00     ,2.50     ,                    ,已支出     ,\n\
2026022400005\t,M2 \t,2026-02-24 12:00:00 ,2026-02-24 12:00:05 ,2026-02-24 12:00:05 ,淘宝        ,即时到账交易         ,面馆            ,面                ,18.00     ,支出      ,交易成功    ,0.00     ,0.00     ,                    ,已支出     ,\n\
------------------------------------------------------------------------------------\n\
共2笔记录\n\
导出时间:[2026-11-04 10:43:03]    用户:某人\n";

    const WECHAT: &str = "微信支付账单明细\n\
导出时间：[2026-03-04 11:37:01]\n\
----------------------微信支付账单明细列表--------------------\n\
交易时间,交易类型,交易对方,商品,收/支,金额(元),支付方式,当前状态,交易单号,商户单号,备注\n\
2026-02-25 10:00:00,微信红包（群红包）,群,/,支出,¥100.00,零钱,已退款(￥6.38),w1,/,/\n\
2026-02-26 10:00:00,微信红包-退款,群,/,收入,¥6.38,/,已退款￥6.38,w2,/,/\n\
2026-02-27 10:00:00,转账,朋友,/,支出,\"¥4,500.00\",零钱,对方已退还,w3,/,/\n\
2026-02-27 11:00:00,转账-退款,朋友,/,收入,¥4500.00,/,已全额退款,w4,/,/\n\
2026-02-28 10:00:00,零钱提现,银行,/,/,¥70.57,银行卡,提现已到账,w5,/,服务费¥0.07\n\
2026-02-28 12:00:00,商户消费,便利店,饮料,支出,¥5.00,零钱,支付成功,w6,/,/\n";

    fn totals(all: &BTreeMap<String, Bill>) -> (i64, i64) {
        let re = refunds(all);
        all.values().map(|b| counted(b, re[&b.id])).fold((0, 0), |a, x| (a.0 + x.0, a.1 + x.1))
    }

    #[test]
    fn alipay_app() {
        let p = parse_rows(&rows(ALIPAY_APP), "").unwrap();
        assert_eq!((p.src, p.export.as_str(), p.bills.len()), ("alipay", "2026-03-04 11:38:59", 5));
        let r = &p.bills[0];
        assert!(r.is_refund);
        assert_eq!(r.parent, "alipay:2026022023001");
        let mut all = BTreeMap::new();
        merge_into(&mut all, p.bills);
        // 关闭的耳机退了，退款行不算收入，转余额宝不算：支出只有咖啡，收入 100
        assert_eq!(totals(&all), (3200, 10000));
    }

    #[test]
    fn web_then_app_dedup() {
        let mut all = BTreeMap::new();
        let w = parse_rows(&rows(ALIPAY_WEB), "").unwrap();
        assert_eq!((w.format, w.bills.len()), ("支付宝网页版", 2));
        assert_eq!(merge_into(&mut all, w.bills), (2, 0));
        // 早导出的 app 版再导：咖啡那笔已有，状态不被旧的「等待确认收货」盖掉，分类补上
        let a = parse_rows(&rows(ALIPAY_APP), "").unwrap();
        assert_eq!(merge_into(&mut all, a.bills), (4, 1));
        let c = &all["alipay:2026022100002"];
        assert_eq!((c.status.as_str(), c.category.as_str(), c.refund_fen), ("交易成功", "餐饮美食", 250));
        // 再导一遍什么都不变
        let a = parse_rows(&rows(ALIPAY_APP), "").unwrap();
        assert_eq!(merge_into(&mut all, a.bills), (0, 0));
        // 咖啡 32 退 2.5、面 18
        assert_eq!(totals(&all), (2950 + 1800, 10000));
    }

    #[test]
    fn wechat() {
        let p = parse_rows(&rows(WECHAT), "").unwrap();
        assert_eq!((p.src, p.bills.len()), ("wechat", 6));
        assert_eq!(p.bills[0].refund_fen, 638);
        assert_eq!(p.bills[2].fen, 450000);
        let mut all = BTreeMap::new();
        merge_into(&mut all, p.bills);
        // 红包 100 退 6.38，转账全退，提现中性，退款行不算收入，便利店 5
        assert_eq!(totals(&all), (10000 - 638 + 500, 0));
    }

    #[test]
    fn not_a_bill() {
        assert!(parse_rows(&rows("a,b,c\n1,2,3\n"), "").is_err());
    }

    /// 用本机真账单试：BEAYY_BILLS=文件1:文件2 cargo test -p beayy real_files -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_files() {
        let mut all = BTreeMap::new();
        for f in std::env::var("BEAYY_BILLS").unwrap_or_default().split(':').filter(|s| !s.is_empty()) {
            let p = parse_rows(&table(Path::new(f)).unwrap(), "").unwrap();
            let (n, s) = (p.bills.len(), p.skipped);
            let (a, c) = merge_into(&mut all, p.bills);
            println!("{} {} 笔 跳过 {s} 新增 {a} 更新 {c} 导出 {}", p.format, n, p.export);
        }
        let mut by: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        let re = refunds(&all);
        for b in all.values() {
            let (o, i) = counted(b, re[&b.id]);
            let e = by.entry(format!("{} {}", b.src, &b.time[..7])).or_default();
            e.0 += o;
            e.1 += i;
        }
        for (k, (o, i)) in by {
            println!("{k} 支出 {:.2} 收入 {:.2}", o as f64 / 100.0, i as f64 / 100.0);
        }
    }
}
