//! 多设备同步客户端。协议见 报告/方案_20261016_多设备同步.md，服务端在 app/server/。
//!
//! - 同步单位是一个分段文件 `devices/<设备>/segments/<分段>/events.jsonl`；本机写的 10 分钟摘要也同步，
//!   块名 `sum.<摘要文件名>`，别的设备下载到 `devices/<设备>/summaries/<摘要文件名>`（时间线显示是哪台电脑的）。
//! - 本机写的日、周、月总结（`<数据根>/daily/日总结_YYYYMMDD.md`、`周总结_YYYYMMDD.md`、`月总结_YYYYMM.md`）也同步，
//!   块名 `daily.d.YYYYMMDD.md`、`daily.w.YYYYMMDD.md`、`daily.m.YYYYMM.md`（块名只能是 ASCII），
//!   别的设备下载到 `devices/<设备>/daily/<原文件名>`（手机不写总结，全靠这个看）。
//! - 设备改名：每台设备把自己改过的名字写在 `<数据根>/device_names.json`（{设备: {name, at}}），块名 `names.json`
//!   同步；别的设备下载到 `devices/<设备>/device_names.json`，读取时按改名时刻取最新的。
//! - 下载来的设备目录里放一个 `.synced` 标记：换账号、重登录清了状态以后，也不会把别人的数据当成自己的再传上去。
//! - 上传前先 deflate 压缩，再用 XChaCha20-Poly1305 加密（随机 24 字节 nonce 放在密文前，AAD＝`<设备>/<分段>`）；
//!   密钥只在设备上（钥匙串），服务器读不到内容。
//! - 连官方服务时，本机电脑的分段去掉屏幕文字（`ui.text` 的 text）再传，省九成空间；屏幕文字只留在本机（见 lean）。
//! - 本机写的设备（本机电脑、本机读到的 iPhone）只上传；别的设备只下载，写到同样的位置，读取方不用改。
//! - 状态在 `<数据根>/sync/state.json`：每个分段上次上传时的大小和修改时间、下载游标、哪些设备是同步来的。
//! - 账号（v2）：邮箱＋主密码。主密码用 Argon2id 派生成主钥，再用 HKDF 分出「登录校验值」（发给服务器，服务器只存它的哈希）
//!   和「包钥」（在本机解开服务器上包好的数据密钥）。恢复码就是数据密钥本身（base32 分组），忘了主密码时用它重设。
//!   每台设备登录拿自己的令牌。v1 的配对码账号用 claim 迁移：同一把数据密钥，已上传的数据不用重传。
//! - 同步文件夹（不经服务器）：账号的 server 写成 `folder:<路径>`，块按同样的加密写成 `<路径>/blobs/<设备>/<块名>`，
//!   数据密钥用主密码包一层放在 `<路径>/vault.json`。文件夹交给 iCloud 云盘、OneDrive、Dropbox、Syncthing、NAS 去同步；
//!   每台设备只写自己的目录（单写者），没有冲突。网盘晚到的文件修改时刻可能比游标早，所以下载不靠游标，按每个文件的大小和修改时刻比。
//!   没有服务器，就没有官方模型、转写、地图和手机直传（这些要账号登录服务器）。
//! - WebDAV（坚果云、NAS、Nextcloud）：server 写成 `webdav:<地址>`，令牌放 Basic 认证（base64 的「用户:密码」），布局、vault、恢复码同文件夹。
//!   列文件用 PROPFIND Depth 1（先列 blobs/ 下的设备，再列每台设备），按大小＋修改时刻（有 etag 也算上）比；列表缓存 60 秒，
//!   坚果云免费版限请求次数：限流、连不上的那一块不记成已传／已下，这一轮停下，下一轮接着来。
//! - iCloud（Mac＋iPhone，不要 Beayy 账号）：server 写成 `icloud:<路径>`，就是放在 iCloud 容器 iCloud.com.fengli.beayy 的 Sync/ 里的同步文件夹。
//!   iPhone 由 Swift 解析容器写进 `<数据根>/icloud_container`；Mac 用 ~/Library/Mobile Documents/iCloud~com~fengli~beayy（要 iPhone 先建好）。

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

pub const DEFAULT_SERVER: &str = "https://sync.beayy.com";
/// 同步文件夹模式下账号 server 的前缀
const FOLDER: &str = "folder:";
/// iCloud 模式的前缀（也是同步文件夹，路径每次重新解析）
const ICLOUD: &str = "icloud:";
/// WebDAV 模式的前缀
const WEBDAV: &str = "webdav:";
/// iCloud 容器（iPhone 的 entitlements 里登记的同一个）
pub const ICLOUD_CONTAINER: &str = "iCloud.com.fengli.beayy";
/// 容器里放同步的目录（不放 Documents：不在「文件」app 里露出来，免得被误删）
const ICLOUD_SUB: &str = "Sync";
const CODE_PREFIX: &str = "beayy-sync-1:";
const PARALLEL: usize = 8;
/// 下载并发：每块都要过一次鉴权查库，单次慢，多开几路
const PARALLEL_DOWN: usize = 16;

/// 一个同步账号：服务器、账号、邮箱（v1 迁移前没有）、这台设备的令牌（只管能不能存取）、数据密钥（加解密，只在设备上）
#[derive(Clone)]
pub struct Account {
    pub server: String,
    pub user: String,
    pub email: Option<String>,
    token: String,
    key: [u8; 32],
}

impl Account {
    /// 配对码：新设备粘贴它就加入同一账号。它等于全部数据的钥匙
    pub fn code(&self) -> String {
        let j = json!({"s": self.server, "u": self.user, "t": self.token, "k": B64.encode(self.key), "e": self.email});
        format!("{CODE_PREFIX}{}", B64.encode(j.to_string()))
    }

    pub fn from_code(code: &str) -> Result<Account, String> {
        let body = code.trim().strip_prefix(CODE_PREFIX).ok_or("配对码不对：应以 beayy-sync-1: 开头")?;
        let raw = B64.decode(body.trim()).map_err(|_| "配对码不对：解不开")?;
        let j: Value = serde_json::from_slice(&raw).map_err(|_| "配对码不对：内容坏了")?;
        let key = B64.decode(j["k"].as_str().unwrap_or("")).map_err(|_| "配对码不对：密钥坏了")?;
        Ok(Account {
            server: j["s"].as_str().unwrap_or(DEFAULT_SERVER).trim_end_matches('/').to_string(),
            user: j["u"].as_str().ok_or("配对码不对：缺账号")?.to_string(),
            email: j["e"].as_str().map(String::from),
            token: j["t"].as_str().ok_or("配对码不对：缺令牌")?.to_string(),
            key: key.try_into().map_err(|_| "配对码不对：密钥长度不对")?,
        })
    }

    fn auth(&self) -> String {
        format!("Bearer {}:{}", self.user, self.token)
    }

    /// 同步文件夹、iCloud 模式：文件夹路径（iCloud 每次重新解析，解析不到用当初的）；其他返回 None
    pub fn folder(&self) -> Option<PathBuf> {
        if let Some(p) = self.server.strip_prefix(FOLDER) {
            return Some(PathBuf::from(p));
        }
        self.server.strip_prefix(ICLOUD).map(|p| icloud_dir().unwrap_or_else(|_| PathBuf::from(p)))
    }

    /// WebDAV 模式：地址；其他返回 None
    pub fn webdav(&self) -> Option<&str> {
        self.server.strip_prefix(WEBDAV)
    }

    /// 同步方式：server／folder／icloud／webdav
    pub fn mode(&self) -> &'static str {
        if self.server.starts_with(FOLDER) {
            "folder"
        } else if self.server.starts_with(ICLOUD) {
            "icloud"
        } else if self.server.starts_with(WEBDAV) {
            "webdav"
        } else {
            "server"
        }
    }

    /// 不经服务器的存放处（文件夹、iCloud、WebDAV）；服务器模式返回 None
    fn remote(&self) -> Option<Store> {
        if let Some(d) = self.folder() {
            return Some(Store::Dir(d));
        }
        self.webdav().map(|b| Store::Dav(Dav { base: b.to_string(), auth: format!("Basic {}", self.token) }))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.key).into())
    }
}

// ---------- 账号存放：默认数据目录存钥匙串，其他（测试用的 BEAYY_DATA_ROOT、Windows）存数据目录里的文件 ----------

fn use_keychain() -> bool {
    cfg!(target_os = "macos") && std::env::var("BEAYY_DATA_ROOT").is_err()
}

/// 存账号的位置：sync＝同步用的；service＝只为官方服务（模型、转写、地图）登的（同步用 iCloud、网盘、文件夹或不同步时）
const SYNC: &str = "sync";
const SERVICE: &str = "service";

fn account_file(root: &Path, slot: &str) -> PathBuf {
    root.join("sync").join(if slot == SYNC { "account" } else { slot })
}

pub fn load(root: &Path) -> Option<Account> {
    load_slot(root, SYNC)
}

fn load_slot(root: &Path, slot: &str) -> Option<Account> {
    let code = if use_keychain() {
        let o = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", "com.fengli.beayy", "-a", slot, "-w"])
            .output()
            .ok()?;
        if !o.status.success() {
            return None;
        }
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    } else {
        fs::read_to_string(account_file(root, slot)).ok()?
    };
    Account::from_code(&code).ok()
}

fn save(root: &Path, a: &Account) -> Result<(), String> {
    save_slot(root, SYNC, a)
}

fn save_slot(root: &Path, slot: &str, a: &Account) -> Result<(), String> {
    if use_keychain() {
        let ok = std::process::Command::new("/usr/bin/security")
            .args(["add-generic-password", "-U", "-s", "com.fengli.beayy", "-a", slot, "-w", &a.code()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        return if ok { Ok(()) } else { Err("写入钥匙串失败".into()) };
    }
    let f = account_file(root, slot);
    fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&f, a.code()).map_err(|e| e.to_string())
}

/// 本机退出同步：忘掉账号（服务器上的数据不动）。同步进度留着，同一个账号再登录不用从头重传
pub fn forget(root: &Path) {
    forget_slot(root, SYNC)
}

fn forget_slot(root: &Path, slot: &str) {
    if use_keychain() {
        let _ = std::process::Command::new("/usr/bin/security").args(["delete-generic-password", "-s", "com.fengli.beayy", "-a", slot]).output();
    }
    let _ = fs::remove_file(account_file(root, slot));
}

/// 官方服务用的账号：同步用的是 Beayy 服务就是它，否则是单为官方服务登的（没有返回 None）
pub fn service(root: &Path) -> Option<Account> {
    load(root).filter(|a| a.mode() == "server").or_else(|| load_slot(root, SERVICE))
}

/// 同步用的是不是 Beayy 服务（官方或自建）：录音、转写这些块只走服务器
pub fn server_mode(root: &Path) -> bool {
    load(root).is_some_and(|a| a.mode() == "server")
}

/// 同步改用 Beayy 服务登录了：单为官方服务登的那个用不着了，服务器上注销、本机忘掉
fn drop_service(root: &Path) {
    if let Some(s) = load_slot(root, SERVICE) {
        let _ = agent().delete(&format!("{}/v2/sessions/self", s.server)).header("Authorization", &s.auth()).call();
        forget_slot(root, SERVICE);
    }
}

/// 停掉 Beayy 服务同步但留着登录给官方服务用（换成 iCloud、网盘，或不同步）：账号挪到 service，令牌照旧有效
pub fn keep_for_service(root: &Path) -> Result<(), String> {
    let a = load(root).filter(|a| a.mode() == "server" && a.email.is_some()).ok_or("没用 Beayy 服务同步")?;
    save_slot(root, SERVICE, &a)?;
    forget(root);
    Ok(())
}

/// 只为官方服务登录（同步不变）
pub fn service_login(root: &Path, server: &str, email: &str, password: &str, device: &str, name: &str) -> Result<(), String> {
    let a = login_account(server, email, password, device, name)?;
    save_slot(root, SERVICE, &a)
}

/// 只为官方服务注册（同步不变），返回恢复码：以后换成 Beayy 服务同步用的也是这个账号
pub fn service_signup(root: &Path, server: &str, invite: &str, email: &str, password: &str, device: &str, name: &str) -> Result<String, String> {
    let a = signup_account(server, invite, email, password, device, name)?;
    save_slot(root, SERVICE, &a)?;
    Ok(recovery_code_of(&a.key))
}

/// 退出单为官方服务登的账号
pub fn service_logout(root: &Path) {
    drop_service(root);
}

/// 单为官方服务登的账号的邮箱（同步用 Beayy 服务时没有）
pub fn service_email(root: &Path) -> Option<String> {
    load_slot(root, SERVICE).and_then(|a| a.email)
}

/// 登录或注册之后：换了账号才清同步进度（上传记录、下载游标都是按账号的）；同一个账号只清掉「要重新登录」和上次的错
fn reset_state_for(root: &Path, user: &str) {
    let mut st = read_state(root);
    if st.raw.get("user").and_then(Value::as_str) != Some(user) {
        let _ = fs::remove_file(state_path(root));
        return;
    }
    st.raw.insert("need_login".into(), json!(false));
    st.raw.insert("last_error".into(), Value::Null);
    write_state(root, &st);
}

// ---------- HTTP ----------

/// 解析出的地址 IPv4 排前面，IPv6 留作后备。
/// Windows 有 IPv6 地址时系统把 IPv6 排前面，ureq 按顺序一个个连，超时按 1/2、1/4… 分给各地址；
/// IPv6 不通又不报错（国内常见）时前两个 IPv6 白等大半时间，轮到 IPv4 已所剩无几。
/// 10-30 那台 Windows：同步常要 25、45 秒一轮，查更新（20 秒）一直「timeout: global」。
#[derive(Debug, Default)]
struct Ipv4First(ureq::unversioned::resolver::DefaultResolver);

impl ureq::unversioned::resolver::Resolver for Ipv4First {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: ureq::unversioned::transport::NextTimeout) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut addrs = self.0.resolve(uri, config, timeout)?;
        addrs.sort_by_key(|a| a.is_ipv6());
        Ok(addrs)
    }
}

