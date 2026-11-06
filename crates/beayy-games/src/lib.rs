//! 英雄联盟：从客户端的本机接口（LCU）记每一局。
//!
//! 只发 GET：不接受对局、不改设置、不读游戏内存、不碰对局进程。客户端没开就什么都不做。
//!
//! - 找客户端：环境变量 `BEAYY_LOL_LOCKFILE` → 进程表里的 LeagueClientUx（Windows）→ 常见安装位置（Mac）。
//!   读 lockfile（`进程名:pid:端口:密码:协议`）；Windows 上没有 lockfile 时读进程命令行里的 `--app-port`、`--remoting-auth-token`
//! - 事件：`game.client`（连上、断开）、`game.phase`（阶段变化；离开选人那条带选人快照）、
//!   `game.match`（一局，摘要字段＋`raw` 接口原样，timestamp＝对局开始时刻）、`game.rank`（段位变了才记）
//! - 去重：`<数据目录>/games/seen.json` 记写过的 gameId；每次连上翻最近的对局列表，没记过的补写
//! - 暂停记录时不写；暂停期间结束的对局不记成已写，下次连上补

use base64::Engine;
use beayy_core::{data_root, fields, iso, EventLog};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 没连上时多久找一次客户端
pub const LOOK_EVERY: Duration = Duration::from_secs(30);
/// 连上后多久问一次阶段
pub const POLL_EVERY: Duration = Duration::from_secs(3);
/// 一局结束后取战绩：取不到时过多久再取（秒）
const RETRY_AFTER: [u64; 3] = [20, 120, 600];
/// 第一次连上最多往前补多少局
const BACKFILL_MAX: usize = 200;
const PAGE: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    pub port: u16,
    pub password: String,
    pub protocol: String,
}

/// lockfile：`LeagueClient:12345:54321:密码:https`。从后往前取，进程名里有冒号也不怕
pub fn parse_lockfile(s: &str) -> Option<Lock> {
    let p: Vec<&str> = s.trim().split(':').collect();
    let n = p.len();
    if n < 5 {
        return None;
    }
    let protocol = p[n - 1].to_string();
    if protocol != "https" && protocol != "http" {
        return None;
    }
    Some(Lock { port: p[n - 3].parse().ok()?, password: p[n - 2].to_string(), protocol })
}

/// 进程命令行里的 `--app-port=54321` `"--remoting-auth-token=xxx"`
pub fn parse_cmdline(s: &str) -> Option<Lock> {
    let arg = |k: &str| {
        let i = s.find(k)? + k.len();
        let v: String = s[i..].chars().take_while(|c| !c.is_whitespace() && *c != '"').collect();
        (!v.is_empty()).then_some(v)
    };
    Some(Lock { port: arg("--app-port=")?.parse().ok()?, password: arg("--remoting-auth-token=")?, protocol: "https".into() })
}

/// 本机接口。只连 127.0.0.1；证书是客户端自签的，不校验
pub struct Client {
    base: String,
    auth: String,
    agent: ureq::Agent,
}

impl Client {
    pub fn new(l: &Lock) -> Self {
        let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).disable_verification(true).build();
        let agent = ureq::Agent::config_builder().proxy(None).tls_config(tls).timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into();
        let auth = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", l.password)));
        Client { base: format!("{}://127.0.0.1:{}", l.protocol, l.port), auth, agent }
    }

    /// `Ok(None)`＝接口回 404（比如不在选人时取选人）
    pub fn get(&self, path: &str) -> Result<Option<Value>, String> {
        let mut r = self.agent.get(&format!("{}{path}", self.base)).header("Authorization", &self.auth).header("Accept", "application/json").call().map_err(|e| e.to_string())?;
        let st = r.status().as_u16();
        if st == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&st) {
            return Err(format!("{path} 回 {st}"));
        }
        let text = r.body_mut().with_config().limit(64 << 20).read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map(Some).map_err(|e| format!("{path}：{e}"))
    }
}

