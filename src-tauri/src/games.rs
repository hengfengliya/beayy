//! 游戏：按游戏分开汇总，英雄联盟（lol）和 Steam（steam）；再加一款就在 GAMES 加一项、写它自己的汇总。
//! 英雄联盟：各台 Windows 电脑上 crates/beayy-games 记的 game.* 事件随分段同步过来，这里汇成对局、每天、英雄、段位。
//! 设置 → 功能 → 游戏（settings.json 的 games）默认开，关掉后：关着采集端不读客户端、侧栏不出游戏页，已记的照样能读。
//! 方案见 报告/方案_20261106_英雄联盟接入.md。都是读的时候现算、不另存；只看 win- 开头的设备（Mac 版客户端以后再接）。
//! - 一局：game.match（对局开始时刻、自己那一行的统计、raw＝客户端原样返回的 10 个人）。
//! - 排队、选人、读条多久：game.phase 的阶段变化，同一台电脑上从离开大厅到回到大厅算一轮，轮里带着 gameId 的就是那一局；
//!   没进成游戏的一轮（取消排队、有人秒退）只算进那天的等待。补写的历史局没有阶段。
//! - 段位：game.rank 每次变了才写；排位局前后两份一比就是这局加减的胜点。
//! Steam：steam.session（start 开始、end 结束带时长），steam.library（每款总时长，变了才写）。见 steam_overview。

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Timelike, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const TYPES: [&str; 6] = ["game.match", "game.rank", "game.client", "game.phase", "steam.session", "steam.library"];
/// 不到 5 分钟就结束的算重开（有人没进来），不计胜负
const REMAKE_S: i64 = 300;
/// 两次阶段变化隔得太久（电脑睡了、客户端挂着没人管），这段不算
const PHASE_MAX_S: i64 = 3 * 3600;
/// 进游戏之前的阶段；一轮里已经打过一局又回到这些，就是下一局了
const PRE: [&str; 3] = ["Matchmaking", "ReadyCheck", "ChampSelect"];

fn ts(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn events() -> Vec<Value> {
    let from = Utc::now() - Duration::days(365 * 20);
    let mut ev = crate::data::device_events("win-", &TYPES, from, Utc::now() + Duration::days(1));
    ev.sort_by(|a, b| a["timestamp"].as_str().unwrap_or("").cmp(b["timestamp"].as_str().unwrap_or("")));
    ev
}

/// 同一局可能从两台电脑来、也可能补写过：按 gameId 留一份，有 raw 的优先
fn matches_of(ev: &[Value]) -> Vec<&Value> {
    let mut by: BTreeMap<i64, &Value> = BTreeMap::new();
    for e in ev.iter().filter(|e| e["event_type"] == "game.match") {
        let id = int(&e["game_id"]);
        if id <= 0 {
            continue;
        }
        if by.get(&id).is_none_or(|old| !old["raw"].is_object() && e["raw"].is_object()) {
            by.insert(id, e);
        }
    }
    let mut v: Vec<&Value> = by.into_values().collect();
    v.sort_by_key(|e| start_of(e));
    v
}

fn start_of(e: &Value) -> Option<DateTime<Utc>> {
    ts(&e["started_at"]).or_else(|| ts(&e["timestamp"]))
}

/// 自己的账号（各次连上客户端时记的 puuid）
fn my_puuids(ev: &[Value]) -> BTreeSet<String> {
    ev.iter()
        .filter(|e| e["event_type"] == "game.client" || e["event_type"] == "game.match")
        .filter_map(|e| e["puuid"].as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// 英雄编号 → 名字：连上客户端时带的整张表（旧的在前、新的盖上），每局带的同局 10 个英雄名，自己用过的
fn champ_names(ev: &[Value], ms: &[&Value]) -> HashMap<i64, String> {
    let mut m = HashMap::new();
    for e in ev.iter().filter(|e| e["event_type"] == "game.client").chain(ms.iter().copied()) {
        for (k, v) in e["champions"].as_object().into_iter().flatten() {
            if let (Ok(id), Some(n)) = (k.parse::<i64>(), v.as_str()) {
                m.insert(id, n.to_string());
            }
        }
    }
    for e in ms {
        if let Some(o) = e["champion_names"].as_object() {
            for (k, v) in o {
                if let (Ok(id), Some(n)) = (k.parse::<i64>(), v.as_str()) {
                    m.insert(id, n.to_string());
                }
            }
        }
        if let Some(n) = e["champion_name"].as_str() {
            m.insert(int(&e["champion_id"]), n.to_string());
        }
    }
    m
}

/// 这局里自己那一行：先认 puuid，再按 队伍＋英雄 认
fn me_in<'a>(raw: &'a Value, puuids: &BTreeSet<String>, team: i64, champ: i64) -> Option<&'a Value> {
    let ps = raw["participants"].as_array()?;
    let pid = raw["participantIdentities"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["player"]["puuid"].as_str().is_some_and(|p| puuids.contains(p))))
        .and_then(|x| x["participantId"].as_i64());
    pid.and_then(|id| ps.iter().find(|p| p["participantId"].as_i64() == Some(id)))
        .or_else(|| ps.iter().find(|p| int(&p["teamId"]) == team && int(&p["championId"]) == champ))
}

