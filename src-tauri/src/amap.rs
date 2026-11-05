//! 高德地图（同手机取名一个来源）：「这次在哪」的附近地点、搜地点（起名、添加常去的地方）、今天页的地图（静态图）。
//! 两种用法（settings.json 的 map_mode）：
//! - official（官方服务，按账号开通）：经官方服务（hk2，香港，到高德快）/v2/amap/… 中转，用服务器的高德 Key，凭同步服务按地图额度发的票据，
//!   每次算账号的地图用量；能缩放的地图拿公开的地图 Key＋安全密钥中转地址；
//! - custom（自己的 Key）：Key 是高德开放平台「Web 服务」类型的，macOS 存钥匙串（服务 com.fengli.beayy，账号 amap_key），Windows 在数据目录 `secrets/amap_key`；不回传网页。
//! 停留点存的是 WGS-84，发给高德前换成 GCJ-02，高德给的坐标换回来（同手机的 ChinaOffset）。坐标会发给高德
//! 地图服务也可以选 OpenStreetMap（settings.json 的 map_provider＝osm，10-30 加）：全球免费、不用 Key，附近的店、搜地点、在哪都走 OSM，见文件末尾

use serde_json::{json, Value};
use std::time::Duration;

pub const SECRET: &str = "amap_key";

/// 地图服务：amap（高德，默认）／osm（OpenStreetMap）
pub fn provider() -> &'static str {
    match crate::data::read_settings_raw().get("map_provider").and_then(|v| v.as_str()) {
        Some("osm") => "osm",
        _ => "amap",
    }
}

/// 地图走官方的吗：设置里选过就按选的；没选过的，有自己的 Key 算自己的，否则官方
pub fn official() -> bool {
    match crate::data::read_settings_raw().get("map_mode").and_then(|v| v.as_str()) {
        Some("official") => true,
        Some("custom") => false,
        _ => beayy_core::secret_get(SECRET).is_none(),
    }
}

/// 官方：同步服务器地址和认证头
fn official_auth() -> Result<(String, String), String> {
    beayy_sync::official_auth(&beayy_core::data_root()).ok_or_else(|| "用官方地图要先登录 Beayy 账号：设置 → 手机与手表 → 地图".to_string())
}

/// 地图票据：快到期（5 分钟内）再取新的
static MAP_TICKET: std::sync::Mutex<Option<(String, i64)>> = std::sync::Mutex::new(None);

fn map_ticket() -> Result<String, String> {
    let now = chrono::Utc::now().timestamp_millis();
    if let Some((t, exp)) = MAP_TICKET.lock().unwrap().clone() {
        if exp - now > 300_000 {
            return Ok(t);
        }
    }
    let got = beayy_sync::ticket_for(&beayy_core::data_root(), "map").map_err(|e| if e.contains("没登录") { "用官方地图要先登录 Beayy 账号：设置 → 手机与手表 → 地图".to_string() } else { e })?;
    *MAP_TICKET.lock().unwrap() = Some(got.clone());
    Ok(got.0)
}

/// 发一个高德 Web 服务请求：官方的经官方服务中转，自己的直接带 Key
fn amap_req(path: &str) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, String> {
    if official() {
        let t = map_ticket()?;
        Ok(agent().get(&format!("{}/v2/amap/{path}", beayy_core::DEFAULT_TRANSCRIBE)).header("Authorization", &format!("Ticket {t}")))
    } else {
        Ok(agent().get(&format!("https://restapi.amap.com/v3/{path}")).query("key", &key()?))
    }
}

fn key() -> Result<String, String> {
    beayy_core::secret_get(SECRET).ok_or_else(|| "还没有高德 Key：到设置里存一下".to_string())
}

/// 存高德 Key：macOS 进钥匙串，Windows 写数据目录；不回传网页。空字符串＝删掉
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_amap_key(key: String) -> Result<(), String> {
    beayy_core::secret_set(SECRET, key.trim())
}