/// 找客户端，读出端口和密码
pub fn find_lock() -> Option<Lock> {
    if let Ok(p) = std::env::var("BEAYY_LOL_LOCKFILE") {
        return std::fs::read_to_string(p).ok().and_then(|s| parse_lockfile(&s));
    }
    #[cfg(target_os = "windows")]
    {
        for (pid, exe) in win::client_processes() {
            let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
            for d in [Some(dir.as_path()), dir.parent()].into_iter().flatten() {
                if let Some(l) = std::fs::read_to_string(d.join("lockfile")).ok().and_then(|s| parse_lockfile(&s)) {
                    return Some(l);
                }
            }
            if let Some(l) = win::cmdline_lock(pid) {
                return Some(l);
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        std::fs::read_to_string("/Applications/League of Legends.app/Contents/LoL/lockfile").ok().and_then(|s| parse_lockfile(&s))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::{parse_cmdline, Lock};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    const NO_WINDOW: u32 = 0x0800_0000;

    /// 在跑的 LeagueClientUx：进程号、程序路径
    pub fn client_processes() -> Vec<(u32, PathBuf)> {
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case("LeagueClientUx.exe") {
                    if let Some(p) = image_path(e.th32ProcessID) {
                        out.push((e.th32ProcessID, p));
                    }
                }
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    fn image_path(pid: u32) -> Option<PathBuf> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
        }
    }

    /// 没有 lockfile 时读命令行（PowerShell，同一个进程只读一次）。客户端以管理员身份跑时读不到
    static CACHE: Mutex<Option<(u32, Option<Lock>)>> = Mutex::new(None);

    pub fn cmdline_lock(pid: u32) -> Option<Lock> {
        let mut c = CACHE.lock().unwrap();
        if let Some((p, l)) = c.as_ref() {
            if *p == pid {
                return l.clone();
            }
        }
        let ps = format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine");
        let out = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &ps]).creation_flags(NO_WINDOW).output().ok();
        let l = out.and_then(|o| parse_cmdline(&String::from_utf8_lossy(&o.stdout)));
        *c = Some((pid, l.clone()));
        l
    }
}

fn i64_at(v: &Value, p: &str) -> i64 {
    v.pointer(p).and_then(Value::as_i64).unwrap_or(0)
}

/// 一局的摘要字段（自己那一行）。`puuid` 对不上时只有对局本身的字段
pub fn summarize(m: &Value, puuid: &str, champions: &BTreeMap<i64, String>, queues: &BTreeMap<i64, String>) -> Map<String, Value> {
    let queue_id = i64_at(m, "/queueId");
    let mut f = fields! {
        "game_id" => i64_at(m, "/gameId"),
        "platform_id" => m["platformId"],
        "queue_id" => queue_id,
        "map_id" => i64_at(m, "/mapId"),
        "game_mode" => m["gameMode"],
        "game_type" => m["gameType"],
        "game_version" => m["gameVersion"],
        "duration_s" => i64_at(m, "/gameDuration"),
    };
    if let Some(q) = queues.get(&queue_id) {
        f.insert("queue_name".into(), json!(q));
    }
    let empty = vec![];
    let ids = m["participantIdentities"].as_array().unwrap_or(&empty);
    let me = ids.iter().find(|i| !puuid.is_empty() && i.pointer("/player/puuid").and_then(Value::as_str) == Some(puuid)).map(|i| i64_at(i, "/participantId"));
    let parts = m["participants"].as_array().unwrap_or(&empty);
    let Some(p) = me.and_then(|id| parts.iter().find(|p| i64_at(p, "/participantId") == id)) else { return f };
    let team = i64_at(p, "/teamId");
    let s = &p["stats"];
    let champ = i64_at(p, "/championId");
    let team_kills: i64 = parts.iter().filter(|q| i64_at(q, "/teamId") == team).map(|q| i64_at(q, "/stats/kills")).sum();
    f.extend(fields! {
        "champion_id" => champ,
        "team_id" => team,
        "win" => s["win"].as_bool().unwrap_or(false),
        "kills" => i64_at(s, "/kills"),
        "deaths" => i64_at(s, "/deaths"),
        "assists" => i64_at(s, "/assists"),
        "team_kills" => team_kills,
        "cs" => i64_at(s, "/totalMinionsKilled") + i64_at(s, "/neutralMinionsKilled"),
        "damage" => i64_at(s, "/totalDamageDealtToChampions"),
        "gold" => i64_at(s, "/goldEarned"),
        "vision" => i64_at(s, "/visionScore"),
        "level" => i64_at(s, "/champLevel"),
        "lane" => p.pointer("/timeline/lane"),
        "role" => p.pointer("/timeline/role"),
    });
    if let Some(n) = champions.get(&champ) {
        f.insert("champion_name".into(), json!(n));
    }
    f
}