fn dmg_of(p: &Value) -> i64 {
    int(&p["stats"]["totalDamageDealtToChampions"])
}

/// 一轮阶段：从离开大厅到回到大厅
#[derive(Default)]
struct Run {
    start: Option<DateTime<Utc>>,
    game: i64,
    by: HashMap<String, i64>,
    champ_select: Value,
}

impl Run {
    fn sum(&self, keys: &[&str]) -> i64 {
        keys.iter().map(|k| self.by.get(*k).copied().unwrap_or(0)).sum()
    }
    fn wait(&self) -> i64 {
        self.sum(&["Matchmaking", "ReadyCheck", "ChampSelect", "GameStart"])
    }
}

fn runs_of(ev: &[Value]) -> Vec<Run> {
    let mut by_dev: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in ev.iter().filter(|e| e["event_type"] == "game.phase" || e["event_type"] == "game.client") {
        by_dev.entry(e["device"].as_str().unwrap_or("")).or_default().push(e);
    }
    let mut out = vec![];
    for list in by_dev.into_values() {
        let mut cur: Option<Run> = None;
        for (i, e) in list.iter().enumerate() {
            let t = ts(&e["timestamp"]);
            let phase = e["phase"].as_str().unwrap_or("");
            // 客户端连上、断开，回到大厅：这一轮完了
            if e["event_type"] == "game.client" || phase == "None" || phase == "Lobby" || phase.is_empty() {
                out.extend(cur.take());
                continue;
            }
            if PRE.contains(&phase) && cur.as_ref().is_some_and(|r| r.by.contains_key("InProgress")) {
                out.extend(cur.take());
            }
            let secs = match (t, list.get(i + 1).and_then(|n| ts(&n["timestamp"]))) {
                (Some(a), Some(b)) => (b - a).num_seconds().clamp(0, PHASE_MAX_S),
                _ => 0,
            };
            let r = cur.get_or_insert_with(|| Run { start: t, ..Default::default() });
            if int(&e["game_id"]) > 0 {
                r.game = int(&e["game_id"]);
            }
            *r.by.entry(phase.to_string()).or_default() += secs;
            if e["champ_select"].is_object() {
                r.champ_select = e["champ_select"].clone();
            }
        }
        out.extend(cur.take());
    }
    out
}

const RANK_QUEUE: [(i64, &str); 2] = [(420, "RANKED_SOLO_5x5"), (440, "RANKED_FLEX_SR")];

fn rank_in<'a>(r: &'a Value, queue: &str) -> Option<&'a Value> {
    r["queues"].as_array()?.iter().find(|q| q["queue"] == queue)
}