/// 地点页能缩放的地图（高德 JS API 2.0）用的 Key 和安全密钥：「Web端(JS API)」类型，和取名用的「Web 服务」Key 是两个。
/// 存钥匙串（账号 amap_js_key、amap_js_code），Windows 在数据目录 secrets/。这一对本来就要放进页面里用，所以交给页面；没存就给 null，页面退回静态图
/// 官方：{key, service_host}（安全密钥留在服务器，地图经中转地址取）；自己的：{key, code}；都没有返回 null，页面退回静态图
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub async fn amap_js_config() -> Value {
    if official() {
        let Ok((srv, auth)) = official_auth() else { return Value::Null };
        let Ok(mut r) = agent().get(&format!("{srv}/v2/amap-js")).header("Authorization", &auth).call() else { return Value::Null };
        let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
        return if v["key"].is_string() { v } else { json!({ "error": v["error"] }) };
    }
    match (beayy_core::secret_get("amap_js_key"), beayy_core::secret_get("amap_js_code")) {
        (Some(k), Some(c)) => json!({ "key": k, "code": c }),
        _ => Value::Null,
    }
}

/// 存地图用的 Key 和安全密钥（空字符串＝删掉）
#[cfg_attr(not(beayy_mobile), tauri::command)]
pub fn set_amap_js(key: String, code: String) -> Result<(), String> {
    beayy_core::secret_set("amap_js_key", key.trim())?;
    beayy_core::secret_set("amap_js_code", code.trim())
}

fn agent() -> ureq::Agent {
    beayy_sync::http_agent(ureq::Agent::config_builder()
        .proxy(None)
        .tls_config(beayy_core::tls!())
        .timeout_global(Some(Duration::from_secs(10)))
        // 出错时也读返回体：官方地图没开通、额度用完写在里面
        .http_status_as_error(false)
        .build())
}

// ---------- 坐标换算（公开的标准公式，同手机） ----------

fn out_of_china(lat: f64, lon: f64) -> bool {
    !(72.004..137.8347).contains(&lon) || !(0.8293..55.8271).contains(&lat)
}

/// WGS-84 → GCJ-02（国外原样）
pub fn gcj(lat: f64, lon: f64) -> (f64, f64) {
    if out_of_china(lat, lon) {
        return (lat, lon);
    }
    let (a, ee) = (6378245.0_f64, 0.006_693_421_622_965_943_f64);
    let pi = std::f64::consts::PI;
    let (x, y) = (lon - 105.0, lat - 35.0);
    let mut dlat = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    dlat += (20.0 * (6.0 * x * pi).sin() + 20.0 * (2.0 * x * pi).sin()) * 2.0 / 3.0;
    dlat += (20.0 * (y * pi).sin() + 40.0 * (y / 3.0 * pi).sin()) * 2.0 / 3.0;
    dlat += (160.0 * (y / 12.0 * pi).sin() + 320.0 * (y * pi / 30.0).sin()) * 2.0 / 3.0;
    let mut dlon = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    dlon += (20.0 * (6.0 * x * pi).sin() + 20.0 * (2.0 * x * pi).sin()) * 2.0 / 3.0;
    dlon += (20.0 * (x * pi).sin() + 40.0 * (x / 3.0 * pi).sin()) * 2.0 / 3.0;
    dlon += (150.0 * (x / 12.0 * pi).sin() + 300.0 * (x / 30.0 * pi).sin()) * 2.0 / 3.0;
    let rad = lat / 180.0 * pi;
    let m = 1.0 - ee * rad.sin().powi(2);
    let s = m.sqrt();
    (lat + dlat * 180.0 / ((a * (1.0 - ee)) / (m * s) * pi), lon + dlon * 180.0 / (a / s * rad.cos() * pi))
}

/// GCJ-02 → WGS-84：反过来逼近两次，误差在 1 米内
pub fn wgs(glat: f64, glon: f64) -> (f64, f64) {
    let (mut lat, mut lon) = (glat, glon);
    for _ in 0..2 {
        let (a, b) = gcj(lat, lon);
        lat -= a - glat;
        lon -= b - glon;
    }
    (lat, lon)
}

fn location(lat: f64, lon: f64) -> String {
    let (a, b) = gcj(lat, lon);
    format!("{b:.6},{a:.6}")
}

// ---------- 请求 ----------

fn get(path: &str, q: &[(&str, String)]) -> Result<Value, String> {
    let mut req = amap_req(path)?;
    for (n, v) in q {
        req = req.query(*n, v);
    }
    let v: Value = req.call().map_err(|e| format!("连不上高德：{e}"))?.body_mut().read_json().map_err(|e| e.to_string())?;
    if let Some(e) = v["error"].as_str().or(v["detail"].as_str()) {
        return Err(e.to_string());
    }
    if v["status"] != "1" {
        return Err(format!("高德没返回结果：{}", v["info"].as_str().unwrap_or("未知")));
    }
    Ok(v)
}