/// 选人快照：自己的位置和英雄、两边选的英雄、两边禁的
pub fn trim_champ_select(s: &Value) -> Value {
    let me = i64_at(s, "/localPlayerCellId");
    let team = |k: &str| -> Vec<Value> {
        s[k].as_array().map(|a| a.iter().map(|p| json!({"cell": p["cellId"], "champion_id": p["championId"], "position": p["assignedPosition"]})).collect()).unwrap_or_default()
    };
    let mine = s["myTeam"].as_array().and_then(|a| a.iter().find(|p| i64_at(p, "/cellId") == me)).cloned().unwrap_or(Value::Null);
    json!({
        "champion_id": mine["championId"],
        "position": mine["assignedPosition"],
        "my_team": team("myTeam"),
        "their_team": team("theirTeam"),
        "bans": s["bans"],
    })
}

/// 段位：每个队列的段位、胜点、胜负场
pub fn trim_rank(r: &Value) -> Value {
    let qs = r["queues"].as_array().map(|a| {
        a.iter()
            .filter(|q| q["queueType"].as_str().map(|t| !t.is_empty()).unwrap_or(false))
            .map(|q| json!({"queue": q["queueType"], "tier": q["tier"], "division": q["division"], "lp": q["leaguePoints"], "wins": q["wins"], "losses": q["losses"]}))
            .collect::<Vec<_>>()
    });
    json!(qs.unwrap_or_default())
}

fn ms_time(ms: i64) -> Option<DateTime<Utc>> {
    (ms > 0).then(|| Utc.timestamp_millis_opt(ms).single()).flatten()
}

/// 一局打完（离开对局阶段）
fn left_game(from: &str, to: &str) -> bool {
    matches!(from, "InProgress" | "Reconnect") && !matches!(to, "InProgress" | "Reconnect")
}

struct Pending {
    game_id: i64,
    tries: usize,
    due: Instant,
}

pub struct Watch {
    dir: PathBuf,
    log: Arc<EventLog>,
    paused: Arc<AtomicBool>,
    client: Option<Client>,
    fails: u32,
    puuid: String,
    phase: String,
    game_id: i64,
    queue_id: i64,
    champ_select: Value,
    seen: BTreeSet<i64>,
    rank: Value,
    pending: Vec<Pending>,
    champions: BTreeMap<i64, String>,
    queues: BTreeMap<i64, String>,
}