/// 所有联网请求都用它建 Agent（先连 IPv4）
pub fn http_agent(config: ureq::config::Config) -> ureq::Agent {
    ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), Ipv4First::default())
}

fn agent() -> ureq::Agent {
    // 根证书用系统的（macOS 钥匙串、Windows 证书库）：ureq 默认的内置清单会关掉系统根证书，Windows 上验不过 Cloudflare 的证书链
    http_agent(
        ureq::Agent::config_builder()
            .proxy(None)
            .tls_config(beayy_core::tls!())
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build(),
    )
}

fn err_of(mut r: ureq::http::Response<ureq::Body>) -> String {
    let status = r.status().as_u16();
    let msg = r.body_mut().read_json::<Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_default();
    format!("服务器返回 {status} {msg}")
}



/// 服务器上这个账号的设备：[{device, segments, bytes, updated}]
pub fn remote_devices(a: &Account) -> Result<Vec<Value>, String> {
    if let Some(store) = a.remote() {
        let mut m: BTreeMap<String, (u64, u64, i64)> = BTreeMap::new();
        for it in store.items("", false)? {
            let e = m.entry(it["device"].as_str().unwrap_or("").to_string()).or_default();
            e.0 += 1;
            e.1 += it["size"].as_u64().unwrap_or(0);
            e.2 = e.2.max(it["updated"].as_i64().unwrap_or(0));
        }
        return Ok(m.into_iter().map(|(d, (n, b, u))| json!({"device": d, "segments": n, "bytes": b, "updated": u})).collect());
    }
    let r = agent().get(&format!("{}/v1/devices", a.server)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    let mut r = r;
    let v: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(v["devices"].as_array().cloned().unwrap_or_default())
}

// ---------- 账号（v2）：邮箱＋主密码 ----------

/// 主密码 → (登录校验值 hex, 包钥)。Argon2id m=64MB t=3 p=1，盐由服务器存（每个账号随机 16 字节）
fn derive(password: &str, salt_hex: &str) -> Result<(String, [u8; 32]), String> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let salt = hex_decode(salt_hex).ok_or("盐不对")?;
    let mut master = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(65536, 3, 1, Some(32)).map_err(|e| e.to_string())?)
        .hash_password_into(password.as_bytes(), &salt, &mut master)
        .map_err(|e| e.to_string())?;
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &master);
    let (mut auth, mut wrap) = ([0u8; 32], [0u8; 32]);
    hk.expand(b"beayy auth", &mut auth).map_err(|_| "派生失败")?;
    hk.expand(b"beayy wrap", &mut wrap).map_err(|_| "派生失败")?;
    Ok((hex_encode(&auth), wrap))
}

/// 恢复码的校验值：从数据密钥派生（服务器只存它的哈希）
fn recovery_auth(key: &[u8; 32]) -> String {
    let mut out = [0u8; 32];
    let _ = hkdf::Hkdf::<sha2::Sha256>::new(None, key).expand(b"beayy recovery", &mut out);
    hex_encode(&out)
}

fn wrap_key(data: &[u8; 32], wrap: &[u8; 32]) -> Result<String, String> {
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let ct = XChaCha20Poly1305::new(wrap.into())
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: data, aad: b"beayy-data-key" })
        .map_err(|_| "加密失败")?;
    Ok(B64.encode([nonce.to_vec(), ct].concat()))
}

fn unwrap_key(wrapped: &str, wrap: &[u8; 32]) -> Result<[u8; 32], String> {
    let b = B64.decode(wrapped).map_err(|_| "数据密钥坏了")?;
    if b.len() < 24 {
        return Err("数据密钥坏了".into());
    }
    let pt = XChaCha20Poly1305::new(wrap.into())
        .decrypt(XNonce::from_slice(&b[..24]), Payload { msg: &b[24..], aad: b"beayy-data-key" })
        .map_err(|_| "主密码不对，解不开数据密钥")?;
    pt.try_into().map_err(|_| "数据密钥长度不对".to_string())
}

fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect()
}

const B32: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // 去掉 I O 0 1，抄写不易错

/// 恢复码：数据密钥的 base32，每 4 个字一组
pub fn recovery_code_of(key: &[u8; 32]) -> String {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = String::new();
    for &b in key {
        bits = (bits << 8) | b as u32;
        n += 8;
        while n >= 5 {
            out.push(B32[((bits >> (n - 5)) & 31) as usize] as char);
            n -= 5;
        }
    }
    if n > 0 {
        out.push(B32[((bits << (5 - n)) & 31) as usize] as char);
    }
    out.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
}

fn parse_recovery(code: &str) -> Result<[u8; 32], String> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = vec![];
    for c in code.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let c = c.to_ascii_uppercase();
        let v = B32.iter().position(|x| *x as char == c).ok_or("恢复码里有不认识的字")?;
        bits = (bits << 5) | v as u32;
        n += 5;
        if n >= 8 {
            out.push(((bits >> (n - 8)) & 255) as u8);
            n -= 8;
        }
    }
    out.truncate(32);
    out.try_into().map_err(|_| "恢复码长度不对".to_string())
}

fn post_json(ag: &ureq::Agent, url: &str, auth: Option<&str>, body: &Value) -> Result<Value, String> {
    let mut rq = ag.post(url);
    if let Some(a) = auth {
        rq = rq.header("Authorization", a);
    }
    let r = rq.send_json(body).map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    let mut r = r;
    r.body_mut().read_json().map_err(|e| e.to_string())
}

fn new_salt() -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|e| e.to_string())?;
    Ok(hex_encode(&salt))
}

/// 注册前问服务器：要不要邀请码、每个账号能存多少。老服务器没有这个接口，按要邀请码算
pub fn signup_info(server: &str) -> Value {
    match agent().get(&format!("{}/v2/signup", server.trim_end_matches('/'))).call() {
        Ok(mut r) if r.status().is_success() => r.body_mut().read_json().unwrap_or(json!({"invite": true})),
        _ => json!({"invite": true}),
    }
}

/// 新账号：生成数据密钥，主密码包一层存上去；返回恢复码（只在这时给用户看一次，之后设置里也能再看）
pub fn signup(root: &Path, server: &str, invite: &str, email: &str, password: &str, device: &str, name: &str) -> Result<String, String> {
    let a = signup_account(server, invite, email, password, device, name)?;
    save(root, &a)?;
    reset_state_for(root, &a.user);
    drop_service(root);
    Ok(recovery_code_of(&a.key))
}

fn signup_account(server: &str, invite: &str, email: &str, password: &str, device: &str, name: &str) -> Result<Account, String> {
    let server = server.trim_end_matches('/');
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    let salt = new_salt()?;
    let (auth, wrap) = derive(password, &salt)?;
    let v = post_json(&agent(), &format!("{server}/v2/register"), None, &json!({
        "invite": invite, "email": email, "salt": salt, "auth": auth, "wrapped_key": wrap_key(&key, &wrap)?,
        "recovery": recovery_auth(&key), "device": device, "name": name,
    }))?;
    Ok(Account {
        server: server.into(),
        user: v["user"].as_str().ok_or("注册结果缺账号")?.into(),
        email: Some(email.trim().to_lowercase()),
        token: v["token"].as_str().ok_or("注册结果缺令牌")?.into(),
        key,
    })
}

/// 登录已有账号：取盐 → 派生 → 拿设备令牌和包好的数据密钥 → 本机解开
pub fn login(root: &Path, server: &str, email: &str, password: &str, device: &str, name: &str) -> Result<(), String> {
    let a = login_account(server, email, password, device, name)?;
    save(root, &a)?;
    reset_state_for(root, &a.user);
    drop_service(root);
    Ok(())
}

fn login_account(server: &str, email: &str, password: &str, device: &str, name: &str) -> Result<Account, String> {
    let server = server.trim_end_matches('/');
    let ag = agent();
    let email = email.trim().to_lowercase();
    let mut r = ag.get(&format!("{server}/v2/prelogin?email={}", url_escape(&email))).call().map_err(|e| e.to_string())?;
    let pre: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
    let (auth, wrap) = derive(password, pre["salt"].as_str().unwrap_or(""))?;
    let v = post_json(&ag, &format!("{server}/v2/login"), None, &json!({"email": email, "auth": auth, "device": device, "name": name}))?;
    let key = unwrap_key(v["wrapped_key"].as_str().unwrap_or(""), &wrap)?;
    Ok(Account { server: server.into(), user: v["user"].as_str().unwrap_or("").into(), email: Some(email), token: v["token"].as_str().unwrap_or("").into(), key })
}

/// v1 配对码账号设邮箱和主密码：同一把数据密钥包一层存上去，已上传的不用重传；这台设备换成自己的令牌。返回恢复码
pub fn claim(root: &Path, email: &str, password: &str, device: &str, name: &str) -> Result<String, String> {
    let a = load(root).ok_or("这台电脑还没开同步")?;
    let salt = new_salt()?;
    let (auth, wrap) = derive(password, &salt)?;
    let v = post_json(&agent(), &format!("{}/v2/claim", a.server), Some(&a.auth()), &json!({
        "email": email, "salt": salt, "auth": auth, "wrapped_key": wrap_key(&a.key, &wrap)?,
        "recovery": recovery_auth(&a.key), "device": device, "name": name,
    }))?;
    let b = Account { email: Some(email.trim().to_lowercase()), token: v["token"].as_str().ok_or("缺令牌")?.into(), ..a };
    save(root, &b)?;
    Ok(recovery_code_of(&b.key))
}

/// 忘了主密码：恢复码＋新主密码。之前登录的设备会全部登出，这台重新登录
pub fn recover(root: &Path, server: &str, email: &str, code: &str, new_password: &str, device: &str, name: &str) -> Result<(), String> {
    let server = server.trim_end_matches('/');
    let key = parse_recovery(code)?;
    let salt = new_salt()?;
    let (auth, wrap) = derive(new_password, &salt)?;
    post_json(&agent(), &format!("{server}/v2/recover"), None, &json!({
        "email": email.trim().to_lowercase(), "recovery": recovery_auth(&key), "salt": salt, "auth": auth, "wrapped_key": wrap_key(&key, &wrap)?,
    }))?;
    login(root, server, email, new_password, device, name)
}

/// 单为官方服务登的账号忘了主密码：同 recover，重设后登进 service
pub fn service_recover(root: &Path, server: &str, email: &str, code: &str, new_password: &str, device: &str, name: &str) -> Result<(), String> {
    let server = server.trim_end_matches('/');
    let key = parse_recovery(code)?;
    let salt = new_salt()?;
    let (auth, wrap) = derive(new_password, &salt)?;
    post_json(&agent(), &format!("{server}/v2/recover"), None, &json!({
        "email": email.trim().to_lowercase(), "recovery": recovery_auth(&key), "salt": salt, "auth": auth, "wrapped_key": wrap_key(&key, &wrap)?,
    }))?;
    service_login(root, server, email, new_password, device, name)
}