/// 高德的地点 → {id, name, type, distance, lat, lon}（坐标换回 WGS-84）
fn pois(v: &Value) -> Vec<Value> {
    v["pois"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let name = p["name"].as_str()?;
                    let mut xy = p["location"].as_str()?.split(',').filter_map(|x| x.parse::<f64>().ok());
                    let (glon, glat) = (xy.next()?, xy.next()?);
                    let (lat, lon) = wgs(glat, glon);
                    let dist = p["distance"].as_str().and_then(|d| d.parse::<f64>().ok()).or(p["distance"].as_f64());
                    Some(json!({ "id": p["id"], "name": name, "type": p["type"].as_str().unwrap_or(""), "distance": dist, "lat": lat, "lon": lon }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 算「去的是哪家」时要的地点类别：同手机（去处，不要住宅、交通、道路、地名、公共设施）
const PLACE_TYPES: &str = "010000|020000|030000|040000|050000|060000|070000|080000|090000|100000|110000|130000|140000|160000|170000";

/// 附近的地点，按距离（「这次在哪」的候选列表）
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn amap_around(lat: f64, lon: f64, radius: Option<u32>) -> Result<Vec<Value>, String> {
    let r = radius.unwrap_or(150).clamp(10, 3000);
    if provider() == "osm" {
        return osm_around(lat, lon, r);
    }
    let v = get("place/around", &[("location", location(lat, lon)), ("radius", r.to_string()), ("sortrule", "distance".into()), ("types", PLACE_TYPES.into()), ("offset", "25".into())])?;
    Ok(pois(&v))
}

/// 按名字搜地点：有位置就按离这里多远（50 公里内），没有就全国按相关度
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn amap_search(text: String, lat: Option<f64>, lon: Option<f64>) -> Result<Vec<Value>, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(vec![]);
    }
    if provider() == "osm" {
        return osm_search(&text, lat.zip(lon));
    }
    let v = match (lat, lon) {
        (Some(a), Some(b)) => get("place/around", &[("location", location(a, b)), ("keywords", text), ("radius", "50000".into()), ("sortrule", "distance".into()), ("offset", "20".into())])?,
        _ => get("place/text", &[("keywords", text), ("offset", "20".into())])?,
    };
    Ok(pois(&v))
}

/// 今天页的地图：高德静态地图（同一个 Web 服务 Key），停留点按先后编号（1–9、A–Z，标注用设计系统的地点色 place-alt，编号是黑字；selected 那一段用强调色），按先后连线。返回 data:image/png
#[cfg_attr(not(beayy_mobile), tauri::command(async))]
pub fn amap_map(points: Vec<[f64; 2]>, width: Option<u32>, height: Option<u32>, selected: Option<usize>, often: Option<Vec<bool>>) -> Result<String, String> {
    if points.is_empty() {
        return Err("这天没有停留点".into());
    }
    if provider() == "osm" {
        return Err("OpenStreetMap 没有静态图，看能拖动的地图".into());
    }
    let (w, h) = (width.unwrap_or(1000).clamp(200, 1024), height.unwrap_or(480).clamp(200, 1024));
    const MARKS: &[u8] = b"123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let loc: Vec<String> = points.iter().map(|p| location(p[0], p[1])).collect();
    // 选中的那一段用墨线的强调色，其余用地点色；选中的画在最后，压在别的上面
    // 常去的（起过名、或 30 天去过 2 次以上）用地点色 place-alt，第一次去的用更浅的 place
    let often = often.unwrap_or_default();
    let mark = |i: usize, l: &String| format!("large,{},{}:{l}", if Some(i) == selected { "0x6DA7EC" } else if often.get(i).copied().unwrap_or(true) { "0xB3BAC0" } else { "0xE3E6E8" }, MARKS[i.min(MARKS.len() - 1)] as char);
    let mut order: Vec<usize> = (0..loc.len()).filter(|i| Some(*i) != selected).collect();
    order.extend(selected.filter(|i| *i < loc.len()));
    let markers = order.iter().map(|&i| mark(i, &loc[i])).collect::<Vec<_>>().join("|");
    let mut req = amap_req("staticmap")?.query("size", &format!("{w}*{h}")).query("scale", "2").query("markers", &markers);
    if loc.len() > 1 {
        req = req.query("paths", &format!("3,0x73726C,0.9,,:{}", loc.join(";")));
    } else {
        req = req.query("location", &loc[0]).query("zoom", "16");
    }
    let mut r = req.call().map_err(|e| format!("连不上高德：{e}"))?;
    let ct = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let body = r.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
    if !ct.starts_with("image/") {
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        return Err(v["error"].as_str().or(v["detail"].as_str()).map(String::from).unwrap_or_else(|| format!("高德没给地图：{}", v["info"].as_str().unwrap_or("未知"))));
    }
    Ok(format!("data:{ct};base64,{}", crate::icons::base64(&body)))
}

// ---------- 在哪：行政区划＋区域（国家 › 省 › 市 › 区县 › 街道 › 区域） ----------

/// 一个点的完整分级（高德逆地理编码）：国家、省、市（直辖市为空时用省）、区县、街道乡镇，区划代码（adcode＝GB/T 2260 的 6 位，
/// towncode＝统计用区划代码），和区域（AOI：点落在面里的，先挑不是住宅写字楼的、再挑范围大的；都不在就用误差内最近的；再没有写「街道一带」）。
/// 缓存在 Beayy/places/admin.json，按坐标取到小数点后 4 位（约 10 米）；区域的挑法跟误差有关，误差分 3 档进 key
pub fn admin(lat: f64, lon: f64, acc: f64, online: bool) -> Option<Value> {
    let band = if acc <= 30.0 { 0 } else if acc <= 80.0 { 1 } else { 2 };
    let key = format!("{lat:.4},{lon:.4},{band}");
    let path = beayy_core::default_root().join("places").join("admin.json");
    let mut cache: serde_json::Map<String, Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Some(v) = cache.get(&key) {
        return Some(v.clone());
    }
    if !online {
        return None;
    }
    if provider() == "osm" {
        let out = osm_admin(lat, lon)?;
        cache.insert(key, out.clone());
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(&path, serde_json::to_string(&Value::Object(cache)).unwrap_or_default());
        return Some(out);
    }
    let v = get("geocode/regeo", &[("location", location(lat, lon)), ("extensions", "all".into()), ("radius", "100".into()), ("roadlevel", "0".into())]).ok()?;
    std::thread::sleep(Duration::from_millis(350)); // 高德限 3 次／秒
    let r = &v["regeocode"];
    let c = &r["addressComponent"];
    let s = |x: &Value| x.as_str().filter(|t| !t.is_empty()).map(String::from);
    let tol = (acc / 2.0).clamp(15.0, 40.0);
    let aois: Vec<(String, String, f64, f64)> = r["aois"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let num = |k: &str| x[k].as_str().and_then(|t| t.parse::<f64>().ok()).or(x[k].as_f64());
                    Some((s(&x["name"])?, s(&x["type"]).unwrap_or_default(), num("area").unwrap_or(f64::MAX), num("distance").unwrap_or(f64::MAX)))
                })
                .collect()
        })
        .unwrap_or_default();
    let within: Vec<&(String, String, f64, f64)> = aois.iter().filter(|a| a.3 == 0.0).collect();
    let mut pick: Vec<&(String, String, f64, f64)> = if within.is_empty() {
        aois.iter().filter(|a| a.3 <= tol).min_by(|a, b| a.3.partial_cmp(&b.3).unwrap()).into_iter().collect()
    } else {
        within
    };
    pick.sort_by(|a, b| (a.1.starts_with("12")).cmp(&b.1.starts_with("12")).then(b.2.partial_cmp(&a.2).unwrap()));
    let province = s(&c["province"]);
    let street = s(&c["streetNumber"]["street"]);
    let area = pick.first().map(|a| a.0.clone()).or(street.map(|x| format!("{x}一带"))).or(s(&c["township"]));
    let out = json!({
        "country": s(&c["country"]),
        "province": province,
        "city": s(&c["city"]).or(province.clone()),
        "district": s(&c["district"]),
        "township": s(&c["township"]),
        "adcode": s(&c["adcode"]),
        "towncode": s(&c["towncode"]),
        "area": area,
    });
    cache.insert(key, out.clone());
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, serde_json::to_string(&Value::Object(cache)).unwrap_or_default());
    Some(out)
}

