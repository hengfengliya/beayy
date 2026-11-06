//! Steam：每 30 秒看一眼在玩哪款，记每一次从开到关；游戏库里每款的总时长变了记一份。
//!
//! 只读：注册表 `HKCU\Software\Valve\Steam` 的 `RunningAppID`（在玩的，0＝没玩）、`SteamPath`、`ActiveProcess\ActiveUser`，
//! `<Steam>\steamapps\appmanifest_*.acf`（装着的游戏名，各个库目录见 `libraryfolders.vdf`；没装的从 `appcache\appinfo.vdf` 读），
//! `<Steam>\userdata\<账号>\config\localconfig.vdf`（每款的总时长、最近玩），`config\loginusers.vdf`（昵称）。不碰 Steam 和游戏进程。
//!
//! - 事件：`steam.session`（`state`＝`start` 开始一次；`end` 结束，带开始、结束、时长），`steam.library`（每款的总时长，变了才写）
//! - 在玩的那次存在 `<数据目录>/games/steam.json`：Beayy 关了又开、游戏还开着就接着算；两次看隔得太久（电脑睡了、Beayy 没开）就在上次看到的时刻结束
//! - `BEAYY_STEAM_DIR` 指定 Steam 目录（测试用）

use beayy_core::{fields, iso, EventLog};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 多久看一次
pub const EVERY: Duration = Duration::from_secs(30);
/// 两次看隔了超过这么久就不算一直在玩
const GAP_S: i64 = 3 * 30;
/// 游戏库多久对一次（没在玩的时候）
const LIBRARY_EVERY_S: i64 = 3600;

// ---------- VDF（Valve 的文本 KeyValues） ----------