/// 改主密码（已登录）：数据密钥不变，重新包一层
pub fn change_password(root: &Path, new_password: &str) -> Result<(), String> {
    let a = load(root).ok_or("没登录")?;
    if let Some(store) = a.remote() {
        return write_vault(&store, &a.user, &a.key, new_password);
    }
    let salt = new_salt()?;
    let (auth, wrap) = derive(new_password, &salt)?;
    post_json(&agent(), &format!("{}/v2/password", a.server), Some(&a.auth()), &json!({"salt": salt, "auth": auth, "wrapped_key": wrap_key(&a.key, &wrap)?}))?;
    Ok(())
}

/// 账号信息：邮箱、登录着的设备
pub fn account(root: &Path) -> Result<Value, String> {
    let a = load(root).ok_or("没登录")?;
    if a.remote().is_some() {
        // 文件夹、iCloud、WebDAV 没有「登录着的设备」，列写过东西的设备；官方服务和头像看单为官方服务登的账号
        let s = load_slot(root, SERVICE).map(|s| server_account(&s));
        let (services, profile) = match &s {
            Some(Ok(v)) => (v["services"].clone(), v["profile"].clone()),
            Some(Err(e)) => (json!({"plan": null, "error": e}), Value::Null),
            None => (json!({"plan": null, "error": "没登录账号"}), Value::Null),
        };
        return Ok(json!({"user": a.user, "email": null, "mode": a.mode(), "folder": a.folder().map(|d| d.display().to_string()), "webdav": a.webdav(),
            "sessions": [], "devices": remote_devices(&a)?, "profile": profile, "services": services, "service_email": service_email(root)}));
    }
    server_account(&a)
}

/// 不同步、只为官方服务登了账号：只有官方服务和头像
pub fn service_account(root: &Path) -> Result<Value, String> {
    let s = load_slot(root, SERVICE).ok_or("没登录账号")?;
    let v = server_account(&s)?;
    Ok(json!({"services": v["services"], "profile": v["profile"], "service_email": s.email}))
}