// ---------- 是哪家店：候选和把握（电脑为准，手机过渡期也有一份同样的） ----------

/// 喝东西的那几种（高德中类）
const DRINKS: [&str; 5] = ["咖啡厅", "冷饮店", "甜品店", "茶艺馆", "糕饼店"];

fn major(t: &str) -> &str {
    t.split(';').next().unwrap_or("")
}
fn minor(t: &str) -> &str {
    t.split(';').nth(1).unwrap_or(major(t))
}

/// 给附近的地点打分算把握（同手机 Amap.guesses）：离得越近越像；待的时长和时段对得上的加分（饭点 11–14、17–21 点待 18 分钟以上的餐馆＋40，
/// 待 5–35 分钟的咖啡、冷饮、甜品＋35）；30 米内确认过的店＋60（不在高德列表里的也加进来）。分数换成把握：exp(−分/τ)，τ＝max(15, 误差/2)
pub fn score(pois: &[Value], remembered: &[Value], here: (f64, f64), acc: f64, arrive: Option<chrono::DateTime<chrono::Local>>, dur_sec: Option<f64>) -> Vec<Value> {
    use chrono::Timelike;
    let hour = arrive.map(|a| a.hour() as f64 + a.minute() as f64 / 60.0);
    let meal = hour.map(|h| (11.0..=14.0).contains(&h) || (17.0..=21.0).contains(&h)).unwrap_or(false);
    let bonus = |t: &str| -> f64 {
        let Some(d) = dur_sec else { return 0.0 };
        let drink = DRINKS.contains(&minor(t));
        if meal && d >= 18.0 * 60.0 && major(t) == "餐饮服务" && !drink {
            40.0
        } else if (5.0 * 60.0..=35.0 * 60.0).contains(&d) && drink {
            35.0
        } else {
            0.0
        }
    };
    let dist = |lat: f64, lon: f64| crate::data::haversine(here.0, here.1, lat, lon);
    let near: Vec<&Value> = remembered.iter().filter(|r| dist(r["lat"].as_f64().unwrap_or(0.0), r["lon"].as_f64().unwrap_or(0.0)) <= 30.0).collect();
    let mut c: Vec<(String, String, f64, f64)> = pois
        .iter()
        .filter_map(|p| {
            let (n, t) = (p["name"].as_str()?.to_string(), p["type"].as_str().unwrap_or("").to_string());
            let d = p["distance"].as_f64().unwrap_or(0.0);
            let r = near.iter().any(|x| x["name"] == n.as_str());
            let s = d - bonus(&t) - if r { 60.0 } else { 0.0 };
            Some((n, t, d, s))
        })
        .collect();
    for r in near {
        let n = r["name"].as_str().unwrap_or("").to_string();
        if n.is_empty() || c.iter().any(|x| x.0 == n) {
            continue;
        }
        let t = r["type"].as_str().unwrap_or("").to_string();
        let d = dist(r["lat"].as_f64().unwrap_or(0.0), r["lon"].as_f64().unwrap_or(0.0));
        let s = d - bonus(&t) - 60.0;
        c.push((n, t, d, s));
    }
    if c.is_empty() {
        return vec![];
    }
    let tau = (acc / 2.0).max(15.0);
    let best = c.iter().map(|x| x.3).fold(f64::INFINITY, f64::min);
    let w: Vec<f64> = c.iter().map(|x| (-(x.3 - best) / tau).exp()).collect();
    let z: f64 = w.iter().sum();
    let mut out: Vec<Value> = c.iter().zip(&w).map(|(x, w)| json!({ "name": x.0, "type": x.1, "p": ((w / z) * 1000.0).round() / 1000.0, "distance": (x.2 * 10.0).round() / 10.0 })).collect();
    out.sort_by(|a, b| b["p"].as_f64().unwrap_or(0.0).partial_cmp(&a["p"].as_f64().unwrap_or(0.0)).unwrap());
    out.truncate(5);
    out
}