/// 排位局前后两份段位一比：同一段就是加减的胜点，换了段写换到哪
fn rank_change(ranks: &[(DateTime<Utc>, &Value)], queue_id: i64, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<Value> {
    let q = RANK_QUEUE.iter().find(|(id, _)| *id == queue_id)?.1;
    let before = ranks.iter().rev().find(|(t, _)| *t <= start).and_then(|(_, r)| rank_in(r, q))?;
    let after = ranks.iter().find(|(t, _)| *t >= end && *t <= end + Duration::hours(2)).and_then(|(_, r)| rank_in(r, q))?;
    if before["tier"] == after["tier"] && before["division"] == after["division"] {
        Some(json!({ "lp": int(&after["lp"]) - int(&before["lp"]) }))
    } else {
        Some(json!({ "to": { "tier": after["tier"], "division": after["division"], "lp": after["lp"] } }))
    }
}

fn is_night(t: DateTime<Utc>) -> bool {
    let h = t.with_timezone(&Local).hour();
    h >= 23 || h < 5
}

/// 一局给页面的样子（不带 raw）
fn light(e: &Value, puuids: &BTreeSet<String>, run: Option<&Run>, ranks: &[(DateTime<Utc>, &Value)]) -> Value {
    let raw = &e["raw"];
    let dur = int(&e["duration_s"]);
    let (team, champ) = (int(&e["team_id"]), int(&e["champion_id"]));
    let team_dmg: i64 = raw["participants"].as_array().into_iter().flatten().filter(|p| int(&p["teamId"]) == team).map(dmg_of).sum();
    let me = me_in(raw, puuids, team, champ);
    let (k, a, tk, dmg) = (int(&e["kills"]), int(&e["assists"]), int(&e["team_kills"]), int(&e["damage"]));
    let mut v = json!({
        "id": e["game_id"], "start": start_of(e).map(beayy_core::iso), "dur": dur, "device": e["device"],
        "queue_id": e["queue_id"], "queue": e["queue_name"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(Value::from).unwrap_or_else(|| e["game_mode"].clone()),
        "mode": e["game_mode"], "map": e["map_id"], "version": e["game_version"],
        "champ_id": champ, "champ": e["champion_name"], "win": e["win"], "remake": dur > 0 && dur < REMAKE_S,
        "k": k, "d": e["deaths"], "a": a, "cs": e["cs"], "dmg": dmg, "gold": e["gold"], "vision": e["vision"], "level": e["level"],
        "lane": e["lane"], "role": e["role"],
        "kp": if tk > 0 { json!((k + a) as f64 / tk as f64) } else { Value::Null },
        "dmg_share": if team_dmg > 0 { json!(dmg as f64 / team_dmg as f64) } else { Value::Null },
        "spells": me.map(|p| json!([p["spell1Id"], p["spell2Id"]])),
        "night": start_of(e).is_some_and(is_night),
    });
    if let Some(r) = run {
        v["phases"] = json!({ "queue": r.sum(&["Matchmaking", "ReadyCheck"]), "select": r.sum(&["ChampSelect"]), "load": r.sum(&["GameStart"]), "post": r.sum(&["WaitingForStats", "PreEndOfGame", "EndOfGame"]) });
        if let Some(p) = r.champ_select["position"].as_str().filter(|s| !s.is_empty()) {
            v["position"] = json!(p);
        }
    }
    if let (Some(s), Some(q)) = (start_of(e), e["queue_id"].as_i64()) {
        if let Some(c) = rank_change(ranks, q, s, s + Duration::seconds(dur)) {
            v["rank"] = c;
        }
    }
    v
}

#[derive(Default)]
struct Day {
    n: i64,
    w: i64,
    remakes: i64,
    play: i64,
    wait: i64,
    night: i64,
    games: Vec<Value>,
}

/// 支持的游戏
const GAMES: [&str; 2] = ["lol", "steam"];

/// 有哪些游戏、各记了多少（局、次）和最近一次；没记过的不列
fn games_list(ms: &[&Value], ss: &[Session]) -> Vec<Value> {
    let mut v = vec![];
    if !ms.is_empty() {
        v.push(json!({ "id": "lol", "n": ms.len(), "last": ms.last().and_then(|e| start_of(e)).map(beayy_core::iso) }));
    }
    if !ss.is_empty() {
        v.push(json!({ "id": "steam", "n": ss.len(), "last": ss.iter().map(|s| s.start).max().map(beayy_core::iso) }));
    }
    v
}

/// 游戏页：有哪些游戏（games），和选的那款的数据。没指定就选最近玩过的那款
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn games_overview(game: Option<String>) -> Value {
    let ev = events();
    let ms = matches_of(&ev);
    let ss = sessions_of(&ev, Utc::now());
    let games = games_list(&ms, &ss);
    let latest = games.iter().max_by_key(|g| g["last"].as_str().unwrap_or("").to_string()).and_then(|g| g["id"].as_str()).unwrap_or("lol").to_string();
    let game = game.filter(|g| GAMES.contains(&g.as_str())).unwrap_or(latest);
    if game == "steam" {
        return steam_overview(&ev, ss, games);
    }
    let puuids = my_puuids(&ev);
    let runs = runs_of(&ev);
    let by_game: HashMap<i64, &Run> = runs.iter().filter(|r| r.game > 0).map(|r| (r.game, r)).collect();
    let ranks: Vec<(DateTime<Utc>, &Value)> = ev.iter().filter(|e| e["event_type"] == "game.rank").filter_map(|e| Some((ts(&e["timestamp"])?, e))).collect();

    let mut days: BTreeMap<String, Day> = BTreeMap::new();
    let mut list = vec![];
    for e in &ms {
        let m = light(e, &puuids, by_game.get(&int(&e["game_id"])).copied(), &ranks);
        let Some(start) = start_of(e) else { continue };
        let d = days.entry(start.with_timezone(&Local).format("%Y-%m-%d").to_string()).or_default();
        let dur = int(&m["dur"]);
        d.play += dur;
        d.night += m["night"].as_bool().unwrap_or(false) as i64;
        d.games.push(json!({ "id": m["id"], "start": m["start"], "dur": dur, "win": m["win"], "remake": m["remake"] }));
        if m["remake"] == true {
            d.remakes += 1;
        } else if m["win"].is_boolean() {
            d.n += 1;
            d.w += (m["win"] == true) as i64;
        }
        list.push(m);
    }
    for r in &runs {
        if let Some(t) = r.start {
            days.entry(t.with_timezone(&Local).format("%Y-%m-%d").to_string()).or_default().wait += r.wait();
        }
    }
    list.reverse();
    let client = ev.iter().rev().find(|e| e["event_type"] == "game.client" && e["state"] == "connected");
    let account = client.map(|c| {
        json!({
            "name": c["game_name"], "tag": c["tag_line"], "level": c["level"], "region": c["region"],
            "platform": ms.last().map(|m| m["platform_id"].clone()).unwrap_or(Value::Null), "at": c["timestamp"], "device": c["device"],
        })
    });
    let rank = ranks.last().map(|(t, r)| {
        let qs: Vec<&Value> = r["queues"].as_array().into_iter().flatten().filter(|q| q["tier"].as_str().is_some_and(|s| !s.is_empty() && s != "NONE")).collect();
        json!({ "at": beayy_core::iso(*t), "queues": qs })
    });
    json!({
        "game": "lol",
        "games": games,
        "account": account,
        "rank": rank,
        "matches": list,
        "days": days.into_iter().rev().map(|(date, d)| json!({ "date": date, "n": d.n, "w": d.w, "remakes": d.remakes, "play": d.play, "wait": d.wait, "night": d.night, "games": d.games })).collect::<Vec<_>>(),
    })
}

/// 一局的 10 个人：自己那队在前
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn get_game(id: i64) -> Value {
    let ev = events();
    let ms = matches_of(&ev);
    let Some(e) = ms.iter().find(|e| int(&e["game_id"]) == id) else { return Value::Null };
    let puuids = my_puuids(&ev);
    let names = champ_names(&ev, &ms);
    let runs = runs_of(&ev);
    let run = runs.iter().find(|r| r.game == id);
    let ranks: Vec<(DateTime<Utc>, &Value)> = ev.iter().filter(|e| e["event_type"] == "game.rank").filter_map(|e| Some((ts(&e["timestamp"])?, e))).collect();
    let raw = &e["raw"];
    let me = me_in(raw, &puuids, int(&e["team_id"]), int(&e["champion_id"])).and_then(|p| p["participantId"].as_i64());
    let who: HashMap<i64, &Value> = raw["participantIdentities"].as_array().into_iter().flatten().filter_map(|x| Some((x["participantId"].as_i64()?, &x["player"]))).collect();
    let name = |cid: i64| names.get(&cid).cloned().map(Value::from).unwrap_or(Value::Null);
    let mut teams: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for p in raw["participants"].as_array().into_iter().flatten() {
        let s = &p["stats"];
        let pid = int(&p["participantId"]);
        let pl = who.get(&pid).copied().unwrap_or(&Value::Null);
        let nick = pl["gameName"].as_str().filter(|s| !s.is_empty()).or_else(|| pl["summonerName"].as_str()).unwrap_or("");
        teams.entry(int(&p["teamId"])).or_default().push(json!({
            "name": nick, "tag": pl["tagLine"], "me": Some(pid) == me,
            "champ_id": p["championId"], "champ": name(int(&p["championId"])), "level": s["champLevel"],
            "k": s["kills"], "d": s["deaths"], "a": s["assists"], "cs": int(&s["totalMinionsKilled"]) + int(&s["neutralMinionsKilled"]),
            "dmg": dmg_of(p), "gold": s["goldEarned"], "vision": s["visionScore"], "win": s["win"],
        }));
    }
    let my_team = int(&e["team_id"]);
    let mut out: Vec<Value> = teams
        .into_iter()
        .map(|(tid, players)| {
            let t = raw["teams"].as_array().and_then(|a| a.iter().find(|t| int(&t["teamId"]) == tid)).cloned().unwrap_or(Value::Null);
            let sum = |k: &str| players.iter().map(|p| int(&p[k])).sum::<i64>();
            json!({
                "id": tid, "mine": tid == my_team, "win": t["win"] == "Win",
                "kills": sum("k"), "dmg": sum("dmg"), "gold": sum("gold"),
                "bans": t["bans"].as_array().into_iter().flatten().filter(|b| int(&b["championId"]) > 0).map(|b| json!({ "id": b["championId"], "name": name(int(&b["championId"])) })).collect::<Vec<_>>(),
                "players": players,
            })
        })
        .collect();
    out.sort_by_key(|t| !t["mine"].as_bool().unwrap_or(false));
    json!({ "match": light(e, &puuids, run, &ranks), "teams": out })
}

// ---------- Steam ----------

/// 只有开始没有结束的那次（还在玩）：最多算这么久，再久多半是 Beayy 被关了、没来得及写结束
const LIVE_MAX_S: i64 = 12 * 3600;

/// 玩一次：开始、多久、哪款
struct Session {
    appid: i64,
    name: Option<String>,
    start: DateTime<Utc>,
    dur: i64,
    device: String,
    live: bool,
}

/// 每次：结束那条带开始和时长；某台电脑最后一条是开始、还没结束的，算到现在（标 live）
fn sessions_of(ev: &[Value], now: DateTime<Utc>) -> Vec<Session> {
    let key = |e: &Value| (e["device"].as_str().unwrap_or("").to_string(), e["started_at"].as_str().unwrap_or("").to_string(), int(&e["appid"]));
    let mut by: BTreeMap<(String, String, i64), Session> = BTreeMap::new();
    let mut last: HashMap<&str, &Value> = HashMap::new();
    for e in ev.iter().filter(|e| e["event_type"] == "steam.session") {
        last.insert(e["device"].as_str().unwrap_or(""), e);
        if e["state"] != "end" {
            continue;
        }
        let Some(start) = ts(&e["started_at"]) else { continue };
        by.insert(key(e), Session { appid: int(&e["appid"]), name: e["name"].as_str().map(String::from), start, dur: int(&e["duration_s"]).max(0), device: e["device"].as_str().unwrap_or("").into(), live: false });
    }
    for e in last.into_values().filter(|e| e["state"] == "start") {
        let Some(start) = ts(&e["started_at"]) else { continue };
        let dur = (now - start).num_seconds();
        if !by.contains_key(&key(e)) && (0..LIVE_MAX_S).contains(&dur) {
            by.insert(key(e), Session { appid: int(&e["appid"]), name: e["name"].as_str().map(String::from), start, dur, device: e["device"].as_str().unwrap_or("").into(), live: true });
        }
    }
    let mut v: Vec<Session> = by.into_values().collect();
    v.sort_by_key(|s| s.start);
    v
}

fn local_at(d: NaiveDate, h: u32) -> Option<DateTime<Utc>> {
    Local.from_local_datetime(&d.and_hms_opt(h, 0, 0)?).earliest().map(|t| t.with_timezone(&Utc))
}

/// [s, e) 里落在本地 23:00–5:00 的秒数
fn night_secs(s: DateTime<Utc>, e: DateTime<Utc>) -> i64 {
    let mut total = 0;
    let (mut d, end) = (s.with_timezone(&Local).date_naive() - Duration::days(1), e.with_timezone(&Local).date_naive());
    while d <= end {
        if let (Some(a), Some(b)) = (local_at(d, 23), d.succ_opt().and_then(|n| local_at(n, 5))) {
            total += (e.min(b) - s.max(a)).num_seconds().max(0);
        }
        let Some(n) = d.succ_opt() else { break };
        d = n;
    }
    total
}

/// Steam：每次（新的在前）、每天、每款的总时长（游戏库里 Steam 自己记的，各台电脑各账号取最新那份、同一款取大的）
fn steam_overview(ev: &[Value], ss: Vec<Session>, games: Vec<Value>) -> Value {
    let mut latest: BTreeMap<(String, i64), &Value> = BTreeMap::new();
    for e in ev.iter().filter(|e| e["event_type"] == "steam.library") {
        latest.insert((e["device"].as_str().unwrap_or("").to_string(), int(&e["account_id"])), e);
    }
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut lib: BTreeMap<i64, (i64, String)> = BTreeMap::new();
    for e in latest.values() {
        for a in e["apps"].as_array().into_iter().flatten() {
            let id = int(&a["appid"]);
            if let Some(n) = a["name"].as_str() {
                names.insert(id, n.to_string());
            }
            let x = lib.entry(id).or_default();
            x.0 = x.0.max(int(&a["playtime_min"]));
            x.1 = x.1.clone().max(a["last_played"].as_str().unwrap_or("").to_string());
        }
    }
    for s in &ss {
        if let Some(n) = &s.name {
            names.insert(s.appid, n.clone());
        }
    }
    let name = |id: i64| names.get(&id).cloned().map(Value::from).unwrap_or(Value::Null);
    let mut days: BTreeMap<String, (i64, i64, Vec<Value>)> = BTreeMap::new();
    let mut list = vec![];
    for s in &ss {
        let night = night_secs(s.start, s.start + Duration::seconds(s.dur));
        let start = beayy_core::iso(s.start);
        let d = days.entry(s.start.with_timezone(&Local).format("%Y-%m-%d").to_string()).or_default();
        d.0 += s.dur;
        d.1 += night;
        d.2.push(json!({ "appid": s.appid, "name": name(s.appid), "start": start, "dur": s.dur }));
        list.push(json!({ "appid": s.appid, "name": name(s.appid), "start": start, "dur": s.dur, "device": s.device, "live": s.live, "night_s": night }));
    }
    list.reverse();
    let mut library: Vec<Value> = lib.into_iter().map(|(id, (min, last))| json!({ "appid": id, "name": name(id), "total_min": min, "last": (!last.is_empty()).then_some(last) })).collect();
    library.sort_by_key(|a| -int(&a["total_min"]));
    let account = latest.values().max_by_key(|e| e["timestamp"].as_str().unwrap_or("").to_string()).and_then(|e| e["persona"].as_str()).map(|p| json!({ "name": p }));
    json!({
        "game": "steam",
        "games": games,
        "account": account,
        "sessions": list,
        "days": days.into_iter().rev().map(|(date, (play, night, g))| json!({ "date": date, "n": g.len(), "play": play, "night_s": night, "games": g })).collect::<Vec<_>>(),
        "library": library,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_sessions() {
        let now = Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap();
        let es = vec![
            json!({"event_type": "steam.session", "device": "win-a", "timestamp": "2026-11-06T08:00:00.000Z", "state": "start", "appid": 10, "name": "雾港", "started_at": "2026-11-06T08:00:00.000Z"}),
            json!({"event_type": "steam.session", "device": "win-a", "timestamp": "2026-11-06T09:00:00.000Z", "state": "end", "appid": 10, "name": "雾港", "started_at": "2026-11-06T08:00:00.000Z", "duration_s": 3600}),
            // 同一次又同步来一份：只算一次
            json!({"event_type": "steam.session", "device": "win-a", "timestamp": "2026-11-06T09:00:00.000Z", "state": "end", "appid": 10, "name": "雾港", "started_at": "2026-11-06T08:00:00.000Z", "duration_s": 3600}),
            // 还在玩
            json!({"event_type": "steam.session", "device": "win-a", "timestamp": "2026-11-06T11:30:00.000Z", "state": "start", "appid": 20, "started_at": "2026-11-06T11:30:00.000Z"}),
            // 另一台很久以前开始、没写结束：不算
            json!({"event_type": "steam.session", "device": "win-b", "timestamp": "2026-11-01T11:30:00.000Z", "state": "start", "appid": 30, "started_at": "2026-11-01T11:30:00.000Z"}),
            json!({"event_type": "steam.library", "device": "win-a", "timestamp": "2026-11-06T09:01:00.000Z", "account_id": 42, "persona": "旧人", "apps": [{"appid": 10, "name": "雾港", "playtime_min": 600}, {"appid": 20, "name": "Lantern", "playtime_min": 30}]}),
        ];
        let ss = sessions_of(&es, now);
        assert_eq!(ss.iter().map(|s| (s.appid, s.dur, s.live)).collect::<Vec<_>>(), vec![(10, 3600, false), (20, 1800, true)]);
        let v = steam_overview(&es, ss, vec![]);
        assert_eq!(v["sessions"][0]["name"], "Lantern");
        assert_eq!(v["library"][0]["total_min"], 600);
        assert_eq!(v["account"]["name"], "旧人");
        assert_eq!(v["days"].as_array().unwrap().iter().map(|d| int(&d["play"])).sum::<i64>(), 5400);
    }

    #[test]
    fn night() {
        let d = Local::now().date_naive();
        let at = |h: u32, m: u32| Local.from_local_datetime(&d.and_hms_opt(h, m, 0).unwrap()).earliest().unwrap().with_timezone(&Utc);
        // 22:00 到次日 1:00：深夜两小时
        assert_eq!(night_secs(at(22, 0), at(22, 0) + Duration::hours(3)), 7200);
        assert_eq!(night_secs(at(12, 0), at(13, 0)), 0);
        // 凌晨 4:30 到 6:00：半小时
        assert_eq!(night_secs(at(4, 30), at(6, 0)), 1800);
    }
}