fn server_account(a: &Account) -> Result<Value, String> {
    let r = agent().get(&format!("{}/v2/account", a.server)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    let mut r = r;
    r.body_mut().read_json().map_err(|e| e.to_string())
}

/// 移除一台设备的登录（id＝"self" 就是这台退出）
pub fn remove_session(root: &Path, id: &str) -> Result<(), String> {
    let a = load(root).ok_or("没登录")?;
    if a.remote().is_some() {
        return if id == "self" { Ok(()) } else { Err("同步文件夹、iCloud、WebDAV 没有设备登录，要停用一台设备就在那台上退出".into()) };
    }
    let r = agent().delete(&format!("{}/v2/sessions/{id}", a.server)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    Ok(())
}

/// 退出登录：服务器上注销这台的令牌，本机忘掉账号
pub fn logout(root: &Path) {
    let _ = remove_session(root, "self");
    forget(root);
}

/// 存账号的显示名称和头像（data URL）；返回服务器记的更新时刻
pub fn set_profile(root: &Path, name: &str, avatar: Option<&str>) -> Result<i64, String> {
    // 文件夹、iCloud、WebDAV 模式存到单为官方服务登的账号上；都没有就只存在本机
    let Some(a) = service(root) else { return Ok(0) };
    let v = post_json(&agent(), &format!("{}/v2/profile", a.server), Some(&a.auth()), &json!({"name": name, "avatar": avatar}))?;
    Ok(v["updated"].as_i64().unwrap_or(0))
}

/// 这台电脑上的恢复码（数据密钥）
pub fn recovery_code(root: &Path) -> Option<String> {
    load(root).map(|a| recovery_code_of(&a.key))
}

fn url_escape(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

// ---------- 不经服务器的存放处：同步文件夹（含 iCloud）、WebDAV ----------

/// 同一套布局：<根>/vault.json（主密码包着的数据密钥）、<根>/blobs/<设备>/<块名>（加密块）。每台设备只写自己的目录
enum Store {
    Dir(PathBuf),
    Dav(Dav),
}

fn blob_rel(d: &str, n: &str) -> String {
    format!("blobs/{d}/{n}")
}

impl Store {
    /// 根在不在（文件夹没挂上就别同步，免得以为全删了）
    fn ready(&self) -> Result<(), String> {
        match self {
            Store::Dir(d) if !d.is_dir() => Err(format!("找不到同步文件夹 {}（网盘没挂上？）", d.display())),
            _ => Ok(()),
        }
    }

    fn path(d: &Path, rel: &str) -> PathBuf {
        rel.split('/').filter(|x| !x.is_empty()).fold(d.to_path_buf(), |p, x| p.join(x))
    }

    /// 读一个文件；没有返回 None
    fn read(&self, rel: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Store::Dir(d) => match fs::read(Self::path(d, rel)) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            Store::Dav(d) => d.get(rel),
        }
    }

    fn write(&self, rel: &str, data: &[u8]) -> Result<(), String> {
        match self {
            Store::Dir(d) => write_atomic(&Self::path(d, rel), data),
            Store::Dav(d) => d.put(rel, data),
        }
    }

    fn remove(&self, rel: &str) -> Result<(), String> {
        match self {
            Store::Dir(d) => {
                let _ = fs::remove_file(Self::path(d, rel));
                Ok(())
            }
            Store::Dav(d) => d.delete(rel),
        }
    }

    /// 全部块：[{device, segment, size, updated, etag?}]，按 updated 升序（同服务器 /v1/list 的样子）。fresh＝WebDAV 不用列表缓存
    fn items(&self, prefix: &str, fresh: bool) -> Result<Vec<Value>, String> {
        match self {
            Store::Dir(d) => Ok(folder_items(d, prefix)),
            Store::Dav(d) => Ok(d.list(fresh)?.into_iter().filter(|x| x["segment"].as_str().unwrap_or("").starts_with(prefix)).collect()),
        }
    }

    /// 开过同步没有。iCloud 在 iPhone 上没下载下来的文件是 .<名字>.icloud 占位，也算有（免得再开一份、两把钥匙）
    fn has_vault(&self) -> Result<bool, String> {
        match self {
            Store::Dir(d) => Ok(d.join("vault.json").exists() || d.join(".vault.json.icloud").exists()),
            Store::Dav(d) => Ok(d.get("vault.json")?.is_some()),
        }
    }

    /// 新开前建好根目录和 blobs/
    fn prepare(&self) -> Result<(), String> {
        match self {
            Store::Dir(d) => fs::create_dir_all(d).map_err(|e| format!("建不了文件夹：{e}")),
            Store::Dav(d) => {
                d.mkcol("")?;
                d.mkcol("blobs")
            }
        }
    }

    /// 存进账号的令牌：WebDAV 是 Basic 认证，文件夹没有
    fn token(&self) -> String {
        match self {
            Store::Dir(_) => String::new(),
            Store::Dav(d) => d.auth.trim_start_matches("Basic ").to_string(),
        }
    }
}

/// 先写临时文件再改名：网盘同步时不会传半截文件
fn write_atomic(p: &Path, data: &[u8]) -> Result<(), String> {
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = p.with_file_name(format!(".{}.tmp", p.file_name().unwrap().to_string_lossy()));
    fs::write(&tmp, data).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}

fn mtime_ms(m: &fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// 文件夹里的全部块：[{device, segment, size, updated}]，按 updated 升序。. 开头的（临时文件、iCloud 占位）跳过
fn folder_items(dir: &Path, prefix: &str) -> Vec<Value> {
    let mut out = vec![];
    for dev in fs::read_dir(dir.join("blobs")).into_iter().flatten().flatten() {
        let d = dev.file_name().to_string_lossy().to_string();
        if !safe_name(&d) {
            continue;
        }
        for f in fs::read_dir(dev.path()).into_iter().flatten().flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if !safe_name(&n) || n.starts_with('.') || !n.starts_with(prefix) {
                continue;
            }
            if let Ok(m) = f.metadata() {
                out.push(json!({"device": d, "segment": n, "size": m.len(), "updated": mtime_ms(&m)}));
            }
        }
    }
    out.sort_by_key(|x| x["updated"].as_i64().unwrap_or(0));
    out
}

fn expand_home(path: &str) -> PathBuf {
    let path = path.trim();
    match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
}

// ---------- WebDAV ----------

/// 限流、连不上这类过一会儿就好的错，带上这句；同步遇到就停下这一轮
const BUSY: &str = "，下一轮再试";
/// 列表缓存多久：同步、取录音停留点每轮都要列，坚果云免费版限请求次数
const DAV_FRESH: Duration = Duration::from_secs(60);
static DAV_LIST: Mutex<Option<(String, std::time::Instant, Vec<Value>)>> = Mutex::new(None);
const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/><d:getetag/></d:prop></d:propfind>"#;

#[derive(Clone)]
struct Dav {
    /// 不带结尾的 /，路径里的中文、空格已转成 %xx
    base: String,
    /// Authorization 头：Basic <base64(用户:密码)>
    auth: String,
}

fn is_busy(e: &str) -> bool {
    e.contains(BUSY)
}

fn dav_status(s: u16) -> String {
    match s {
        401 | 403 => format!("WebDAV 返回 {s}：用户名或密码不对，或没有这个目录的权限（坚果云要用「第三方应用管理」里生成的应用密码）"),
        404 | 409 => format!("WebDAV 返回 {s}：地址里的上一级目录不在"),
        423 | 429 | 502 | 503 | 504 => format!("WebDAV 返回 {s}：服务器忙，或请求次数到了上限{BUSY}"),
        507 => "WebDAV 返回 507：空间满了".into(),
        s => format!("WebDAV 返回 {s}"),
    }
}

/// WebDAV 地址：去掉结尾的 /，路径里的中文、空格转成 %xx（ureq 只收合法的地址）
fn dav_base(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    if !(u.starts_with("https://") || u.starts_with("http://")) {
        return Err("WebDAV 地址要以 https:// 开头".into());
    }
    let enc: String = u.bytes().map(|b| if b.is_ascii_graphic() && !b"\"<>\\^`{|}".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    enc.parse::<ureq::http::Uri>().map_err(|_| "WebDAV 地址不对".to_string())?;
    Ok(enc)
}

fn dav_agent() -> &'static ureq::Agent {
    static AG: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AG.get_or_init(|| {
        http_agent(
            ureq::Agent::config_builder()
                .proxy(None)
                .tls_config(beayy_core::tls!())
                .timeout_global(Some(Duration::from_secs(60)))
                .http_status_as_error(false)
                .allow_non_standard_methods(true) // PROPFIND、MKCOL
                .build(),
        )
    })
}

impl Dav {
    fn new(url: &str, user: &str, pass: &str) -> Result<Dav, String> {
        Ok(Dav { base: dav_base(url)?, auth: format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{}:{pass}", user.trim()))) })
    }

    fn call(&self, method: &str, rel: &str, depth: Option<&str>, body: Option<&[u8]>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let m = ureq::http::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        // 块名、设备名可能是中文（win-小栗子），按段编码，/ 保留
        let rel = rel.split('/').map(url_escape).collect::<Vec<_>>().join("/");
        let mut b = ureq::http::Request::builder().method(m).uri(format!("{}/{rel}", self.base)).header("Authorization", &self.auth);
        if let Some(d) = depth {
            b = b.header("Depth", d).header("Content-Type", "application/xml; charset=utf-8");
        } else if body.is_some() {
            b = b.header("Content-Type", "application/octet-stream");
        }
        let r = match body {
            Some(body) => dav_agent().run(b.body(body).map_err(|e| e.to_string())?),
            None => dav_agent().run(b.body(()).map_err(|e| e.to_string())?),
        };
        r.map_err(|e| format!("连不上 WebDAV：{e}{BUSY}"))
    }

    fn get(&self, rel: &str) -> Result<Option<Vec<u8>>, String> {
        let mut r = self.call("GET", rel, None, None)?;
        match r.status().as_u16() {
            404 => Ok(None),
            200..=299 => r.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map(Some).map_err(|e| format!("WebDAV 读到一半断了：{e}{BUSY}")),
            s => Err(dav_status(s)),
        }
    }

    fn put(&self, rel: &str, data: &[u8]) -> Result<(), String> {
        let s = self.call("PUT", rel, None, Some(data))?.status().as_u16();
        if (200..300).contains(&s) {
            return Ok(());
        }
        // 上一级目录还没有（409，有的服务器回 404）：建好再传一次
        if s != 409 && s != 404 {
            return Err(dav_status(s));
        }
        let parts: Vec<&str> = rel.split('/').collect();
        for i in 1..parts.len() {
            self.mkcol(&parts[..i].join("/"))?;
        }
        match self.call("PUT", rel, None, Some(data))?.status().as_u16() {
            200..=299 => Ok(()),
            s => Err(dav_status(s)),
        }
    }

    /// 建目录；已经有了（405）也算成
    fn mkcol(&self, rel: &str) -> Result<(), String> {
        let rel = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        match self.call("MKCOL", &rel, None, None)?.status().as_u16() {
            200..=299 | 301 | 405 => Ok(()),
            s => Err(dav_status(s)),
        }
    }

    fn delete(&self, rel: &str) -> Result<(), String> {
        match self.call("DELETE", rel, None, None)?.status().as_u16() {
            200..=299 | 404 => Ok(()),
            s => Err(dav_status(s)),
        }
    }

    /// 列一个目录（Depth 1，含它自己）；目录不在返回空
    fn propfind(&self, rel: &str) -> Result<Vec<DavEntry>, String> {
        let mut r = self.call("PROPFIND", rel, Some("1"), Some(PROPFIND_BODY.as_bytes()))?;
        match r.status().as_u16() {
            404 => Ok(vec![]),
            200..=299 => Ok(dav_parse(&r.body_mut().with_config().limit(64 * 1024 * 1024).read_to_string().map_err(|e| format!("WebDAV 列表读到一半断了：{e}{BUSY}"))?)),
            s => Err(dav_status(s)),
        }
    }

    /// 全部块：先列 blobs/ 下的设备目录，再列每台设备（1＋设备数 个请求）；fresh 以外 60 秒内用缓存
    fn list(&self, fresh: bool) -> Result<Vec<Value>, String> {
        if !fresh {
            if let Some((b, t, v)) = DAV_LIST.lock().unwrap().as_ref() {
                if *b == self.base && t.elapsed() < DAV_FRESH {
                    return Ok(v.clone());
                }
            }
        }
        let mut out = vec![];
        for dev in self.propfind("blobs/")? {
            // 列表里也有 blobs 自己
            if !dev.dir || dev.name == "blobs" || !safe_name(&dev.name) {
                continue;
            }
            for f in self.propfind(&format!("blobs/{}/", dev.name))? {
                if f.dir || !safe_name(&f.name) || f.name.starts_with('.') {
                    continue;
                }
                out.push(json!({"device": dev.name, "segment": f.name, "size": f.size, "updated": f.updated, "etag": f.etag}));
            }
        }
        out.sort_by_key(|x| x["updated"].as_i64().unwrap_or(0));
        *DAV_LIST.lock().unwrap() = Some((self.base.clone(), std::time::Instant::now(), out.clone()));
        Ok(out)
    }
}

#[derive(Default, Debug)]
struct DavEntry {
    name: String,
    dir: bool,
    size: u64,
    updated: i64,
    etag: Option<String>,
}

/// PROPFIND 的回复（207 multistatus）：不管命名空间前缀叫什么（d:、D:、lp1:、没有），按标签的本地名取 href、
/// resourcetype 里的 collection、getcontentlength、getlastmodified（RFC 1123）、getetag
fn dav_parse(xml: &str) -> Vec<DavEntry> {
    let mut out = vec![];
    let mut cur: Option<DavEntry> = None;
    let mut text = String::new();
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        text.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let end = tag.starts_with('/');
        let empty = tag.ends_with('/');
        let name = tag.trim_start_matches('/').trim_end_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or("").to_ascii_lowercase();
        if !end {
            text.clear();
            match local.as_str() {
                "response" if !empty => cur = Some(DavEntry::default()),
                "collection" => {
                    if let Some(e) = cur.as_mut() {
                        e.dir = true;
                    }
                }
                _ => {}
            }
            continue;
        }
        let t = xml_unescape(text.trim());
        text.clear();
        match (local.as_str(), cur.as_mut()) {
            ("response", _) => out.extend(cur.take()),
            ("href", Some(e)) => {
                let path = t.split_once("://").map(|(_, r)| r.find('/').map(|i| &r[i..]).unwrap_or("/")).unwrap_or(&t);
                e.name = pct_decode(path.trim_end_matches('/').rsplit('/').next().unwrap_or(""));
            }
            ("getcontentlength", Some(e)) => e.size = t.parse().unwrap_or(0),
            ("getlastmodified", Some(e)) => {
                e.updated = chrono::DateTime::parse_from_rfc2822(&t).or_else(|_| chrono::DateTime::parse_from_rfc3339(&t)).map(|d| d.timestamp_millis()).unwrap_or(0)
            }
            ("getetag", Some(e)) if !t.is_empty() => e.etag = Some(t),
            _ => {}
        }
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#34;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

// ---------- iCloud ----------

/// iCloud 里放同步的目录：<容器>/Sync。
/// iPhone：Swift 用 FileManager.url(forUbiquityContainerIdentifier:) 解析容器，写进 <数据根>/icloud_container（没开 iCloud 就删掉）。
/// Mac：容器在 ~/Library/Mobile Documents/iCloud~com~fengli~beayy。Mac 版没登记这个容器（Developer ID 要另配描述文件），
/// 自己建不了（Mobile Documents 是只读的），要 iPhone 先用过、iCloud 把它同步到这台 Mac；建好以后在里面读写是普通文件，iCloud 云盘照常同步。
/// 环境变量 BEAYY_ICLOUD_DIR 可以直接指定容器（测试用）
pub fn icloud_dir() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("BEAYY_ICLOUD_DIR") {
        return Ok(PathBuf::from(p).join(ICLOUD_SUB));
    }
    #[cfg(target_os = "ios")]
    {
        let p = fs::read_to_string(beayy_core::default_root().join("icloud_container")).map(|s| s.trim().to_string()).unwrap_or_default();
        if p.is_empty() {
            return Err("这台 iPhone 没开 iCloud 云盘，或没登录 Apple 账户：设置 → Apple 账户 → iCloud → iCloud 云盘".into());
        }
        Ok(PathBuf::from(p).join(ICLOUD_SUB))
    }
    #[cfg(target_os = "macos")]
    {
        let docs = expand_home("~/Library/Mobile Documents");
        let c = docs.join(ICLOUD_CONTAINER.replace('.', "~"));
        if c.is_dir() {
            Ok(c.join(ICLOUD_SUB))
        } else if !docs.is_dir() {
            Err("这台 Mac 没开 iCloud 云盘：系统设置 → Apple 账户 → iCloud → iCloud 云盘".into())
        } else {
            Err("iCloud 里还没有 Beayy 的位置：先在 iPhone 的 Beayy 里选 iCloud 新开同步，等一两分钟同步到这台 Mac 再来".into())
        }
    }
    #[cfg(not(any(target_os = "ios", target_os = "macos")))]
    Err("只有 Mac 和 iPhone 能用 iCloud 同步".into())
}

// ---------- 新开、加入、找回（文件夹、iCloud、WebDAV 共用） ----------

/// 第一台设备：生成数据密钥，用主密码包一层写进 vault.json；返回恢复码
fn store_create(root: &Path, store: &Store, server: String, password: &str) -> Result<String, String> {
    if store.has_vault()? {
        return Err("这里已经开过同步了，用「加入」".into());
    }
    store.prepare()?;
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    let mut uid = [0u8; 12];
    getrandom::getrandom(&mut uid).map_err(|e| e.to_string())?;
    let user = hex_encode(&uid);
    write_vault(store, &user, &key, password)?;
    let a = Account { server, user, email: None, token: store.token(), key };
    save(root, &a)?;
    reset_state_for(root, &a.user);
    Ok(recovery_code_of(&key))
}

/// 别的设备：加入已有的同步，用主密码解开数据密钥
fn store_join(root: &Path, store: &Store, server: String, password: &str) -> Result<(), String> {
    let v = read_vault(store)?;
    let (_, wrap) = derive(password, v["salt"].as_str().unwrap_or(""))?;
    let key = unwrap_key(v["wrapped_key"].as_str().unwrap_or(""), &wrap)?;
    let a = Account { server, user: v["user"].as_str().unwrap_or("").into(), email: None, token: store.token(), key };
    save(root, &a)?;
    reset_state_for(root, &a.user);
    Ok(())
}

/// 忘了主密码：恢复码＋新主密码，重包 vault.json 并加入
fn store_recover(root: &Path, store: &Store, server: String, code: &str, new_password: &str) -> Result<(), String> {
    let v = read_vault(store)?;
    let key = parse_recovery(code)?;
    if v["recovery"].as_str() != Some(recovery_auth(&key).as_str()) {
        return Err("恢复码不对".into());
    }
    let user = v["user"].as_str().unwrap_or("").to_string();
    write_vault(store, &user, &key, new_password)?;
    let a = Account { server, user, email: None, token: store.token(), key };
    save(root, &a)?;
    reset_state_for(root, &a.user);
    Ok(())
}

fn write_vault(store: &Store, user: &str, key: &[u8; 32], password: &str) -> Result<(), String> {
    let salt = new_salt()?;
    let (_, wrap) = derive(password, &salt)?;
    let v = json!({"v": 1, "user": user, "salt": salt, "kdf": {"alg": "argon2id", "m": 65536, "t": 3, "p": 1},
        "wrapped_key": wrap_key(key, &wrap)?, "recovery": recovery_auth(key)});
    store.write("vault.json", serde_json::to_string_pretty(&v).unwrap().as_bytes())
}

fn read_vault(store: &Store) -> Result<Value, String> {
    let b = store.read("vault.json")?.ok_or("这里还没开同步（没有 vault.json）；网盘、iCloud 还没同步下来的话，等一会儿再试")?;
    serde_json::from_slice(&b).map_err(|_| "vault.json 坏了".to_string())
}

// ---------- 同步文件夹 ----------

/// 这个文件夹里有没有已经开好的同步（设置里选文件夹后，账号页据此给「新建」还是「加入」）
pub fn folder_state(path: &str) -> Value {
    let dir = expand_home(path);
    json!({"path": dir.display().to_string(), "exists": dir.is_dir(), "vault": Store::Dir(dir.clone()).has_vault().unwrap_or(false)})
}

fn folder_server(dir: &Path) -> String {
    format!("{FOLDER}{}", dir.display())
}

pub fn folder_create(root: &Path, path: &str, password: &str) -> Result<String, String> {
    let dir = expand_home(path);
    store_create(root, &Store::Dir(dir.clone()), folder_server(&dir), password)
}

pub fn folder_join(root: &Path, path: &str, password: &str) -> Result<(), String> {
    let dir = expand_home(path);
    store_join(root, &Store::Dir(dir.clone()), folder_server(&dir), password)
}

pub fn folder_recover(root: &Path, path: &str, code: &str, new_password: &str) -> Result<(), String> {
    let dir = expand_home(path);
    store_recover(root, &Store::Dir(dir.clone()), folder_server(&dir), code, new_password)
}

// ---------- iCloud（就是放在 iCloud 容器里的同步文件夹） ----------

/// {available, path?, vault, error?}：这台设备能不能用 iCloud、里面开没开过同步
pub fn icloud_state() -> Value {
    match icloud_dir() {
        Ok(d) => json!({"available": true, "path": d.display().to_string(), "vault": Store::Dir(d).has_vault().unwrap_or(false)}),
        Err(e) => json!({"available": false, "vault": false, "error": e}),
    }
}

pub fn icloud_create(root: &Path, password: &str) -> Result<String, String> {
    let dir = icloud_dir()?;
    store_create(root, &Store::Dir(dir.clone()), format!("{ICLOUD}{}", dir.display()), password)
}

pub fn icloud_join(root: &Path, password: &str) -> Result<(), String> {
    let dir = icloud_dir()?;
    store_join(root, &Store::Dir(dir.clone()), format!("{ICLOUD}{}", dir.display()), password)
}

pub fn icloud_recover(root: &Path, code: &str, new_password: &str) -> Result<(), String> {
    let dir = icloud_dir()?;
    store_recover(root, &Store::Dir(dir.clone()), format!("{ICLOUD}{}", dir.display()), code, new_password)
}

// ---------- WebDAV（坚果云、NAS、Nextcloud） ----------

/// {ok, vault, error?}：连不连得上、里面开没开过同步
pub fn webdav_state(url: &str, user: &str, pass: &str) -> Value {
    match Dav::new(url, user, pass).and_then(|d| d.get("vault.json")) {
        Ok(v) => json!({"ok": true, "vault": v.is_some()}),
        Err(e) => json!({"ok": false, "vault": false, "error": e}),
    }
}

pub fn webdav_create(root: &Path, url: &str, user: &str, pass: &str, password: &str) -> Result<String, String> {
    let d = Dav::new(url, user, pass)?;
    let server = format!("{WEBDAV}{}", d.base);
    store_create(root, &Store::Dav(d), server, password)
}

pub fn webdav_join(root: &Path, url: &str, user: &str, pass: &str, password: &str) -> Result<(), String> {
    let d = Dav::new(url, user, pass)?;
    let server = format!("{WEBDAV}{}", d.base);
    store_join(root, &Store::Dav(d), server, password)
}

pub fn webdav_recover(root: &Path, url: &str, user: &str, pass: &str, code: &str, new_password: &str) -> Result<(), String> {
    let d = Dav::new(url, user, pass)?;
    let server = format!("{WEBDAV}{}", d.base);
    store_recover(root, &Store::Dav(d), server, code, new_password)
}

/// 文件夹、iCloud、WebDAV 的下载：别的设备写的块，大小或修改时刻（WebDAV 另有 etag）跟上次下的不一样就下（不靠游标，见文件开头）。
/// 64 块一批、4 路并行读和解密，再按顺序写盘；每批记一次进度。WebDAV 限流、连不上就停下这一轮
fn store_download(root: &Path, a: &Account, store: &Store, local_device: &str, st: &mut State, errors: &mut Vec<String>) -> usize {
    let items = match store.items("", true) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("列文件：{e}"));
            return 0;
        }
    };
    let mut seen: Map<String, Value> = st.raw.get("down").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut todo = vec![];
    for it in items {
        let (d, n) = (it["device"].as_str().unwrap_or("").to_string(), it["segment"].as_str().unwrap_or("").to_string());
        let mine = d == local_device || (d.starts_with("iphone-") && root.join("devices").join(&d).exists() && !is_remote(root, st, &d));
        if mine || is_special(&d, &n) {
            continue;
        }
        let sig = if it["etag"].is_string() { json!([it["size"], it["updated"], it["etag"]]) } else { json!([it["size"], it["updated"]]) };
        if seen.get(&format!("{d}/{n}")) != Some(&sig) {
            todo.push((d, n, sig));
        }
    }
    set_phase("download", todo.len());
    let mut downloaded = 0;
    const LANES: usize = 4;
    for batch in todo.chunks(64) {
        let got: Vec<Result<Vec<u8>, String>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..LANES)
                .map(|lane| {
                    s.spawn(move || {
                        batch
                            .iter()
                            .enumerate()
                            .skip(lane)
                            .step_by(LANES)
                            .map(|(i, (d, n, _))| (i, store.read(&blob_rel(d, n)).and_then(|b| b.ok_or_else(|| "文件不见了".to_string())).and_then(|b| open(a, d, n, &b))))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut out: Vec<Result<Vec<u8>, String>> = (0..batch.len()).map(|_| Err(String::new())).collect();
            for h in hs {
                for (i, r) in h.join().unwrap_or_default() {
                    out[i] = r;
                }
            }
            out
        });
        let mut busy = false;
        for ((d, n, sig), r) in batch.iter().zip(got) {
            DONE.fetch_add(1, Ordering::Relaxed);
            match r.and_then(|plain| store_file(root, d, n, plain)) {
                Ok(_) => {
                    st.remote.insert(d.clone());
                    seen.insert(format!("{d}/{n}"), sig.clone());
                    downloaded += 1;
                }
                // 网盘还在传的半截文件解不开、WebDAV 限流：不记，下一轮再试
                Err(e) => {
                    busy |= is_busy(&e);
                    if errors.len() < 10 {
                        errors.push(format!("下载 {d}/{n}：{e}"));
                    }
                }
            }
        }
        st.raw.insert("down".into(), Value::Object(seen.clone()));
        write_state(root, st);
        if busy {
            break;
        }
    }
    downloaded
}

// ---------- 加密 ----------

fn seal(a: &Account, device: &str, segment: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    let packed = miniz_oxide::deflate::compress_to_vec(plain, 6);
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let aad = format!("{device}/{segment}");
    let ct = a.cipher().encrypt(XNonce::from_slice(&nonce), Payload { msg: &packed, aad: aad.as_bytes() }).map_err(|_| "加密失败")?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(out)
}

fn open(a: &Account, device: &str, segment: &str, blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 24 {
        return Err("块太短".into());
    }
    let aad = format!("{device}/{segment}");
    let packed = a
        .cipher()
        .decrypt(XNonce::from_slice(&blob[..24]), Payload { msg: &blob[24..], aad: aad.as_bytes() })
        .map_err(|_| "解密失败：密钥不对，或块被改过")?;
    miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, 512 * 1024 * 1024).map_err(|_| "解压失败".to_string())
}

// ---------- 状态 ----------

fn state_path(root: &Path) -> PathBuf {
    root.join("sync").join("state.json")
}

#[derive(Default)]
struct State {
    /// "设备/分段" → [大小, 修改时间秒]
    up: BTreeMap<String, (u64, u64)>,
    /// 下载游标：服务器上的更新时刻（毫秒）
    since: i64,
    /// 同步来的设备（不上传）
    remote: BTreeSet<String>,
    raw: Map<String, Value>,
}

fn read_state(root: &Path) -> State {
    let raw: Map<String, Value> = fs::read_to_string(state_path(root)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let up = raw
        .get("up")
        .and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), (v.get(0)?.as_u64()?, v.get(1)?.as_u64()?)))).collect())
        .unwrap_or_default();
    let remote = raw.get("remote").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    State { up, since: raw.get("since").and_then(Value::as_i64).unwrap_or(0), remote, raw }
}