/// 一段停留的候选（电脑算的）：缓存在 Beayy/places/guesses.json，按「停留点 id:版本:离开时刻」存，停留点变了才重算。
/// 高德问不到（没 Key、没网）返回 None，10 分钟内不再试；30 米内确认过的店（picks.json 的 remembered）每次现算，不进缓存
pub fn guesses_for(id: &str, rev: i64, lat: f64, lon: f64, acc: f64, arrive: Option<chrono::DateTime<chrono::Utc>>, depart: Option<chrono::DateTime<chrono::Utc>>, remembered: &[Value], online: bool) -> Option<Vec<Value>> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;
    static FAILED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let key = format!("{id}:{rev}:{}", depart.map(|d| d.timestamp()).unwrap_or(0));
    let path = beayy_core::default_root().join("places").join("guesses.json");
    let mut cache: serde_json::Map<String, Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let pois: Vec<Value> = match cache.get(&key).and_then(|v| v.as_array()) {
        Some(a) => a.clone(),
        None => {
            if !online {
                return None;
            }
            if FAILED.lock().ok()?.get_or_insert_with(HashMap::new).get(&key).map(|t| t.elapsed().as_secs() < 600).unwrap_or(false) {
                return None;
            }
            let reach = (acc * 2.0).clamp(50.0, 120.0) as u32;
            match amap_around(lat, lon, Some(reach)) {
                Ok(p) => {
                    // 只存算分要的字段
                    let slim: Vec<Value> = p.iter().map(|x| json!({ "name": x["name"], "type": x["type"], "distance": x["distance"] })).collect();
                    cache.insert(key.clone(), json!(slim));
                    if cache.len() > 2000 {
                        let drop: Vec<String> = cache.keys().take(cache.len() - 2000).cloned().collect();
                        for k in drop {
                            cache.remove(&k);
                        }
                    }
                    let _ = std::fs::create_dir_all(path.parent().unwrap());
                    let _ = std::fs::write(&path, serde_json::to_string(&Value::Object(cache)).unwrap_or_default());
                    std::thread::sleep(Duration::from_millis(350)); // 高德限 3 次／秒
                    slim
                }
                Err(_) => {
                    FAILED.lock().ok()?.get_or_insert_with(HashMap::new).insert(key, Instant::now());
                    return None;
                }
            }
        }
    };
    let dur = arrive.zip(depart).map(|(a, d)| (d - a).num_seconds() as f64);
    Some(score(&pois, remembered, (lat, lon), acc, arrive.map(|a| a.with_timezone(&chrono::Local)), dur))
}