/// `"键" "值"`、`"键" { … }` → JSON 对象；`//` 注释、`[$WIN32]` 条件跳过；同名键后面的盖前面的
pub fn parse_vdf(s: &str) -> Value {
    let toks = tokens(s);
    let mut i = 0;
    Value::Object(object(&toks, &mut i))
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

fn tokens(s: &str) -> Vec<Tok> {
    let mut out = vec![];
    let mut cs = s.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '"' => {
                let mut v = String::new();
                while let Some(c) = cs.next() {
                    match c {
                        '"' => break,
                        '\\' => match cs.next() {
                            Some('n') => v.push('\n'),
                            Some('t') => v.push('\t'),
                            Some(x) => v.push(x),
                            None => {}
                        },
                        _ => v.push(c),
                    }
                }
                out.push(Tok::Str(v));
            }
            '/' if cs.peek() == Some(&'/') => {
                for c in cs.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            '[' => {
                for c in cs.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            c => {
                let mut v = String::from(c);
                while let Some(&n) = cs.peek() {
                    if n.is_whitespace() || matches!(n, '{' | '}' | '"') {
                        break;
                    }
                    v.push(n);
                    cs.next();
                }
                out.push(Tok::Str(v));
            }
        }
    }
    out
}

fn object(t: &[Tok], i: &mut usize) -> Map<String, Value> {
    let mut m = Map::new();
    while *i < t.len() {
        match &t[*i] {
            Tok::Close => {
                *i += 1;
                return m;
            }
            Tok::Open => *i += 1,
            Tok::Str(k) => {
                *i += 1;
                match t.get(*i) {
                    Some(Tok::Open) => {
                        *i += 1;
                        let v = object(t, i);
                        m.insert(k.clone(), Value::Object(v));
                    }
                    Some(Tok::Str(v)) => {
                        m.insert(k.clone(), json!(v));
                        *i += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    m
}

/// 按路径取，键不分大小写（`apps`、`Apps` 都有人写）
pub fn vdf_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |cur, k| cur.as_object()?.iter().find(|(kk, _)| kk.eq_ignore_ascii_case(k)).map(|(_, v)| v))
}

fn vdf_i64(v: &Value, k: &str) -> i64 {
    vdf_at(v, &[k]).and_then(Value::as_str).and_then(|s| s.parse().ok()).unwrap_or(0)
}

// ---------- 读 Steam 的文件 ----------

/// SteamID64 → 账号号（userdata 下的目录名）
const ID64_BASE: u64 = 76_561_197_960_265_728;

/// 各个库目录（`libraryfolders.vdf`），Steam 目录自己在第一个
pub fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    if let Ok(s) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        let v = parse_vdf(&s);
        for (_, f) in vdf_at(&v, &["libraryfolders"]).and_then(Value::as_object).into_iter().flatten() {
            if let Some(p) = vdf_at(f, &["path"]).and_then(Value::as_str) {
                let p = PathBuf::from(p);
                if !out.iter().any(|o| same_dir(o, &p)) {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    n(a) == n(b)
}

/// 装着的游戏：appid → 名字
pub fn installed(root: &Path) -> BTreeMap<u32, String> {
    let mut m = BTreeMap::new();
    for lib in libraries(root) {
        let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let Ok(s) = std::fs::read_to_string(e.path()) else { continue };
            let v = parse_vdf(&s);
            let st = vdf_at(&v, &["AppState"]).cloned().unwrap_or(Value::Null);
            let id = vdf_i64(&st, "appid");
            if let (true, Some(name)) = (id > 0, vdf_at(&st, &["name"]).and_then(Value::as_str)) {
                m.insert(id as u32, name.to_string());
            }
        }
    }
    m
}

/// 登录过的账号：账号号 → 昵称；`most_recent` 是最近登录的那个
pub fn login_users(root: &Path) -> (BTreeMap<u32, String>, Option<u32>) {
    let mut m = BTreeMap::new();
    let mut recent = None;
    let Ok(s) = std::fs::read_to_string(root.join("config/loginusers.vdf")) else { return (m, None) };
    let v = parse_vdf(&s);
    for (id64, u) in vdf_at(&v, &["users"]).and_then(Value::as_object).into_iter().flatten() {
        let Some(id) = id64.parse::<u64>().ok().and_then(|x| x.checked_sub(ID64_BASE)).and_then(|x| u32::try_from(x).ok()) else { continue };
        m.insert(id, vdf_at(u, &["PersonaName"]).and_then(Value::as_str).unwrap_or("").to_string());
        if vdf_at(u, &["MostRecent"]).and_then(Value::as_str) == Some("1") {
            recent = Some(id);
        }
    }
    (m, recent)
}

/// 一个账号的游戏库：每款的总分钟数、最近玩（只留玩过的）
pub fn playtimes(root: &Path, account: u32) -> Vec<(u32, i64, i64)> {
    let Ok(s) = std::fs::read_to_string(root.join(format!("userdata/{account}/config/localconfig.vdf"))) else { return vec![] };
    let v = parse_vdf(&s);
    let apps = vdf_at(&v, &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]).and_then(Value::as_object);
    let mut out: Vec<(u32, i64, i64)> = apps
        .into_iter()
        .flatten()
        .filter_map(|(id, a)| Some((id.parse().ok()?, vdf_i64(a, "Playtime"), vdf_i64(a, "LastPlayed"))))
        .filter(|(_, p, _)| *p > 0)
        .collect();
    out.sort();
    out
}

/// 在用的账号：Steam 开着时注册表里有；没有就用最近登录的、再没有就用 userdata 下唯一的那个
fn account(root: &Path, active: Option<u32>) -> Option<u32> {
    if let Some(a) = active.filter(|a| *a > 0) {
        return Some(a);
    }
    if let (_, Some(r)) = login_users(root) {
        return Some(r);
    }
    let ids: Vec<u32> = std::fs::read_dir(root.join("userdata")).ok()?.flatten().filter_map(|e| e.file_name().to_str()?.parse().ok()).filter(|x| *x > 0).collect();
    (ids.len() == 1).then(|| ids[0])
}

// ---------- appinfo.vdf（Steam 缓存的每款游戏信息，二进制） ----------

/// `appcache/appinfo.vdf` 里这些 appid 的名字（`common > name`）。没装的游戏只有这里有名字。
/// 认第 28、29 版（magic 0x07564428、0x07564429）：每条 appid、size、再 60 字节头，后面是二进制 KeyValues；
/// 第 29 版的键名是文件末尾字符串表的下标。读不懂就返回空的
pub fn appinfo_names(root: &Path, want: &BTreeSet<u32>) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    if want.is_empty() {
        return out;
    }
    let Ok(b) = std::fs::read(root.join("appcache/appinfo.vdf")) else { return out };
    let u32_at = |p: usize| b.get(p..p + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    let Some(magic) = u32_at(0) else { return out };
    let (mut pos, strings) = match magic {
        0x0756_4428 => (8, None),
        0x0756_4429 => {
            let Some(off) = b.get(8..16).map(|x| u64::from_le_bytes(x.try_into().unwrap_or_default()) as usize) else { return out };
            (16, Some(string_table(&b, off)))
        }
        _ => return out,
    };
    while let (Some(id), Some(size)) = (u32_at(pos), u32_at(pos + 4)) {
        if id == 0 {
            break;
        }
        let (start, end) = (pos + 8, pos + 8 + size as usize);
        if end > b.len() {
            break;
        }
        if want.contains(&id) {
            let mut kv = Kv { b: &b[start..end], i: 60, strings: strings.as_deref() };
            let v = Value::Object(kv.object());
            if let Some(n) = vdf_at(&v, &["appinfo", "common", "name"]).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                out.insert(id, n.to_string());
            }
        }
        pos = end;
    }
    out
}

fn string_table(b: &[u8], off: usize) -> Vec<String> {
    let Some(n) = b.get(off..off + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize) else { return vec![] };
    let mut out = Vec::with_capacity(n.min(1 << 16));
    let mut p = off + 4;
    while out.len() < n && p < b.len() {
        let e = b[p..].iter().position(|&c| c == 0).map_or(b.len(), |x| p + x);
        out.push(String::from_utf8_lossy(&b[p..e]).into_owned());
        p = e + 1;
    }
    out
}

/// 二进制 KeyValues：类型 1 字节、键、值；0 子节、1 字符串、2 int32、3 float、4 指针、5 宽字符串、6 颜色、7 uint64、10 int64，8 和 11 是节尾
struct Kv<'a> {
    b: &'a [u8],
    i: usize,
    strings: Option<&'a [String]>,
}

impl Kv<'_> {
    fn cstr(&mut self) -> Option<String> {
        let e = self.b.get(self.i..)?.iter().position(|&c| c == 0)? + self.i;
        let s = String::from_utf8_lossy(&self.b[self.i..e]).into_owned();
        self.i = e + 1;
        Some(s)
    }
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn key(&mut self) -> Option<String> {
        match self.strings {
            Some(t) => {
                let x = self.take(4)?;
                t.get(u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize).cloned()
            }
            None => self.cstr(),
        }
    }
    fn object(&mut self) -> Map<String, Value> {
        let mut m = Map::new();
        while let Some(&t) = self.b.get(self.i) {
            self.i += 1;
            if t == 8 || t == 11 {
                break;
            }
            let Some(k) = self.key() else { break };
            let v = match t {
                0 => Value::Object(self.object()),
                1 => match self.cstr() {
                    Some(s) => json!(s),
                    None => break,
                },
                2 | 4 | 6 => match self.take(4) {
                    Some(x) => json!(i32::from_le_bytes([x[0], x[1], x[2], x[3]])),
                    None => break,
                },
                3 => match self.take(4) {
                    Some(x) => json!(f32::from_le_bytes([x[0], x[1], x[2], x[3]])),
                    None => break,
                },
                7 | 10 => match self.take(8) {
                    Some(x) => json!(i64::from_le_bytes(x.try_into().unwrap_or_default())),
                    None => break,
                },
                5 => {
                    // 宽字符串：两字节一个，到 0x0000
                    let mut u = vec![];
                    while let Some(x) = self.take(2) {
                        let c = u16::from_le_bytes([x[0], x[1]]);
                        if c == 0 {
                            break;
                        }
                        u.push(c);
                    }
                    json!(String::from_utf16_lossy(&u))
                }
                _ => break,
            };
            m.insert(k, v);
        }
        m
    }
}

// ---------- 注册表 ----------

/// Steam 现在的样子
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Now {
    /// Steam 目录
    pub root: Option<PathBuf>,
    /// 在玩的 appid（0＝没玩）
    pub running: u32,
    /// 在用的账号
    pub active_user: Option<u32>,
}

pub fn read_now() -> Now {
    let root = std::env::var("BEAYY_STEAM_DIR").ok().map(PathBuf::from);
    #[cfg(target_os = "windows")]
    {
        let root = root.or_else(|| win::string("Software\\Valve\\Steam", "SteamPath").map(PathBuf::from)).filter(|p| p.is_dir());
        Now { root, running: win::dword("Software\\Valve\\Steam", "RunningAppID").unwrap_or(0), active_user: win::dword("Software\\Valve\\Steam\\ActiveProcess", "ActiveUser") }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Now { root, running: 0, active_user: None }
    }
}

/// 注册表里 Steam 记的游戏名（`Apps\<appid>\Name`，装过的才有）
fn registry_name(id: u32) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        win::string(&format!("Software\\Valve\\Steam\\Apps\\{id}"), "Name").filter(|s| !s.is_empty())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = id;
        None
    }
}