fn write_state(root: &Path, s: &State) {
    let mut raw = s.raw.clone();
    raw.insert("up".into(), Value::Object(s.up.iter().map(|(k, (a, b))| (k.clone(), json!([a, b]))).collect()));
    raw.insert("since".into(), json!(s.since));
    raw.insert("remote".into(), json!(s.remote));
    let _ = fs::create_dir_all(root.join("sync"));
    let tmp = root.join("sync").join("state.json.tmp");
    if fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(raw)).unwrap_or_default()).is_ok() {
        let _ = fs::rename(tmp, state_path(root));
    }
}

/// 给页面看的同步状态
pub fn status(root: &Path) -> Value {
    let a = load(root);
    let s = read_state(root);
    json!({
        "configured": a.is_some(),
        "email": a.as_ref().and_then(|a| a.email.clone()),
        "mode": a.as_ref().map(|a| a.mode()),
        "server": a.as_ref().filter(|a| a.mode() == "server").map(|a| a.server.clone()),
        "folder": a.as_ref().and_then(|a| a.folder()).map(|p| p.display().to_string()),
        "webdav": a.as_ref().and_then(|a| a.webdav().map(String::from)),
        "user": a.as_ref().map(|a| a.user.clone()),
        "last": s.raw.get("last"),
        "last_error": s.raw.get("last_error"),
        "uploaded": s.up.len(),
        "need_login": s.raw.get("need_login").and_then(Value::as_bool).unwrap_or(false),
        "service_email": service_email(root),
        "remote_devices": remote_set(root),
        "progress": progress(),
    })
}