impl Watch {
    pub fn new(root: &Path, log: Arc<EventLog>, paused: Arc<AtomicBool>) -> Self {
        let dir = root.join("games");
        let saved: Value = std::fs::read(dir.join("seen.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        let seen = saved["games"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
        let names = |f: &str| -> BTreeMap<i64, String> {
            std::fs::read(dir.join(f)).ok().and_then(|b| serde_json::from_slice::<BTreeMap<String, String>>(&b).ok()).map(|m| m.into_iter().filter_map(|(k, v)| Some((k.parse().ok()?, v))).collect()).unwrap_or_default()
        };
        Watch {
            champions: names("champions.json"),
            queues: names("queues.json"),
            dir,
            log,
            paused,
            client: None,
            fails: 0,
            puuid: String::new(),
            phase: "None".into(),
            game_id: 0,
            queue_id: 0,
            champ_select: Value::Null,
            seen,
            rank: saved["rank"].clone(),
            pending: Vec::new(),
        }
    }

    pub fn connected(&self) -> bool {
        self.client.is_some()
    }

    fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    fn write(&self, t: &str, f: Map<String, Value>, at: Option<DateTime<Utc>>) {
        if !self.paused() {
            self.log.write(t, f, at);
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(&self.dir);
        let v = json!({"games": self.seen, "rank": self.rank});
        let tmp = self.dir.join("seen.json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, self.dir.join("seen.json"));
        }
    }

    /// 走一步，返回下一步之前等多久
    pub fn step(&mut self) -> Duration {
        if self.client.is_none() {
            match find_lock() {
                Some(l) => self.connect(&l),
                None => return LOOK_EVERY,
            }
            if self.client.is_none() {
                return LOOK_EVERY;
            }
        }
        match self.poll() {
            Ok(()) => {
                self.fails = 0;
                POLL_EVERY
            }
            Err(e) => {
                self.fails += 1;
                if self.fails >= 3 {
                    self.disconnect(&e);
                    return LOOK_EVERY;
                }
                POLL_EVERY
            }
        }
    }

    fn connect(&mut self, l: &Lock) {
        let c = Client::new(l);
        // lockfile 可能是客户端崩掉留下的；问不通就当没开
        let Ok(Some(me)) = c.get("/lol-summoner/v1/current-summoner") else { return };
        self.puuid = me["puuid"].as_str().unwrap_or("").to_string();
        let version = c.get("/lol-patch/v1/game-version").ok().flatten();
        let region = c.get("/riotclient/region-locale").ok().flatten().unwrap_or(Value::Null);
        self.client = Some(c);
        self.fails = 0;
        self.write(
            "game.client",
            fields! {
                "state" => "connected",
                "puuid" => self.puuid,
                "game_name" => me["gameName"],
                "tag_line" => me["tagLine"],
                "display_name" => me["displayName"],
                "level" => me["summonerLevel"],
                "region" => region["region"],
                "locale" => region["locale"],
                "version" => version,
            },
            None,
        );
        self.load_names();
        self.check_rank();
        self.backfill();
    }

    fn disconnect(&mut self, why: &str) {
        self.write("game.client", fields! {"state" => "disconnected", "puuid" => self.puuid, "reason" => why}, None);
        self.client = None;
        self.phase = "None".into();
        self.champ_select = Value::Null;
    }

    fn get(&self, path: &str) -> Result<Option<Value>, String> {
        self.client.as_ref().ok_or("没连上")?.get(path)
    }

    /// 英雄名、队列名用客户端自带的（跟客户端语言），缓存在本机
    fn load_names(&mut self) {
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(Some(Value::Array(a))) = self.get("/lol-game-data/assets/v1/champion-summary.json") {
            let m: BTreeMap<i64, String> = a.iter().filter_map(|c| Some((c["id"].as_i64()?, c["name"].as_str()?.to_string()))).filter(|(id, _)| *id > 0).collect();
            if !m.is_empty() {
                let _ = std::fs::write(self.dir.join("champions.json"), serde_json::to_vec(&m).unwrap_or_default());
                self.champions = m;
            }
        }
        if let Ok(Some(Value::Array(a))) = self.get("/lol-game-queues/v1/queues") {
            let m: BTreeMap<i64, String> = a
                .iter()
                .filter_map(|q| {
                    let n = [&q["description"], &q["name"], &q["shortName"]].into_iter().filter_map(Value::as_str).find(|s| !s.is_empty())?;
                    Some((q["id"].as_i64()?, n.to_string()))
                })
                .collect();
            if !m.is_empty() {
                let _ = std::fs::write(self.dir.join("queues.json"), serde_json::to_vec(&m).unwrap_or_default());
                self.queues = m;
            }
        }
    }

    fn poll(&mut self) -> Result<(), String> {
        let phase = self.get("/lol-gameflow/v1/gameflow-phase")?.and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "None".into());
        if phase == "ChampSelect" {
            if let Ok(Some(s)) = self.get("/lol-champ-select/v1/session") {
                self.champ_select = s;
            }
        }
        if phase != self.phase {
            self.on_phase(phase);
        }
        self.due_matches();
        Ok(())
    }

    fn on_phase(&mut self, to: String) {
        let from = std::mem::replace(&mut self.phase, to.clone());
        let s = self.get("/lol-gameflow/v1/session").ok().flatten().unwrap_or(Value::Null);
        let (id, q) = (i64_at(&s, "/gameData/gameId"), i64_at(&s, "/gameData/queue/id"));
        // 刚打完时会话可能已经清掉：沿用对局里拿到的那个
        let ending = left_game(&from, &to) || matches!(from.as_str(), "WaitingForStats" | "PreEndOfGame");
        if id > 0 || !ending {
            self.game_id = id;
        }
        if q != 0 || !ending {
            self.queue_id = q;
        }
        let mut f = fields! {"phase" => to, "from" => from, "queue_id" => self.queue_id, "game_id" => self.game_id};
        if from == "ChampSelect" && to != "ChampSelect" && !self.champ_select.is_null() {
            f.insert("champ_select".into(), trim_champ_select(&self.champ_select));
            self.champ_select = Value::Null;
        }
        self.write("game.phase", f, None);
        if left_game(&from, &to) && self.game_id > 0 && !self.seen.contains(&self.game_id) && !self.pending.iter().any(|p| p.game_id == self.game_id) {
            self.pending.push(Pending { game_id: self.game_id, tries: 0, due: Instant::now() + Duration::from_secs(RETRY_AFTER[0]) });
        }
    }

    fn due_matches(&mut self) {
        let now = Instant::now();
        let mut keep = Vec::new();
        let mut ranked = false;
        for mut p in std::mem::take(&mut self.pending) {
            if p.due > now {
                keep.push(p);
                continue;
            }
            match self.fetch_match(p.game_id) {
                Ok(Some(q)) => ranked |= matches!(q, 420 | 440),
                _ => {
                    p.tries += 1;
                    if let Some(s) = RETRY_AFTER.get(p.tries) {
                        p.due = now + Duration::from_secs(*s);
                        keep.push(p);
                    }
                }
            }
        }
        self.pending.extend(keep);
        if ranked {
            self.check_rank();
        }
    }

    /// 取一局的完整战绩写成 game.match；返回队列号。`Ok(None)`＝还没有（战绩没出来、或者不是会进战绩的局）
    fn fetch_match(&mut self, id: i64) -> Result<Option<i64>, String> {
        let Some(m) = self.get(&format!("/lol-match-history/v1/games/{id}"))? else { return Ok(None) };
        if i64_at(&m, "/gameId") != id {
            return Ok(None);
        }
        let mut f = summarize(&m, &self.puuid, &self.champions, &self.queues);
        let q = i64_at(&m, "/queueId");
        let at = ms_time(i64_at(&m, "/gameCreation"));
        if let Some(t) = at {
            f.insert("started_at".into(), json!(iso(t)));
        }
        f.insert("raw".into(), m);
        if self.paused() {
            return Ok(Some(q));
        }
        self.log.write("game.match", f, at);
        self.seen.insert(id);
        self.save();
        Ok(Some(q))
    }

    /// 翻最近的对局列表，没记过的补写（旧的先写）
    fn backfill(&mut self) {
        let first = self.seen.is_empty();
        let mut todo: Vec<(i64, i64)> = Vec::new();
        let mut beg = 0;
        while beg < BACKFILL_MAX {
            let Ok(Some(v)) = self.get(&format!("/lol-match-history/v1/products/lol/current-summoner/matches?begIndex={beg}&endIndex={}", beg + PAGE)) else { break };
            let games = v.pointer("/games/games").and_then(Value::as_array).cloned().unwrap_or_default();
            if games.is_empty() {
                break;
            }
            let mut fresh = 0;
            for g in &games {
                let id = i64_at(g, "/gameId");
                if id > 0 && !self.seen.contains(&id) && !todo.iter().any(|t| t.0 == id) {
                    todo.push((id, i64_at(g, "/gameCreation")));
                    fresh += 1;
                }
            }
            // 不是第一次：翻到全是记过的那页就停
            if fresh == 0 && !first || games.len() < PAGE {
                break;
            }
            beg += PAGE;
        }
        todo.sort_by_key(|t| t.1);
        for (id, _) in todo {
            if self.fetch_match(id).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    fn check_rank(&mut self) {
        let Ok(Some(r)) = self.get("/lol-ranked/v1/current-ranked-stats") else { return };
        let t = trim_rank(&r);
        if t == self.rank || self.paused() {
            return;
        }
        self.log.write("game.rank", fields! {"queues" => t, "raw" => r}, None);
        self.rank = t;
        self.save();
    }
}

/// 起后台线程：找客户端、记阶段和每一局
pub fn start(log: Arc<EventLog>, paused: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut w = Watch::new(&data_root(), log, paused);
        loop {
            let wait = w.step();
            std::thread::sleep(wait);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_and_cmdline() {
        assert_eq!(parse_lockfile("LeagueClient:1234:56789:abc-DEF_1:https\n"), Some(Lock { port: 56789, password: "abc-DEF_1".into(), protocol: "https".into() }));
        assert_eq!(parse_lockfile("坏的"), None);
        let cmd = r#""D:\Games\LeagueClient\LeagueClientUx.exe" "--riotclient-auth-token=x" "--app-port=61234" "--remoting-auth-token=Zk9-q_1" "--locale=zh_CN""#;
        assert_eq!(parse_cmdline(cmd), Some(Lock { port: 61234, password: "Zk9-q_1".into(), protocol: "https".into() }));
        assert_eq!(parse_cmdline("--app-port=1"), None);
    }

    #[test]
    fn summarize_me() {
        let m = json!({
            "gameId": 7, "queueId": 420, "mapId": 11, "gameMode": "CLASSIC", "gameDuration": 1830, "gameCreation": 1_762_400_000_000i64,
            "participantIdentities": [{"participantId": 1, "player": {"puuid": "a"}}, {"participantId": 2, "player": {"puuid": "me"}}],
            "participants": [
                {"participantId": 1, "teamId": 100, "championId": 1, "stats": {"kills": 3}},
                {"participantId": 2, "teamId": 100, "championId": 99, "stats": {"win": true, "kills": 5, "deaths": 2, "assists": 7, "totalMinionsKilled": 150, "neutralMinionsKilled": 10}, "timeline": {"lane": "MIDDLE"}},
                {"participantId": 6, "teamId": 200, "championId": 2, "stats": {"kills": 9}}
            ]
        });
        let champs = BTreeMap::from([(99, "光辉女郎".to_string())]);
        let f = summarize(&m, "me", &champs, &BTreeMap::new());
        assert_eq!(f["champion_name"], "光辉女郎");
        assert_eq!(f["win"], true);
        assert_eq!(f["cs"], 160);
        assert_eq!(f["team_kills"], 8);
        assert_eq!(f["lane"], "MIDDLE");
        // 认不出自己：只有对局字段
        assert!(summarize(&m, "", &champs, &BTreeMap::new()).get("champion_id").is_none());
    }

    #[test]
    fn phases() {
        assert!(left_game("InProgress", "WaitingForStats"));
        assert!(left_game("Reconnect", "None"));
        assert!(!left_game("InProgress", "Reconnect"));
        assert!(!left_game("ChampSelect", "Lobby"));
    }
}
