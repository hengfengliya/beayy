//! 游戏：按游戏分开汇总，现在只有英雄联盟（lol）；以后加 Steam 等就在 GAMES 加一项、写它自己的汇总。
//! 英雄联盟：各台 Windows 电脑上 crates/beayy-games 记的 game.* 事件随分段同步过来，这里汇成对局、每天、英雄、段位。
//! 设置 → 功能 → 游戏（settings.json 的 games）默认关：关着采集端不读客户端、侧栏不出游戏页，已记的照样能读。
//! 方案见 报告/方案_20261106_英雄联盟接入.md。都是读的时候现算、不另存；只看 win- 开头的设备（Mac 版客户端以后再接）。
//! - 一局：game.match（对局开始时刻、自己那一行的统计、raw＝客户端原样返回的 10 个人）。
//! - 排队、选人、读条多久：game.phase 的阶段变化，同一台电脑上从离开大厅到回到大厅算一轮，轮里带着 gameId 的就是那一局；
//!   没进成游戏的一轮（取消排队、有人秒退）只算进那天的等待。补写的历史局没有阶段。
//! - 段位：game.rank 每次变了才写；排位局前后两份一比就是这局加减的胜点。

use chrono::{DateTime, Duration, Local, Timelike, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const TYPES: [&str; 4] = ["game.match", "game.rank", "game.client", "game.phase"];
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
const GAMES: [&str; 1] = ["lol"];

/// 游戏页：有哪些游戏（games），和选的那款（现在只有 lol）的账号、段位、每一局（新的在前）、每天
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn games_overview(game: Option<String>) -> Value {
    let game = game.filter(|g| GAMES.contains(&g.as_str())).unwrap_or_else(|| "lol".into());
    let ev = events();
    let ms = matches_of(&ev);
    let games = json!([{ "id": "lol", "n": ms.len(), "last": ms.last().and_then(|e| start_of(e)).map(beayy_core::iso) }]);
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
        "game": game,
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