/// 店名去掉括号里的分店名：「星巴克(某某广场店)」→「星巴克」（同手机）
pub fn short(name: &str) -> String {
    for (open, close) in [('(', ')'), ('（', '）')] {
        if name.ends_with(close) {
            if let Some(i) = name.find(open).filter(|i| *i > 0) {
                return name[..i].trim().to_string();
            }
        }
    }
    name.trim().to_string()
}

// ---------- OpenStreetMap（map_provider＝osm）：全球免费、不用 Key ----------
// 在哪、搜地点用 Nominatim（每秒最多 1 次，要带认得出 app 的 User-Agent），附近的店用 Overpass；坐标都是 WGS-84，不用换。
// 店的类别换成高德那样的「大类;中类」，算把握时饭点、喝东西的加分照旧（score 认的是高德的叫法）

const OSM_UA: &str = concat!("Beayy/", env!("CARGO_PKG_VERSION"), " (https://beayy.com)");

fn osm_get(url: &str, q: &[(&str, String)]) -> Result<Value, String> {
    let mut req = agent().get(url).header("User-Agent", OSM_UA).header("Accept-Language", "zh-CN,zh,en");
    for (n, v) in q {
        req = req.query(*n, v);
    }
    let mut r = req.call().map_err(|e| format!("连不上 OpenStreetMap：{e}"))?;
    if !r.status().is_success() {
        return Err(format!("OpenStreetMap 没返回结果（{}）", r.status()));
    }
    r.body_mut().read_json().map_err(|e| e.to_string())
}

/// OSM 的标签 → 高德那样的「大类;中类」（常见的几种；其余写成「标签;值」）
fn osm_type(tags: &Value) -> String {
    let t = |k: &str| tags[k].as_str().unwrap_or("");
    let s = match (t("amenity"), t("shop"), t("leisure"), t("tourism")) {
        ("cafe", ..) => "餐饮服务;咖啡厅",
        ("ice_cream", ..) | (_, "ice_cream", ..) => "餐饮服务;冷饮店",
        ("fast_food", ..) => "餐饮服务;快餐厅",
        ("restaurant" | "food_court", ..) => "餐饮服务;餐厅",
        ("bar" | "pub", ..) => "餐饮服务;酒吧",
        (_, "bakery" | "pastry", ..) => "餐饮服务;糕饼店",
        (_, "tea", ..) => "餐饮服务;茶艺馆",
        ("school" | "university" | "college" | "kindergarten", ..) => "科教文化服务;学校",
        ("library", ..) => "科教文化服务;图书馆",
        ("hospital" | "clinic" | "doctors" | "dentist", ..) => "医疗保健服务;医院",
        ("pharmacy", ..) => "医疗保健服务;药店",
        ("bank", ..) => "金融保险服务;银行",
        ("cinema" | "theatre", ..) => "体育休闲服务;影剧院",
        (_, _, "fitness_centre" | "sports_centre", _) => "体育休闲服务;运动场馆",
        (_, _, "park", _) => "风景名胜;公园广场",
        (_, "supermarket", ..) => "购物服务;超级市场",
        (_, "mall" | "department_store", ..) => "购物服务;商场",
        (_, "convenience", ..) => "购物服务;便利店",
        (_, _, _, "hotel" | "hostel" | "guest_house" | "motel") => "住宿服务;宾馆酒店",
        (_, _, _, "museum") => "科教文化服务;博物馆",
        (_, _, _, "attraction" | "viewpoint") => "风景名胜;风景名胜",
        (a, sh, l, tr) => {
            let (k, v) = [("amenity", a), ("shop", sh), ("leisure", l), ("tourism", tr), ("office", t("office"))].into_iter().find(|x| !x.1.is_empty()).unwrap_or(("place", ""));
            return format!("{k};{v}");
        }
    };
    s.to_string()
}