#[cfg(target_os = "windows")]
mod win {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn dword(key: &str, name: &str) -> Option<u32> {
        let (k, n) = (wide(key), wide(name));
        let mut v = 0u32;
        let mut len = 4u32;
        let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut len)) };
        err.is_ok().then_some(v)
    }

    pub fn string(key: &str, name: &str) -> Option<String> {
        let (k, n) = (wide(key), wide(name));
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
        if err.is_err() {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    }
}

// ---------- 记 ----------

#[derive(Clone, Debug, PartialEq)]
struct Cur {
    appid: u32,
    name: Option<String>,
    start: DateTime<Utc>,
    seen: DateTime<Utc>,
}

pub struct Steam {
    dir: PathBuf,
    log: Arc<EventLog>,
    paused: Arc<AtomicBool>,
    cur: Option<Cur>,
    /// 上次写的游戏库（比较用）
    library: Value,
    library_at: Option<DateTime<Utc>>,
}

fn ts(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

impl Steam {
    pub fn new(root: &Path, log: Arc<EventLog>, paused: Arc<AtomicBool>) -> Self {
        let dir = root.join("games");
        let v: Value = std::fs::read(dir.join("steam.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        let cur = (|| {
            let c = &v["cur"];
            Some(Cur { appid: c["appid"].as_u64()? as u32, name: c["name"].as_str().map(String::from), start: ts(&c["start"])?, seen: ts(&c["seen"])? })
        })();
        Steam { dir, log, paused, cur, library: v["library"].clone(), library_at: None }
    }

    fn write(&self, t: &str, f: Map<String, Value>, at: Option<DateTime<Utc>>) {
        if !self.paused.load(Ordering::Relaxed) {
            self.log.write(t, f, at);
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let cur = self.cur.as_ref().map(|c| json!({"appid": c.appid, "name": c.name, "start": iso(c.start), "seen": iso(c.seen)}));
        let v = json!({"cur": cur, "library": self.library});
        let tmp = self.dir.join("steam.json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, self.dir.join("steam.json"));
        }
    }

    fn name_of(id: u32, root: Option<&Path>) -> Option<String> {
        root.and_then(|r| installed(r).remove(&id)).or_else(|| registry_name(id))
    }

    /// 看一眼：`now` 是这一刻 Steam 的样子
    pub fn step(&mut self, now: &Now, t: DateTime<Utc>) {
        let mut changed = false;
        // 上次看到的这次：隔太久了先在上次看到的时刻结束；换了游戏、关了游戏在这一刻结束
        if let Some(c) = self.cur.clone() {
            if (t - c.seen).num_seconds() > GAP_S {
                self.end(t.min(c.seen));
                changed = true;
            } else if now.running != c.appid {
                self.end(t);
                changed = true;
            }
        }
        if now.running > 0 {
            match &mut self.cur {
                Some(c) => c.seen = t,
                None => {
                    let name = Self::name_of(now.running, now.root.as_deref());
                    self.write("steam.session", fields! {"state" => "start", "appid" => now.running, "name" => name, "started_at" => iso(t)}, Some(t));
                    self.cur = Some(Cur { appid: now.running, name, start: t, seen: t });
                }
            }
        }
        // 没在玩的时候对游戏库：刚结束一次（Steam 退出游戏后才写总时长，下一次看时再对）、每小时、刚开
        let due = self.library_at.is_none_or(|a| (t - a).num_seconds() >= LIBRARY_EVERY_S) || changed;
        if self.cur.is_none() && due {
            if let Some(root) = &now.root {
                self.check_library(root, now.active_user, t);
            }
        }
        if self.cur.is_some() || changed {
            self.save();
        }
    }

    fn end(&mut self, at: DateTime<Utc>) {
        let Some(c) = self.cur.take() else { return };
        let dur = (at - c.start).num_seconds().max(0);
        self.write(
            "steam.session",
            fields! {"state" => "end", "appid" => c.appid, "name" => c.name, "started_at" => iso(c.start), "ended_at" => iso(at), "duration_s" => dur},
            Some(at),
        );
        // 刚结束的这次之后马上（下一次看）对一下游戏库
        self.library_at = None;
    }

    /// 关掉了游戏记录：在玩的那次在这一刻结束
    pub fn stop(&mut self, t: DateTime<Utc>) {
        if self.cur.is_some() {
            self.end(t);
            self.save();
        }
    }

    fn check_library(&mut self, root: &Path, active: Option<u32>, t: DateTime<Utc>) {
        self.library_at = Some(t);
        let Some(acc) = account(root, active) else { return };
        let pt = playtimes(root, acc);
        if pt.is_empty() {
            return;
        }
        let mut names = installed(root);
        for id in pt.iter().map(|p| p.0).filter(|id| !names.contains_key(id)).collect::<Vec<_>>() {
            if let Some(n) = registry_name(id) {
                names.insert(id, n);
            }
        }
        // 没装的游戏：名字只在 Steam 缓存的 appinfo.vdf 里
        let missing: BTreeSet<u32> = pt.iter().map(|p| p.0).filter(|id| !names.contains_key(id)).collect();
        names.extend(appinfo_names(root, &missing));
        let apps: Vec<Value> = pt
            .iter()
            .map(|(id, min, last)| {
                let name = names.get(id).cloned();
                let last = (*last > 0).then(|| Utc.timestamp_opt(*last, 0).single()).flatten().map(iso);
                json!({"appid": id, "name": name, "playtime_min": min, "last_played": last})
            })
            .collect();
        let v = json!({"account_id": acc, "apps": apps});
        if v == self.library {
            return;
        }
        let persona = login_users(root).0.remove(&acc).filter(|s| !s.is_empty());
        self.write("steam.library", fields! {"account_id" => acc, "persona" => persona, "apps" => apps}, Some(t));
        if !self.paused.load(Ordering::Relaxed) {
            self.library = v;
            self.save();
        }
    }
}

/// 起后台线程：每 30 秒看一眼。和英雄联盟一样受「游戏」开关管
pub fn start(log: Arc<EventLog>, paused: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut s = Steam::new(&beayy_core::data_root(), log, paused);
        loop {
            if crate::game_on("steam") {
                s.step(&read_now(), Utc::now());
            } else {
                s.stop(Utc::now());
            }
            std::thread::sleep(EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as D;

    #[test]
    fn vdf() {
        let s = "\"AppState\"\n{\n\t\"appid\"\t\t\"1091500\"\n\t\"name\"\t\t\"Cyberpunk 2077\" // 注释\n\t\"InstalledDepots\" { \"1\" { \"size\" \"3\" } }\n\t\"esc\" \"a\\\"b\\\\c\"\n}\n";
        let v = parse_vdf(s);
        assert_eq!(vdf_at(&v, &["appstate", "NAME"]).unwrap(), "Cyberpunk 2077");
        assert_eq!(vdf_at(&v, &["AppState", "InstalledDepots", "1", "size"]).unwrap(), "3");
        assert_eq!(vdf_at(&v, &["AppState", "esc"]).unwrap(), "a\"b\\c");
        assert_eq!(vdf_i64(vdf_at(&v, &["AppState"]).unwrap(), "appid"), 1091500);
    }

    fn fake_steam() -> (PathBuf, PathBuf) {
        // 测试并行跑：macOS 的时钟只到微秒，两个测试可能拿到同一个时刻、共用一个目录（先跑完的删掉，后面的就读不到），加个计数
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("beayy-steam-{}-{}-{n}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let root = base.join("Steam");
        let lib2 = base.join("Lib2");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::create_dir_all(lib2.join("steamapps")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("userdata/42/config")).unwrap();
        let lib2s = lib2.to_string_lossy().replace('\\', "\\\\");
        std::fs::write(root.join("steamapps/libraryfolders.vdf"), format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{lib2s}\" }} }}", root.to_string_lossy().replace('\\', "\\\\"))).unwrap();
        std::fs::write(root.join("steamapps/appmanifest_10.acf"), "\"AppState\" { \"appid\" \"10\" \"name\" \"雾港\" }").unwrap();
        std::fs::write(lib2.join("steamapps/appmanifest_20.acf"), "\"AppState\" { \"appid\" \"20\" \"name\" \"Lantern\" }").unwrap();
        std::fs::write(root.join("config/loginusers.vdf"), format!("\"users\" {{ \"{}\" {{ \"PersonaName\" \"旧人\" \"MostRecent\" \"1\" }} }}", ID64_BASE + 42)).unwrap();
        std::fs::write(
            root.join("userdata/42/config/localconfig.vdf"),
            "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"Apps\" { \"10\" { \"LastPlayed\" \"1762400000\" \"Playtime\" \"95\" } \"7\" { \"cloud\" { } } \"30\" { \"Playtime\" \"5\" } } } } } }",
        )
        .unwrap();
        (base, root)
    }

    /// 造一个 appinfo.vdf：v29 用字符串表、v28 键名直接写
    fn appinfo_bytes(v29: bool, apps: &[(u32, &str)]) -> Vec<u8> {
        let table = ["appinfo", "appid", "common", "name", "type"];
        let key = |out: &mut Vec<u8>, k: &str| {
            if v29 {
                out.extend((table.iter().position(|x| *x == k).unwrap() as u32).to_le_bytes());
            } else {
                out.extend(k.as_bytes());
                out.push(0);
            }
        };
        let mut body = vec![];
        for (id, name) in apps {
            let mut kv = vec![0u8];
            key(&mut kv, "appinfo");
            kv.push(2);
            key(&mut kv, "appid");
            kv.extend(id.to_le_bytes());
            kv.push(0);
            key(&mut kv, "common");
            kv.push(1);
            key(&mut kv, "type");
            kv.extend(b"Game\0");
            kv.push(1);
            key(&mut kv, "name");
            kv.extend(name.as_bytes());
            kv.push(0);
            kv.extend([8, 8, 8]);
            let mut e = vec![0u8; 60];
            e.extend(kv);
            body.extend(id.to_le_bytes());
            body.extend((e.len() as u32).to_le_bytes());
            body.extend(e);
        }
        body.extend(0u32.to_le_bytes());
        let mut out = vec![];
        if v29 {
            out.extend(0x0756_4429u32.to_le_bytes());
            out.extend(1u32.to_le_bytes());
            out.extend(((16 + body.len()) as u64).to_le_bytes());
            out.extend(body);
            out.extend((table.len() as u32).to_le_bytes());
            for k in table {
                out.extend(k.as_bytes());
                out.push(0);
            }
        } else {
            out.extend(0x0756_4428u32.to_le_bytes());
            out.extend(1u32.to_le_bytes());
            out.extend(body);
        }
        out
    }

    #[test]
    fn appinfo() {
        let (base, root) = fake_steam();
        std::fs::create_dir_all(root.join("appcache")).unwrap();
        let want = BTreeSet::from([30, 99]);
        for v29 in [true, false] {
            std::fs::write(root.join("appcache/appinfo.vdf"), appinfo_bytes(v29, &[(5, "别的"), (30, "余烬之路"), (31, "Not wanted")])).unwrap();
            assert_eq!(appinfo_names(&root, &want), BTreeMap::from([(30, "余烬之路".to_string())]));
        }
        // 认不得的版本、截断的文件：不报错，读不到就算了
        std::fs::write(root.join("appcache/appinfo.vdf"), [0x27u8, 0x44, 0x56, 0x07, 1, 0, 0, 0]).unwrap();
        assert!(appinfo_names(&root, &want).is_empty());
        let mut cut = appinfo_bytes(true, &[(30, "余烬之路")]);
        cut.truncate(40);
        std::fs::write(root.join("appcache/appinfo.vdf"), cut).unwrap();
        assert!(appinfo_names(&root, &want).is_empty());
        // 游戏库里没装的 30 号也带上名字
        let data = base.join("data");
        std::fs::write(root.join("appcache/appinfo.vdf"), appinfo_bytes(true, &[(30, "余烬之路")])).unwrap();
        let mut s = Steam::new(&data, Arc::new(EventLog::new(data.clone(), "win-test")), Arc::new(AtomicBool::new(false)));
        s.step(&Now { root: Some(root.clone()), running: 0, active_user: Some(42) }, Utc::now());
        let lib = read(&data).into_iter().find(|e| e["event_type"] == "steam.library").unwrap();
        let names: Vec<(i64, Value)> = lib["apps"].as_array().unwrap().iter().map(|a| (a["appid"].as_i64().unwrap(), a["name"].clone())).collect();
        assert_eq!(names, vec![(10, json!("雾港")), (30, json!("余烬之路"))]);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn files() {
        let (base, root) = fake_steam();
        assert_eq!(libraries(&root).len(), 2);
        let i = installed(&root);
        assert_eq!(i.get(&10).map(String::as_str), Some("雾港"));
        assert_eq!(i.get(&20).map(String::as_str), Some("Lantern"));
        let (u, recent) = login_users(&root);
        assert_eq!((u.get(&42).map(String::as_str), recent), (Some("旧人"), Some(42)));
        assert_eq!(playtimes(&root, 42), vec![(10, 95, 1762400000), (30, 5, 0)]);
        assert_eq!(account(&root, None), Some(42));
        let _ = std::fs::remove_dir_all(base);
    }

    fn read(root: &Path) -> Vec<Value> {
        let mut out = vec![];
        for e in walk(&root.join("devices")) {
            if e.file_name().is_some_and(|n| n == "events.jsonl") {
                out.extend(std::fs::read_to_string(&e).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()));
            }
        }
        out.sort_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()));
        out
    }

    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut v = vec![];
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                v.extend(walk(&e.path()));
            } else {
                v.push(e.path());
            }
        }
        v
    }

    #[test]
    fn sessions() {
        let (base, root) = fake_steam();
        let data = base.join("data");
        let log = Arc::new(EventLog::new(data.clone(), "win-test"));
        let paused = Arc::new(AtomicBool::new(false));
        let mut s = Steam::new(&data, log.clone(), paused.clone());
        let t0 = Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap();
        let idle = Now { root: Some(root.clone()), running: 0, active_user: Some(42) };
        let play = |id| Now { running: id, ..idle.clone() };
        s.step(&idle, t0); // 刚开：写一份游戏库
        s.step(&play(10), t0 + D::seconds(30));
        s.step(&play(10), t0 + D::seconds(60));
        // Beayy 重开，游戏还开着：接着算
        drop(s);
        let mut s = Steam::new(&data, log.clone(), paused.clone());
        s.step(&play(10), t0 + D::seconds(90));
        // 换了一款
        s.step(&play(20), t0 + D::seconds(120));
        // 电脑睡了一小时：在睡之前结束，醒来算新的一次
        s.step(&play(20), t0 + D::seconds(150));
        s.step(&play(20), t0 + D::seconds(3750));
        s.step(&idle, t0 + D::seconds(3780));
        let ev = read(&data);
        let ss: Vec<(&str, i64, i64)> = ev.iter().filter(|e| e["event_type"] == "steam.session").map(|e| (e["state"].as_str().unwrap(), e["appid"].as_i64().unwrap(), e["duration_s"].as_i64().unwrap_or(-1))).collect();
        assert_eq!(ss, vec![("start", 10, -1), ("end", 10, 90), ("start", 20, -1), ("end", 20, 30), ("start", 20, -1), ("end", 20, 30)]);
        let first = ev.iter().find(|e| e["event_type"] == "steam.session").unwrap();
        assert_eq!(first["name"], "雾港");
        let libs: Vec<&Value> = ev.iter().filter(|e| e["event_type"] == "steam.library").collect();
        // 游戏库没变：只写开头那一份
        assert_eq!(libs.len(), 1);
        assert_eq!(libs[0]["persona"], "旧人");
        assert_eq!(libs[0]["apps"][0]["playtime_min"], 95);
        // 总时长变了才再写
        std::fs::write(root.join("userdata/42/config/localconfig.vdf"), "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"10\" { \"Playtime\" \"97\" } } } } } }").unwrap();
        s.step(&idle, t0 + D::seconds(3780 + LIBRARY_EVERY_S));
        assert_eq!(read(&data).iter().filter(|e| e["event_type"] == "steam.library").count(), 2);
        let _ = std::fs::remove_dir_all(base);
    }
}