/// 哪些设备是同步来的（设备页标「经同步」）：状态里记的，加上目录里有 .synced 标记的
pub fn remote_set(root: &Path) -> BTreeSet<String> {
    let mut s = read_state(root).remote;
    if let Ok(it) = fs::read_dir(root.join("devices")) {
        for e in it.flatten() {
            if e.path().join(SYNCED).is_file() {
                s.insert(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    s
}

const SYNCED: &str = ".synced";
/// 设备改名的块名和本机文件名
const NAMES: &str = "names.json";
const NAMES_FILE: &str = "device_names.json";

fn read_names(p: &Path) -> Map<String, Value> {
    fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// 各设备的自定义名字（所有设备上改过的，按改名时刻取最新；改回空＝用默认名，不出现在这里）
pub fn device_names(root: &Path) -> BTreeMap<String, String> {
    let mut best: BTreeMap<String, (i64, String)> = BTreeMap::new();
    let mut files = vec![root.join(NAMES_FILE)];
    if let Ok(it) = fs::read_dir(root.join("devices")) {
        files.extend(it.flatten().map(|e| e.path().join(NAMES_FILE)));
    }
    for f in files {
        for (id, v) in read_names(&f) {
            let at = v["at"].as_i64().unwrap_or(0);
            if best.get(&id).map(|b| at > b.0).unwrap_or(true) {
                best.insert(id, (at, v["name"].as_str().unwrap_or("").to_string()));
            }
        }
    }
    best.into_iter().filter(|(_, (_, n))| !n.is_empty()).map(|(id, (_, n))| (id, n)).collect()
}

/// 给一台设备改名（空＝改回默认名）。写在本机的改名表里，下一轮同步给别的设备
pub fn rename_device(root: &Path, id: &str, name: &str) -> Result<(), String> {
    let name: String = name.trim().chars().take(40).collect();
    let f = root.join(NAMES_FILE);
    let mut m = read_names(&f);
    m.insert(id.to_string(), json!({"name": name, "at": chrono::Utc::now().timestamp_millis()}));
    fs::write(&f, serde_json::to_vec_pretty(&Value::Object(m)).unwrap_or_default()).map_err(|e| e.to_string())
}
/// 摘要块名的前缀（分段名都以年份开头，不会撞）
const SUM: &str = "sum.";
/// 日、周、月总结块名的前缀
const DAILY: &str = "daily.";
const DAILY_KINDS: [(&str, &str); 3] = [("d", "日总结_"), ("w", "周总结_"), ("m", "月总结_")];

/// 总结文件名 → 块名（日总结_20261028.md → daily.d.20261028.md）；不是总结返回 None
fn daily_block(file: &str) -> Option<String> {
    let (k, rest) = DAILY_KINDS.iter().find_map(|(k, pre)| file.strip_prefix(pre).map(|r| (*k, r)))?;
    let date = rest.strip_suffix(".md")?;
    (date.len() >= 6 && date.chars().all(|c| c.is_ascii_digit())).then(|| format!("{DAILY}{k}.{date}.md"))
}

/// 块名 → 总结文件名
fn daily_file(block: &str) -> Option<String> {
    let rest = block.strip_prefix(DAILY)?;
    let (k, date) = rest.split_once('.')?;
    let date = date.strip_suffix(".md")?;
    let pre = DAILY_KINDS.iter().find(|(x, _)| *x == k)?.1;
    (date.len() >= 6 && date.chars().all(|c| c.is_ascii_digit())).then(|| format!("{pre}{date}.md"))
}

fn is_remote(root: &Path, st: &State, d: &str) -> bool {
    st.remote.contains(d) || root.join("devices").join(d).join(SYNCED).is_file()
}

// ---------- 一轮同步 ----------

pub struct Report {
    pub uploaded: usize,
    pub downloaded: usize,
    pub errors: Vec<String>,
}

/// 设备名、块名：字母数字（含中文，Windows 计算机名可以是「小栗子」）和 . - _，不超过 80 个字，不能全是点
fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 80 && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')) && !s.chars().all(|c| c == '.')
}

/// 数据块的地址；名字要编码，原样发中文 Cloudflare 回 400
fn blob_url(server: &str, d: &str, n: &str) -> String {
    format!("{server}/v1/blob/{}/{}", url_escape(d), url_escape(n))
}

/// 一轮：先上传本机写的分段里变了的，再下载别的设备新传的。没开同步返回 None
// ---------- 这一轮进行到哪（设备页显示进度） ----------

static PHASE: Mutex<&'static str> = Mutex::new("");
static DONE: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn set_phase(phase: &'static str, total: usize) {
    *PHASE.lock().unwrap() = phase;
    DONE.store(0, Ordering::Relaxed);
    TOTAL.store(total, Ordering::Relaxed);
}

/// {phase: "upload"／"download"／""（没在同步）, done, total}
pub fn progress() -> Value {
    json!({"phase": *PHASE.lock().unwrap(), "done": DONE.load(Ordering::Relaxed), "total": TOTAL.load(Ordering::Relaxed)})
}

pub fn sync_once(root: &Path, local_device: &str) -> Option<Report> {
    let r = sync_round(root, local_device, true);
    set_phase("", 0);
    r
}

/// 只下载（手机）：手机自己不采集电脑那种分段，也不能把它算出的东西当成某台设备的分段传上去盖掉原来的
pub fn sync_download_only(root: &Path, local_device: &str) -> Option<Report> {
    let r = sync_round(root, local_device, false);
    set_phase("", 0);
    r
}

fn sync_round(root: &Path, local_device: &str, upload: bool) -> Option<Report> {
    let a = load(root)?;
    let mut st = read_state(root);
    let mut errors = vec![];

    // 1. 上传：本机设备＋本机读到的 iPhone（同步来的不传）
    // 官方服务只传本机电脑分段的精简版（见 lean）；头一回切到精简时，已传的完整版按精简版重传一遍，腾出空间
    let lean = lean(&a);
    if lean && st.raw.get("lean") != Some(&json!(true)) {
        let pre = format!("{local_device}/");
        st.up.retain(|k, _| !k.strip_prefix(&pre).map(|n| !n.contains('.')).unwrap_or(false));
        st.raw.insert("lean".into(), json!(true));
    } else if !lean {
        st.raw.remove("lean");
    }
    let mut slim: HashSet<String> = HashSet::new();
    let mut todo: Vec<(String, String, PathBuf, (u64, u64))> = vec![];
    let devs = fs::read_dir(root.join("devices")).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default();
    for dev in devs {
        let d = dev.file_name().to_string_lossy().to_string();
        let mine = d == local_device || (d.starts_with("iphone-") && !is_remote(root, &st, &d));
        if !upload || !mine || !safe_name(&d) || !dev.path().is_dir() {
            continue;
        }
        let segs = fs::read_dir(dev.path().join("segments")).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default();
        for seg in segs {
            let name = seg.file_name().to_string_lossy().to_string();
            let f = seg.path().join("events.jsonl");
            let Ok(meta) = fs::metadata(&f) else { continue };
            if !safe_name(&name) {
                continue;
            }
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            let sig = (meta.len(), mtime);
            if st.up.get(&format!("{d}/{name}")) != Some(&sig) {
                if lean && d == local_device {
                    slim.insert(format!("{d}/{name}"));
                }
                todo.push((d.clone(), name, f, sig));
            }
        }
    }
    // 本机写的 10 分钟摘要（<数据根>/summaries/），块名 sum.<文件名>
    for f in fs::read_dir(root.join("summaries")).ok().filter(|_| upload).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default() {
        let file = f.file_name().to_string_lossy().to_string();
        let name = format!("{SUM}{file}");
        if !file.ends_with("-memory-summary.md") || !safe_name(&name) {
            continue;
        }
        let Ok(meta) = f.metadata() else { continue };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let sig = (meta.len(), mtime);
        if st.up.get(&format!("{local_device}/{name}")) != Some(&sig) {
            todo.push((local_device.to_string(), name, f.path(), sig));
        }
    }
    // 本机写的日、周、月总结（<数据根>/daily/），块名 daily.<d|w|m>.<日期>.md
    for f in fs::read_dir(root.join("daily")).ok().filter(|_| upload).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default() {
        let Some(name) = daily_block(&f.file_name().to_string_lossy()) else { continue };
        let Ok(meta) = f.metadata() else { continue };
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let sig = (meta.len(), mtime);
        if st.up.get(&format!("{local_device}/{name}")) != Some(&sig) {
            todo.push((local_device.to_string(), name, f.path(), sig));
        }
    }
    // 本机的设备改名表
    if let Some(meta) = fs::metadata(root.join(NAMES_FILE)).ok().filter(|_| upload) {
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let sig = (meta.len(), mtime);
        if st.up.get(&format!("{local_device}/{NAMES}")) != Some(&sig) {
            todo.push((local_device.to_string(), NAMES.to_string(), root.join(NAMES_FILE), sig));
        }
    }
    todo.sort_by(|x, y| (&x.0, &x.1).cmp(&(&y.0, &y.1)));
    set_phase("upload", todo.len());
    let ag = agent();
    let store = a.remote();
    let store = store.as_ref();
    let stop = std::sync::atomic::AtomicBool::new(false);
    // WebDAV（坚果云）别一下开太多路
    let lanes = if matches!(store, Some(Store::Dav(_))) { 4 } else { PARALLEL };
    let mut uploaded = 0;
    // 每 200 段记一次进度：传到一半退出，下次接着传
    for batch in todo.chunks(200) {
        let results: Vec<(String, Result<(u64, u64), String>)> = std::thread::scope(|s| {
            let chunks: Vec<Vec<&(String, String, PathBuf, (u64, u64))>> = (0..lanes).map(|i| batch.iter().skip(i).step_by(lanes).collect()).collect();
            let hs: Vec<_> = chunks
                .into_iter()
                .map(|chunk| {
                    let (a, ag, stop, slim) = (&a, &ag, &stop, &slim);
                    s.spawn(move || {
                        chunk
                            .into_iter()
                            .map(|(d, n, f, sig)| {
                                let r = (|| {
                                    let mut plain = fs::read(f).map_err(|e| e.to_string())?;
                                    if slim.contains(&format!("{d}/{n}")) {
                                        plain = lean_segment(&plain);
                                    }
                                    let blob = seal(a, d, n, &plain)?;
                                    if let Some(store) = store {
                                        // WebDAV 限流、连不上：这一轮剩下的不传了，下一轮接着传
                                        if stop.load(Ordering::Relaxed) {
                                            return Err(format!("前面的出错了{BUSY}"));
                                        }
                                        return store.write(&blob_rel(d, n), &blob).map(|_| *sig).inspect_err(|e| {
                                            if is_busy(e) {
                                                stop.store(true, Ordering::Relaxed);
                                            }
                                        });
                                    }
                                    let resp = ag
                                        .put(&blob_url(&a.server, d, n))
                                        .header("Authorization", &a.auth())
                                        .header("Content-Type", "application/octet-stream")
                                        .send(&blob[..])
                                        .map_err(|e| e.to_string())?;
                                    if !resp.status().is_success() {
                                        return Err(err_of(resp));
                                    }
                                    Ok(*sig)
                                })();
                                DONE.fetch_add(1, Ordering::Relaxed);
                                (format!("{d}/{n}"), r)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        for (k, r) in results {
            match r {
                Ok(sig) => {
                    st.up.insert(k, sig);
                    uploaded += 1;
                }
                Err(e) => {
                    if errors.len() < 5 {
                        errors.push(format!("上传 {k}：{e}"));
                    }
                }
            }
        }
        // 中途出错也把已经传上去的记下来
        st.raw.insert("user".into(), json!(a.user));
        write_state(root, &st);
    }

    // 2. 下载：别的设备新传的
    let mut downloaded = 0;
    if let Some(store) = store {
        if let Err(e) = store.ready() {
            errors.push(e);
            return Some(finish(root, st, uploaded, downloaded, errors));
        }
        downloaded = store_download(root, &a, store, local_device, &mut st, &mut errors);
        return Some(finish(root, st, uploaded, downloaded, errors));
    }
    loop {
        let r = match ag.get(&format!("{}/v1/list?since={}", a.server, st.since)).header("Authorization", &a.auth()).call() {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                errors.push(format!("取更新列表：{}", err_of(r)));
                break;
            }
            Err(e) => {
                errors.push(format!("取更新列表：{e}"));
                break;
            }
        };
        let mut r = r;
        let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
        let items = v["items"].as_array().cloned().unwrap_or_default();
        let full = items.len() >= 5000;
        // 要下载的：别的设备的（本机电脑和本机读到的 iPhone 是自己传的）
        let wanted = |it: &Value| {
            let (d, n) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""));
            if is_special(d, n) {
                return false;
            }
            let mine = d == local_device || (d.starts_with("iphone-") && root.join("devices").join(d).exists() && !is_remote(root, &st, d));
            !mine && safe_name(d) && safe_name(n)
        };
        let want: Vec<bool> = items.iter().map(wanted).collect();
        set_phase("download", want.iter().filter(|w| **w).count());
        // 64 段一批：8 路并行下载、解密，再按顺序写盘、推进游标（遇到失败停在那一段，下一轮从那里接着下），每批记一次进度
        let rows: Vec<(&Value, bool)> = items.iter().zip(want).collect();
        for batch in rows.chunks(64) {
            let fetched: Vec<Option<Result<Vec<u8>, String>>> = std::thread::scope(|s| {
                let lanes: Vec<_> = (0..PARALLEL_DOWN)
                    .map(|lane| {
                        let (a, ag) = (&a, &ag);
                        s.spawn(move || {
                            batch
                                .iter()
                                .enumerate()
                                .skip(lane)
                                .step_by(PARALLEL_DOWN)
                                .map(|(i, (it, want))| {
                                    let (d, n) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""));
                                    let r = want.then(|| fetch(a, ag, d, n));
                                    if r.as_ref().map(|x| x.is_ok()).unwrap_or(false) {
                                        DONE.fetch_add(1, Ordering::Relaxed);
                                    }
                                    (i, r)
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                let mut out: Vec<Option<Result<Vec<u8>, String>>> = (0..batch.len()).map(|_| None).collect();
                for h in lanes {
                    for (i, r) in h.join().unwrap_or_default() {
                        out[i] = r;
                    }
                }
                out
            });
            for ((it, _), got) in batch.iter().zip(fetched) {
                let (d, n) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""));
                if let Some(got) = got {
                    match got.and_then(|plain| store_file(root, d, n, plain)) {
                        Ok(_) => {
                            st.remote.insert(d.to_string());
                            downloaded += 1;
                        }
                        Err(e) => {
                            if errors.len() < 10 {
                                errors.push(format!("下载 {d}/{n}：{e}"));
                            }
                            write_state(root, &st);
                            return Some(finish(root, st, uploaded, downloaded, errors));
                        }
                    }
                }
                st.since = st.since.max(it["updated"].as_i64().unwrap_or(0));
            }
            write_state(root, &st);
        }
        if !full {
            break;
        }
    }
    Some(finish(root, st, uploaded, downloaded, errors))
}

/// 连的是官方服务：电脑分段传精简版。屏幕文字（`ui.text` 的 text）占分段九成，只有本机写摘要用得着，
/// 别的设备看时间线、时长只要其余字段；同步文件夹、iCloud、网盘、自己部署的服务端照旧传完整的
fn lean(a: &Account) -> bool {
    a.remote().is_none() && a.server.trim_end_matches('/') == DEFAULT_SERVER
}

/// 分段去掉屏幕文字：`ui.text` 事件留着（时刻、app、窗口、网址），只删 text；别的行原样
fn lean_segment(plain: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(plain.len() / 4);
    for line in plain.split_inclusive(|b| *b == b'\n') {
        let slim = (|| {
            if !line.windows(9).any(|w| w == b"\"ui.text\"") {
                return None;
            }
            let mut v: Map<String, Value> = serde_json::from_slice(line).ok()?;
            if v.get("event_type").and_then(Value::as_str) != Some("ui.text") || v.remove("text").is_none() {
                return None;
            }
            let mut b = serde_json::to_vec(&v).ok()?;
            b.push(b'\n');
            Some(b)
        })();
        out.extend(slim.as_deref().unwrap_or(line));
    }
    out
}

// ---------- 录音、转写、停留点、配置这些块（方案见 报告/方案_20261021_录音与存储.md）：普通同步跳过，由 beayy-remote 单独处理 ----------

/// 不是分段、摘要、改名表的块：录音 audio.、转写 transcript.、停留点 places.、苹果健康 health.，以及 account 设备下的配置和声纹
pub fn is_special(device: &str, name: &str) -> bool {
    device == "account" || ["audio.", "transcript.", "places.", "health."].iter().any(|p| name.starts_with(p))
}

fn logged_in(root: &Path) -> Result<Account, String> {
    load(root).ok_or_else(|| "没登录账号".to_string())
}

/// 加密后传一块
pub fn put_named(root: &Path, device: &str, name: &str, plain: &[u8]) -> Result<(), String> {
    let a = logged_in(root)?;
    if !safe_name(device) || !safe_name(name) {
        return Err(format!("块名不合法：{device}/{name}"));
    }
    let blob = seal(&a, device, name, plain)?;
    if let Some(store) = a.remote() {
        return store.write(&blob_rel(device, name), &blob);
    }
    let r = agent()
        .put(&blob_url(&a.server, device, name))
        .header("Authorization", &a.auth())
        .header("Content-Type", "application/octet-stream")
        .send(&blob[..])
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    Ok(())
}

/// 取一块并解开；服务器上没有返回 None
pub fn get_named(root: &Path, device: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
    let a = logged_in(root)?;
    if let Some(store) = a.remote() {
        return match store.read(&blob_rel(device, name))? {
            Some(b) => open(&a, device, name, &b).map(Some),
            None => Ok(None),
        };
    }
    let mut r = agent().get(&blob_url(&a.server, device, name)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if r.status().as_u16() == 404 {
        return Ok(None);
    }
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    let blob = r.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
    open(&a, device, name, &blob).map(Some)
}

pub fn delete_named(root: &Path, device: &str, name: &str) -> Result<(), String> {
    let a = logged_in(root)?;
    if let Some(store) = a.remote() {
        return store.remove(&blob_rel(device, name));
    }
    let r = agent().delete(&blob_url(&a.server, device, name)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    Ok(())
}

/// 按块名前缀列（[{device, segment, size, updated}]，按 updated 升序），since 之后的；自动翻页
pub fn list_named(root: &Path, prefix: &str, since: i64) -> Result<Vec<Value>, String> {
    let a = logged_in(root)?;
    if let Some(store) = a.remote() {
        return Ok(store.items(prefix, false)?.into_iter().filter(|x| x["updated"].as_i64().unwrap_or(0) > since).collect());
    }
    let ag = agent();
    let (mut out, mut since) = (vec![], since);
    loop {
        let mut r = ag.get(&format!("{}/v1/list?since={since}&prefix={}", a.server, url_escape(prefix))).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(err_of(r));
        }
        let v: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
        let items = v["items"].as_array().cloned().unwrap_or_default();
        let full = items.len() >= 5000;
        since = items.last().and_then(|x| x["updated"].as_i64()).unwrap_or(since);
        out.extend(items);
        if !full {
            return Ok(out);
        }
    }
}

/// 服务器上这个账号存了什么：{items: [{device, kind, count, bytes, oldest, newest}], audio_max_days}
pub fn usage(root: &Path) -> Result<Value, String> {
    let a = logged_in(root)?;
    if let Some(store) = a.remote() {
        // 同服务器 /v1/usage：按设备和种类合计
        let mut m: BTreeMap<(String, String), (u64, u64, i64, i64)> = BTreeMap::new();
        for it in store.items("", false)? {
            let (d, n) = (it["device"].as_str().unwrap_or(""), it["segment"].as_str().unwrap_or(""));
            let kind = if d == "account" { "account" } else if n.starts_with(SUM) { "summary" } else if n.starts_with(DAILY) { "daily" } else if n == NAMES { "account" } else {
                ["audio", "transcript", "places", "health"].into_iter().find(|k| n.starts_with(&format!("{k}."))).unwrap_or("segment")
            };
            let u = it["updated"].as_i64().unwrap_or(0);
            let e = m.entry((d.to_string(), kind.to_string())).or_insert((0, 0, i64::MAX, 0));
            e.0 += 1;
            e.1 += it["size"].as_u64().unwrap_or(0);
            e.2 = e.2.min(u);
            e.3 = e.3.max(u);
        }
        let items: Vec<Value> = m.into_iter().map(|((d, k), (c, b, o, n))| json!({"device": d, "kind": k, "count": c, "bytes": b, "oldest": o, "newest": n})).collect();
        return Ok(json!({"items": items, "audio_max_days": 30, "now": chrono::Utc::now().timestamp_millis(), "folder": a.folder().map(|d| d.display().to_string()), "webdav": a.webdav()}));
    }
    let mut r = agent().get(&format!("{}/v1/usage", a.server)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    r.body_mut().read_json().map_err(|e| e.to_string())
}

/// 官方服务（模型、地图）用的：同步服务器地址和这台设备的认证头。没登录返回 None；令牌只在 Rust 里用，不给网页
pub fn official_auth(root: &Path) -> Option<(String, String)> {
    service(root).map(|a| (a.server.clone(), a.auth()))
}

/// 官方模型的代理地址和认证头（同步服务 /v2/llm/chat/completions，按账号开通）。没登录返回 None；令牌只在 Rust 里用，不给网页
pub fn official_llm(root: &Path) -> Option<(String, String)> {
    service(root).map(|a| (format!("{}/v2/llm/chat/completions", a.server), a.auth()))
}

/// 转写票据（1 小时有效）：(票据, 到期毫秒)
pub fn ticket(root: &Path) -> Result<(String, i64), String> {
    ticket_for(root, "asr")
}

/// 官方服务的票据（1 小时有效）：kind＝asr（转写）或 map（地图），同步服务按那一项的开通和额度发
pub fn ticket_for(root: &Path, kind: &str) -> Result<(String, i64), String> {
    let a = service(root).ok_or("没登录账号")?;
    let mut r = agent().get(&format!("{}/v2/ticket?for={kind}", a.server)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(err_of(r));
    }
    let v: Value = r.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok((v["ticket"].as_str().ok_or("服务器没给票据")?.to_string(), v["expires"].as_i64().unwrap_or(0)))
}

/// 取一块密文并解开
fn fetch(a: &Account, ag: &ureq::Agent, d: &str, n: &str) -> Result<Vec<u8>, String> {
    let mut resp = ag.get(&blob_url(&a.server, d, n)).header("Authorization", &a.auth()).call().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(err_of(resp));
    }
    let blob = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
    open(a, d, n, &blob)
}

/// 下载来的块写到和本机写的同样的位置：分段 → devices/<设备>/segments/<分段>/events.jsonl，摘要 → devices/<设备>/summaries/<文件名>
fn store_file(root: &Path, d: &str, n: &str, plain: Vec<u8>) -> Result<(), String> {
    let dev = root.join("devices").join(d);
    fs::create_dir_all(&dev).map_err(|e| e.to_string())?;
    let _ = fs::write(dev.join(SYNCED), "");
    if n == NAMES {
        return fs::write(dev.join(NAMES_FILE), plain).map_err(|e| e.to_string());
    }
    if n.starts_with(DAILY) {
        let file = daily_file(n).ok_or_else(|| format!("总结块名不对：{n}"))?;
        let dir = dev.join("daily");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        return fs::write(dir.join(file), plain).map_err(|e| e.to_string());
    }
    if let Some(file) = n.strip_prefix(SUM) {
        // 同一分段重写过的摘要只留最新的（文件名前 19 位是分段开始时刻）
        let dir = dev.join("summaries");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        if file.len() >= 19 {
            for old in fs::read_dir(&dir).map(|r| r.filter_map(|e| e.ok()).collect::<Vec<_>>()).unwrap_or_default() {
                let o = old.file_name().to_string_lossy().to_string();
                if o != file && o.get(..19) == file.get(..19) {
                    let _ = fs::remove_file(old.path());
                }
            }
        }
        return fs::write(dir.join(file), plain).map_err(|e| e.to_string());
    }
    let dir = dev.join("segments").join(n);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("events.jsonl"), plain).map_err(|e| e.to_string())
}

fn finish(root: &Path, mut st: State, uploaded: usize, downloaded: usize, errors: Vec<String>) -> Report {
    if let Some(a) = load(root) {
        st.raw.insert("user".into(), json!(a.user));
    }
    st.raw.insert("last".into(), json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)));
    st.raw.insert("last_error".into(), if errors.is_empty() { Value::Null } else { json!(errors.join("；")) });
    // 令牌被注销（别的设备用恢复码重设、或在设备列表里移除了这台）：要重新登录
    st.raw.insert("need_login".into(), json!(errors.iter().any(|e| e.contains("401"))));
    write_state(root, &st);
    Report { uploaded, downloaded, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// localhost 解析出 ::1 和 127.0.0.1：IPv4 要排在前面
    #[test]
    fn ipv4_first() {
        use ureq::unversioned::resolver::Resolver;
        let config = ureq::Agent::config_builder().build();
        let uri: ureq::http::Uri = "http://localhost:1/".parse().unwrap();
        let addrs = Ipv4First::default().resolve(&uri, &config, ureq::unversioned::transport::NextTimeout { after: ureq::unversioned::transport::time::Duration::NotHappening, reason: ureq::Timeout::Global }).unwrap();
        assert!(addrs[0].is_ipv4(), "{:?}", &addrs[..]);
        assert!(addrs.windows(2).all(|w| !(w[0].is_ipv6() && w[1].is_ipv4())), "{:?}", &addrs[..]);
    }

    /// 两台设备经同一个文件夹同步：A 新开、写一段，B 用主密码加入、下载到同样的位置；主密码错了加入不了
    #[test]
    fn folder_roundtrip() {
        let base = std::env::temp_dir().join(format!("beayy-folder-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (ra, rb, dir) = (base.join("a"), base.join("b"), base.join("cloud"));
        std::env::set_var("BEAYY_DATA_ROOT", &ra); // 账号存文件，不碰钥匙串
        let seg = ra.join("devices/mac-a/segments/2026-10-29T01-00-00Z");
        fs::create_dir_all(&seg).unwrap();
        fs::write(seg.join("events.jsonl"), "{\"x\":1}\n").unwrap();
        let code = folder_create(&ra, dir.to_str().unwrap(), "password-1").unwrap();
        assert!(folder_create(&ra, dir.to_str().unwrap(), "password-1").is_err());
        let r = sync_once(&ra, "mac-a").unwrap();
        assert_eq!((r.uploaded, r.errors.len()), (1, 0));
        assert!(folder_join(&rb, dir.to_str().unwrap(), "wrong-pass").is_err());
        folder_join(&rb, dir.to_str().unwrap(), "password-1").unwrap();
        let r = sync_once(&rb, "mac-b").unwrap();
        assert_eq!((r.downloaded, r.errors.len()), (1, 0));
        assert_eq!(fs::read_to_string(rb.join("devices/mac-a/segments/2026-10-29T01-00-00Z/events.jsonl")).unwrap(), "{\"x\":1}\n");
        // 再同步一轮：没有变的不重下
        assert_eq!(sync_once(&rb, "mac-b").unwrap().downloaded, 0);
        // 恢复码重设主密码后，新密码能加入
        folder_recover(&rb, dir.to_str().unwrap(), &code, "password-2").unwrap();
        assert!(folder_join(&ra, dir.to_str().unwrap(), "password-1").is_err());
        folder_join(&ra, dir.to_str().unwrap(), "password-2").unwrap();
        assert!(official_llm(&ra).is_none());
        let _ = fs::remove_dir_all(&base);
    }

    /// PROPFIND 的回复：各家服务器的命名空间前缀不一样（ns0:、D:、没有前缀），href 有的带域名、带 %xx
    #[test]
    fn dav_parse_any_prefix() {
        let a = r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:"><D:response><D:href>/dav/Beayy/blobs/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
<D:response><D:href>https://dav.jianguoyun.com/dav/Beayy/blobs/mac-a/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype><D:getlastmodified>Wed, 04 Nov 2026 09:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat><D:propstat><D:prop><D:getcontentlength/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat></D:response></D:multistatus>"#;
        let e = dav_parse(a);
        assert_eq!(e.len(), 2);
        assert_eq!((e[1].name.as_str(), e[1].dir, e[1].updated), ("mac-a", true, 1793782800000));
        let b = r#"<multistatus xmlns="DAV:"><response><href>/r/%E5%90%8C%E6%AD%A5/blobs/mac-a/2026-10-29T01-00-00Z</href><propstat><prop><resourcetype/><getcontentlength>123</getcontentlength><getlastmodified>Thu, 29 Oct 2026 01:02:03 GMT</getlastmodified><getetag>&quot;abc&quot;</getetag></prop></propstat></response></multistatus>"#;
        let e = dav_parse(b);
        assert_eq!((e[0].name.as_str(), e[0].dir, e[0].size, e[0].etag.as_deref()), ("2026-10-29T01-00-00Z", false, 123, Some("\"abc\"")));
        assert_eq!(pct_decode("/r/%E5%90%8C%E6%AD%A5/"), "/r/同步/");
        assert_eq!(dav_base("https://dav.jianguoyun.com/dav/我的 同步/").unwrap(), "https://dav.jianguoyun.com/dav/%E6%88%91%E7%9A%84%20%E5%90%8C%E6%AD%A5");
        assert!(dav_base("dav.jianguoyun.com/dav").is_err());
    }

    /// iCloud 就是放在 iCloud 容器里的同步文件夹：BEAYY_ICLOUD_DIR 指一个临时目录当容器
    #[test]
    fn icloud_folder() {
        let base = std::env::temp_dir().join(format!("beayy-icloud-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (ra, rb) = (base.join("a"), base.join("b"));
        std::env::set_var("BEAYY_DATA_ROOT", &ra);
        std::env::set_var("BEAYY_ICLOUD_DIR", base.join("container"));
        assert_eq!(icloud_state()["vault"], json!(false));
        icloud_create(&ra, "password-1").unwrap();
        assert!(icloud_create(&ra, "password-1").is_err());
        assert_eq!(icloud_state()["vault"], json!(true));
        icloud_join(&rb, "password-1").unwrap();
        let st = status(&rb);
        assert_eq!((st["mode"].as_str(), st["server"].is_null()), (Some("icloud"), true));
        assert!(st["folder"].as_str().unwrap().ends_with("Sync"));
        logout(&rb);
        assert!(load(&rb).is_none());
        assert!(status(&rb)["mode"].is_null());
        let _ = fs::remove_dir_all(&base);
    }

    /// 两台设备经同一个 WebDAV 同步（同 folder_roundtrip），要一个真的 WebDAV，默认不跑：
    ///   python3 -m venv v && v/bin/pip install wsgidav cheroot
    ///   v/bin/wsgidav --host 127.0.0.1 --port 18765 --root <空目录> 再配一个 Basic 认证的用户（--config，simple_dc.user_mapping）
    ///   BEAYY_TEST_WEBDAV=http://127.0.0.1:18765 BEAYY_TEST_WEBDAV_USER=tester BEAYY_TEST_WEBDAV_PASS=secret-1 \
    ///     cargo test -p beayy-sync webdav_roundtrip -- --ignored --nocapture
    /// 每次在地址下面新建 beayy-test-<进程号>/，跑完删掉
    #[test]
    #[ignore]
    fn webdav_roundtrip() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("要设 {k}"));
        let (url, user, pass) = (format!("{}/beayy-test-{}", env("BEAYY_TEST_WEBDAV").trim_end_matches('/'), std::process::id()), env("BEAYY_TEST_WEBDAV_USER"), env("BEAYY_TEST_WEBDAV_PASS"));
        let base = std::env::temp_dir().join(format!("beayy-webdav-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (ra, rb) = (base.join("a"), base.join("b"));
        std::env::set_var("BEAYY_DATA_ROOT", &ra); // 账号存文件，不碰钥匙串
        let seg = ra.join("devices/mac-a/segments/2026-10-29T01-00-00Z");
        fs::create_dir_all(&seg).unwrap();
        fs::write(seg.join("events.jsonl"), "{\"x\":1}\n").unwrap();
        assert_eq!(webdav_state(&url, &user, &pass), json!({"ok": true, "vault": false}));
        assert_eq!(webdav_state(&url, &user, "wrong")["ok"], json!(false));
        let code = webdav_create(&ra, &url, &user, &pass, "password-1").unwrap();
        assert!(webdav_create(&ra, &url, &user, &pass, "password-1").is_err());
        assert_eq!(webdav_state(&url, &user, &pass), json!({"ok": true, "vault": true}));
        let r = sync_once(&ra, "mac-a").unwrap();
        assert_eq!((r.uploaded, r.errors.len()), (1, 0), "{:?}", r.errors);
        assert!(webdav_join(&rb, &url, &user, &pass, "wrong-pass").is_err());
        webdav_join(&rb, &url, &user, &pass, "password-1").unwrap();
        let st = status(&rb);
        assert_eq!((st["mode"].as_str(), st["webdav"].as_str(), st["server"].is_null()), (Some("webdav"), Some(url.as_str()), true));
        let r = sync_once(&rb, "mac-b").unwrap();
        assert_eq!((r.downloaded, r.errors.len()), (1, 0), "{:?}", r.errors);
        let got = rb.join("devices/mac-a/segments/2026-10-29T01-00-00Z/events.jsonl");
        assert_eq!(fs::read_to_string(&got).unwrap(), "{\"x\":1}\n");
        // 没变的不重下；变了的重传、重下
        assert_eq!(sync_once(&rb, "mac-b").unwrap().downloaded, 0);
        fs::write(seg.join("events.jsonl"), "{\"x\":1}\n{\"x\":2}\n").unwrap();
        assert_eq!(sync_once(&ra, "mac-a").unwrap().uploaded, 1);
        assert_eq!(sync_once(&rb, "mac-b").unwrap().downloaded, 1);
        assert_eq!(fs::read_to_string(&got).unwrap().lines().count(), 2);
        // 手机传的停留点这类单独的块：传、列、取、删
        put_named(&rb, "iphone-test", "places.2026-10-29", b"{\"id\":\"a\"}\n").unwrap();
        assert_eq!(get_named(&ra, "iphone-test", "places.2026-10-29").unwrap().as_deref(), Some(&b"{\"id\":\"a\"}\n"[..]));
        let a = load(&ra).unwrap();
        let listed = a.remote().unwrap().items("places.", true).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0]["updated"].as_i64().unwrap() > 0 && listed[0]["size"].as_u64().unwrap() > 0);
        delete_named(&ra, "iphone-test", "places.2026-10-29").unwrap();
        assert_eq!(get_named(&ra, "iphone-test", "places.2026-10-29").unwrap(), None);
        assert!(remote_devices(&a).unwrap().iter().any(|d| d["device"] == "mac-a"));
        assert!(official_llm(&ra).is_none() && ticket(&ra).is_err());
        // 恢复码重设主密码后，新密码能加入，旧的不行
        webdav_recover(&rb, &url, &user, &pass, &code, "password-2").unwrap();
        assert!(webdav_join(&ra, &url, &user, &pass, "password-1").is_err());
        webdav_join(&ra, &url, &user, &pass, "password-2").unwrap();
        logout(&rb);
        assert!(load(&rb).is_none());
        let d = Dav::new(&url, &user, &pass).unwrap();
        let _ = d.delete("");
        let _ = fs::remove_dir_all(&base);
    }

    /// 同步用文件夹（iCloud、网盘同理）时单为官方服务登账号；要一台开放注册的服务端：
    ///   cd app/server && BEAYY_OPEN_SIGNUP=1 BEAYY_NEW_USER_LLM_TOKENS=50000 uvicorn server:app --port 8770（在空目录里跑，数据写进当前目录）
    ///   BEAYY_TEST_SERVER=http://127.0.0.1:8770 cargo test -p beayy-sync service_roundtrip -- --ignored --nocapture
    #[test]
    #[ignore]
    fn service_roundtrip() {
        let server = std::env::var("BEAYY_TEST_SERVER").expect("要设 BEAYY_TEST_SERVER");
        let base = std::env::temp_dir().join(format!("beayy-service-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("a");
        std::env::set_var("BEAYY_DATA_ROOT", &root); // 账号存文件，不碰钥匙串
        let email = format!("svc-{}@example.com", std::process::id());
        folder_create(&root, base.join("vault").to_str().unwrap(), "password-1").unwrap();
        assert!(official_llm(&root).is_none() && ticket(&root).is_err() && service_email(&root).is_none());
        // 只为官方服务注册：同步还是文件夹，官方模型、票据、头像走这个账号
        service_signup(&root, &server, "", &email, "password-1", "mac-a", "Mac · a").unwrap();
        assert_eq!(load(&root).unwrap().mode(), "folder");
        assert_eq!(service_email(&root).as_deref(), Some(email.as_str()));
        assert_eq!(status(&root)["service_email"], json!(email));
        assert!(official_llm(&root).unwrap().0.starts_with(&server));
        let acc = account(&root).unwrap();
        assert_eq!((acc["mode"].as_str(), acc["services"]["plan"].as_str()), (Some("folder"), Some("official")), "{acc}");
        assert!(set_profile(&root, "测试", None).unwrap() > 0);
        // 退出文件夹同步：官方服务照常
        logout(&root);
        assert!(load(&root).is_none() && official_llm(&root).is_some());
        assert_eq!(service_account(&root).unwrap()["services"]["plan"], json!("official"));
        service_logout(&root);
        assert!(official_llm(&root).is_none());
        // 换成服务器同步：登录后 service 那个注销；停掉服务器同步时登录挪回 service，令牌照旧能用
        service_login(&root, &server, &email, "password-1", "mac-a", "Mac · a").unwrap();
        login(&root, &server, &email, "password-1", "mac-a", "Mac · a").unwrap();
        assert!(service_email(&root).is_none() && server_mode(&root));
        keep_for_service(&root).unwrap();
        assert!(load(&root).is_none() && !server_mode(&root));
        assert_eq!(service_account(&root).unwrap()["services"]["plan"], json!("official"));
        // 新账号没开转写：服务器回「没开通」，不是本机说没登录
        let e = ticket_for(&root, "asr").unwrap_err();
        assert!(!e.contains("没登录"), "{e}");
        service_logout(&root);
        assert!(service_account(&root).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn daily_names() {
        assert_eq!(daily_block("日总结_20261028.md").as_deref(), Some("daily.d.20261028.md"));
        assert_eq!(daily_block("月总结_202610.md").as_deref(), Some("daily.m.202610.md"));
        assert_eq!(daily_block("state.json"), None);
        assert_eq!(daily_block("stats_2026-10-28.json"), None);
        assert_eq!(daily_file("daily.w.20261026.md").as_deref(), Some("周总结_20261026.md"));
        assert_eq!(daily_file("daily.x.2026.md"), None);
        assert!(safe_name(&daily_block("周总结_20261026.md").unwrap()));
    }

    #[test]
    fn unicode_device_names() {
        assert!(safe_name("win-小栗子"));
        assert!(!safe_name("..") && !safe_name("a/b") && !safe_name("a b") && !safe_name(""));
        assert_eq!(blob_url("https://s", "win-小栗子", "daily.d.20261105.md"), "https://s/v1/blob/win-%E5%B0%8F%E6%A0%97%E5%AD%90/daily.d.20261105.md");
    }

    #[test]
    fn lean_drops_screen_text() {
        let seg = b"{\"event_type\":\"ui.text\",\"app_name\":\"A\",\"text\":\"secret\"}\n{\"event_type\":\"mouse.click\",\"text\":\"ok\"}\nnot json\n";
        let out = String::from_utf8(lean_segment(seg)).unwrap();
        assert!(!out.contains("secret") && out.contains("\"app_name\":\"A\""), "{out}");
        assert!(out.contains("\"text\":\"ok\"") && out.ends_with("not json\n"), "{out}");
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn seal_open_roundtrip() {
        let a = Account { server: "x".into(), user: "u".into(), email: None, token: "t".into(), key: [7; 32] };
        let blob = seal(&a, "mac-a", "2026-10-16T01-00-00Z", b"hello\n").unwrap();
        assert_eq!(open(&a, "mac-a", "2026-10-16T01-00-00Z", &blob).unwrap(), b"hello\n");
        // 挪到别的分段解不开
        assert!(open(&a, "mac-a", "2026-10-16T01-10-00Z", &blob).is_err());
        // 配对码来回
        let b = Account::from_code(&a.code()).unwrap();
        assert_eq!((b.user, b.token, b.key), (a.user, a.token, a.key));
        // 恢复码来回
        let key = [0xA5u8; 32];
        assert_eq!(parse_recovery(&recovery_code_of(&key).to_lowercase()).unwrap(), key);
        // 包一层再解开；包钥不对解不开
        let w = wrap_key(&key, &[1; 32]).unwrap();
        assert_eq!(unwrap_key(&w, &[1; 32]).unwrap(), key);
        assert!(unwrap_key(&w, &[2; 32]).is_err());
    }

    /// 给别的端（iPhone 的 Swift）对照的测试向量：cargo test -p beayy-sync vectors -- --nocapture
    #[test]
    fn vectors() {
        let (auth, wrap) = derive("correct horse battery", "00112233445566778899aabbccddeeff").unwrap();
        let key = [0x42u8; 32];
        let nonce = [9u8; 24];
        let wrapped = {
            let ct = XChaCha20Poly1305::new((&wrap).into()).encrypt(XNonce::from_slice(&nonce), Payload { msg: &key[..], aad: b"beayy-data-key" }).unwrap();
            B64.encode([nonce.to_vec(), ct].concat())
        };
        assert_eq!(unwrap_key(&wrapped, &wrap).unwrap(), key);
        let a = Account { server: "x".into(), user: "u".into(), email: None, token: "t".into(), key };
        let plain = b"{\"id\":\"watch-20261021T010000Z\"}\nhello";
        let packed = miniz_oxide::deflate::compress_to_vec(plain, 6);
        let ct = a.cipher().encrypt(XNonce::from_slice(&nonce), Payload { msg: &packed, aad: b"watch-1a2b3c4d/audio.watch-20261021T010000Z" }).unwrap();
        let blob = [nonce.to_vec(), ct].concat();
        assert_eq!(open(&a, "watch-1a2b3c4d", "audio.watch-20261021T010000Z", &blob).unwrap(), plain);
        println!("password=correct horse battery salt=00112233445566778899aabbccddeeff");
        println!("auth={auth}");
        println!("wrap={}", hex_encode(&wrap));
        println!("data_key={}", hex_encode(&key));
        println!("wrapped_key(b64url nopad, nonce 0909..)={wrapped}");
        println!("blob device=watch-1a2b3c4d name=audio.watch-20261021T010000Z hex={}", hex_encode(&blob));
        println!("blob plaintext={}", String::from_utf8_lossy(plain));
    }
}