/// 名字：有中文名用中文名
fn osm_name(tags: &Value) -> Option<String> {
    ["name:zh", "name:zh-Hans", "name"].iter().find_map(|k| tags[*k].as_str().filter(|s| !s.is_empty()).map(String::from))
}

/// 附近有名字的店和地方（吃喝、购物、休闲、住宿、办公），按距离，最多 25 个。
/// 先问 Overpass；连不上（在中国大陆常超时）退到 Photon（komoot 的 OSM 搜索）的反查，附近的点少一些
fn osm_around(lat: f64, lon: f64, r: u32) -> Result<Vec<Value>, String> {
    let a = format!("(around:{r},{lat:.6},{lon:.6})");
    let q = format!("[out:json][timeout:15];(nwr{a}[name][amenity];nwr{a}[name][shop];nwr{a}[name][leisure];nwr{a}[name][tourism];nwr{a}[name][office];);out center 60;");
    let v = match osm_get("https://overpass-api.de/api/interpreter", &[("data", q)]) {
        Ok(v) => v,
        Err(_) => return photon_around(lat, lon, r),
    };
    let mut out: Vec<Value> = v["elements"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let tags = &e["tags"];
                    let name = osm_name(tags)?;
                    let (plat, plon) = (e["lat"].as_f64().or(e["center"]["lat"].as_f64())?, e["lon"].as_f64().or(e["center"]["lon"].as_f64())?);
                    let d = crate::data::haversine(lat, lon, plat, plon);
                    Some(json!({ "id": format!("osm:{}/{}", e["type"].as_str().unwrap_or(""), e["id"]), "name": name, "type": osm_type(tags), "distance": (d * 10.0).round() / 10.0, "lat": plat, "lon": plon }))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a["distance"].as_f64().partial_cmp(&b["distance"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(25);
    Ok(out)
}

fn photon_around(lat: f64, lon: f64, r: u32) -> Result<Vec<Value>, String> {
    let v = osm_get("https://photon.komoot.io/reverse", &[("lat", format!("{lat:.6}")), ("lon", format!("{lon:.6}")), ("radius", format!("{:.3}", r as f64 / 1000.0)), ("limit", "40".into())])?;
    let mut out: Vec<Value> = v["features"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let p = &f["properties"];
                    let key = p["osm_key"].as_str()?;
                    if !["amenity", "shop", "leisure", "tourism", "office"].contains(&key) {
                        return None;
                    }
                    let name = p["name"].as_str().filter(|s| !s.is_empty())?.to_string();
                    let c = f["geometry"]["coordinates"].as_array()?;
                    let (plon, plat) = (c.first()?.as_f64()?, c.get(1)?.as_f64()?);
                    let mut tags = serde_json::Map::new();
                    tags.insert(key.into(), p["osm_value"].clone());
                    let d = crate::data::haversine(lat, lon, plat, plon);
                    Some(json!({ "id": format!("osm:{}/{}", p["osm_type"].as_str().unwrap_or(""), p["osm_id"]), "name": name, "type": osm_type(&Value::Object(tags)), "distance": (d * 10.0).round() / 10.0, "lat": plat, "lon": plon }))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a["distance"].as_f64().partial_cmp(&b["distance"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(25);
    Ok(out)
}

/// 按名字搜：有位置就偏向这附近（约 50 公里），按距离排
fn osm_search(text: &str, near: Option<(f64, f64)>) -> Result<Vec<Value>, String> {
    let mut q = vec![("q", text.to_string()), ("format", "jsonv2".into()), ("limit", "20".into()), ("namedetails", "1".into())];
    if let Some((lat, lon)) = near {
        q.push(("viewbox", format!("{:.4},{:.4},{:.4},{:.4}", lon - 0.45, lat + 0.45, lon + 0.45, lat - 0.45)));
    }
    let v = osm_get("https://nominatim.openstreetmap.org/search", &q)?;
    let mut out: Vec<Value> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let (plat, plon) = (r["lat"].as_str()?.parse::<f64>().ok()?, r["lon"].as_str()?.parse::<f64>().ok()?);
                    let name = osm_name(&r["namedetails"]).or_else(|| r["name"].as_str().filter(|s| !s.is_empty()).map(String::from))
                        .or_else(|| r["display_name"].as_str().and_then(|d| d.split(',').next()).map(|s| s.trim().to_string()))?;
                    let mut tags = serde_json::Map::new();
                    tags.insert(r["category"].as_str().unwrap_or("place").to_string(), r["type"].clone());
                    let d = near.map(|(lat, lon)| (crate::data::haversine(lat, lon, plat, plon) * 10.0).round() / 10.0);
                    Some(json!({ "id": format!("osm:{}/{}", r["osm_type"].as_str().unwrap_or(""), r["osm_id"]), "name": name, "type": osm_type(&Value::Object(tags)), "distance": d, "lat": plat, "lon": plon }))
                })
                .collect()
        })
        .unwrap_or_default();
    if near.is_some() {
        out.sort_by(|a, b| a["distance"].as_f64().partial_cmp(&b["distance"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    }
    Ok(out)
}

/// 在哪（同高德 admin 的字段）：国家、省、市、区县、街道，区域是点所在的那个有名字的地方（楼、园区、店），没有就「路名一带」
fn osm_admin(lat: f64, lon: f64) -> Option<Value> {
    let v = osm_get("https://nominatim.openstreetmap.org/reverse", &[("lat", format!("{lat:.6}")), ("lon", format!("{lon:.6}")), ("format", "jsonv2".into()), ("zoom", "18".into()), ("addressdetails", "1".into()), ("namedetails", "1".into())]).ok()?;
    std::thread::sleep(Duration::from_millis(1100)); // Nominatim 限每秒 1 次
    let a = &v["address"];
    let s = |keys: &[&str]| keys.iter().find_map(|k| a[*k].as_str().filter(|t| !t.is_empty()).map(String::from));
    let province = s(&["state", "province", "region"]);
    let named = osm_name(&v["namedetails"]).or_else(|| v["name"].as_str().filter(|t| !t.is_empty()).map(String::from));
    // 区域：点落在的那个有名字的地方（不是路）；再看地址里的小区、商圈、楼；都没有写「路名一带」
    let area = named.filter(|_| v["category"] != "highway")
        .or_else(|| s(&["amenity", "shop", "building", "leisure", "tourism", "office", "residential", "commercial", "retail", "industrial"]))
        .or_else(|| s(&["road"]).map(|r| format!("{r}一带")))
        .or_else(|| s(&["neighbourhood", "quarter", "suburb"]));
    // 中国的数据里 city 常常填的是区县（「江宁区」），这时把它当区县，市写省
    let mut city = s(&["city", "town", "municipality"]);
    let mut district = s(&["city_district", "district", "county", "borough"]);
    if district.is_none() && city.as_deref().map(|c| c.ends_with('区') || c.ends_with('县')).unwrap_or(false) {
        district = city.take();
    }
    Some(json!({
        "country": s(&["country"]),
        "province": province,
        "city": city.or(province.clone()),
        "district": district,
        "township": s(&["suburb", "quarter", "village", "town"]),
        "adcode": Value::Null,
        "towncode": Value::Null,
        "area": area,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcj_round_trip() {
        let (a, b) = gcj(31.23040, 121.47370);
        assert!((a - 31.23040).abs() > 0.001 || (b - 121.47370).abs() > 0.001);
        let (x, y) = wgs(a, b);
        assert!((x - 31.23040).abs() < 1e-5 && (y - 121.47370).abs() < 1e-5);
        assert_eq!(gcj(40.0, -74.0), (40.0, -74.0));
    }

    #[test]
    fn short_name() {
        assert_eq!(short("星巴克(某某广场店)"), "星巴克");
        assert_eq!(short("某某面馆（中山路店）"), "某某面馆");
        assert_eq!(short("某某小区"), "某某小区");
    }
}
