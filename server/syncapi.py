"""Beayy 同步服务：只存设备上加密好的数据块，服务器读不到内容。
从 Cloudflare 版（app/sync-server/worker.js）移植，接口、返回格式相同，客户端两边都能连：
D1 → SQLite（data/sync/beayy.db，表结构 schema.sql），R2 → 本地文件（data/sync/blobs/<账号>/<设备>/<块名>），
自动更新的安装包放 data/sync/update/（自己部署的一般用不上，客户端从官方检查更新）。

v2 账号：邮箱＋主密码。主密码只在设备上用，派生出两把：登录校验值（服务器只存它的哈希）和包数据密钥的钥匙
（服务器只存包好的数据密钥，解不开）。每台设备登录后拿自己的令牌，可单独移除。另有恢复码，忘了主密码时重设。
"""
import base64
import hashlib
import hmac
import json
import os
import re
import secrets
import shutil
import sqlite3
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

from fastapi import APIRouter, Request
from fastapi.responses import FileResponse, JSONResponse, Response

SYNC = Path(os.environ.get("BEAYY_SYNC_DIR", "data/sync"))
DB = SYNC / "beayy.db"
BLOBS = SYNC / "blobs"
UPDATE = SYNC / "update"
BACKUP = Path(os.environ.get("BEAYY_BACKUP_DIR", "backup"))
BACKUP_KEEP = 14  # 每天一份库的备份，留两周（数据块本身是各设备本地数据的加密副本，不另备份）
HERE = Path(__file__).resolve().parent
INVITE = os.environ.get("BEAYY_INVITE", "")
# 开放注册：不要邀请码也能注册（填了 BEAYY_INVITE 也照收邀请码）。开放时靠下面三道闸护住硬盘
OPEN_SIGNUP = os.environ.get("BEAYY_OPEN_SIGNUP", "") in ("1", "true", "yes")
GB = 1024 ** 3
MB = 1000 ** 2  # 和客户端显示一致，按十进制
# 每个账号最多存多少（MB，0＝不限）；单个账号可以用 admin.py storage 另设
USER_QUOTA = int(float(os.environ.get("BEAYY_USER_QUOTA_MB", "0")) * MB)
# 最多开放多少个账号（0＝不限），满了停注册
MAX_USERS = int(os.environ.get("BEAYY_MAX_USERS", "0"))
# 新注册的账号自动开通官方模型，每月这么多 token（0＝不自动开通，要 admin.py open）；转写、地图不跟着开
NEW_USER_LLM = int(os.environ.get("BEAYY_NEW_USER_LLM_TOKENS", "0") or 0)
# 新注册的账号官方服务试用几天（0＝不限期）；到期后模型、转写、地图都不能用，要在设置里改用自己的
NEW_USER_TRIAL_DAYS = float(os.environ.get("BEAYY_NEW_USER_TRIAL_DAYS", "0") or 0)
# 硬盘剩余低于这么多（GB）就停止注册；再低到一半，连上传也停（不把同一块盘上的别的服务挤爆）
MIN_FREE = int(float(os.environ.get("BEAYY_MIN_FREE_GB", "20")) * GB)
# 同一个 IP 每天最多注册几个、整台服务每天最多注册几个
SIGNUP_PER_IP = int(os.environ.get("BEAYY_SIGNUP_PER_IP", "3"))
SIGNUP_PER_DAY = int(os.environ.get("BEAYY_SIGNUP_PER_DAY", "50"))

def _ticket_secret() -> str:
    """转写、地图票据的签名密钥：没配就生成一个存在同步目录里，重启不变（签票据和验票据都在 server.py 这个进程）"""
    if os.environ.get("BEAYY_TICKET_SECRET"):
        return os.environ["BEAYY_TICKET_SECRET"]
    p = SYNC / "ticket_secret"
    SYNC.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(secrets.token_hex(32))
    except FileExistsError:
        pass
    return p.read_text().strip()


TICKET_SECRET = _ticket_secret()
LLM_GATEWAY = os.environ.get("BEAYY_LLM_GATEWAY", "")
LLM_KEY = os.environ.get("BEAYY_LLM_KEY", "")
LLM_MODEL = os.environ.get("BEAYY_LLM_MODEL", "")
AMAP_JS_KEY = os.environ.get("BEAYY_AMAP_JS_KEY", "")
AMAP_JS_CODE = os.environ.get("BEAYY_AMAP_JS_CODE", "")
# 这台服务对外的地址（能缩放的地图要从这里中转安全密钥）；不填就按请求的地址算（反向代理要带 X-Forwarded-Proto）
OFFICIAL_HOST = os.environ.get("BEAYY_OFFICIAL_HOST", "").rstrip("/")

NAME = re.compile(r"^[A-Za-z0-9._-]{1,80}$")
PREFIX = re.compile(r"^[A-Za-z0-9._-]{1,40}$")
USER = re.compile(r"^[0-9a-f]{24}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
SALT = re.compile(r"^[0-9a-f]{32}$")
EMAIL = re.compile(r"^[^\s@]{1,64}@[^\s@]{1,190}$")
BEARER = re.compile(r"^Bearer ([0-9a-f]{24}):([0-9a-f]{64})$")
MAX_BLOB = 50 * 1024 * 1024
# 设备上主密码派生用的参数（Argon2id）；服务器只是原样存取
KDF = {"alg": "argon2id", "m": 65536, "t": 3, "p": 1}
# 录音块（audio.<段 id>）的兜底期限：电脑转写完 7 天后自己删；电脑关机太久没转的，到这个天数服务器删
AUDIO_MAX_DAYS = 30
SERVICE_NAMES = {"llm": "模型", "asr": "转写", "map": "地图"}
QUOTA_COL = {"llm": "llm_quota", "asr": "asr_quota", "map": "map_quota"}
LLM_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

router = APIRouter()


# ---------- 存储 ----------

def db() -> sqlite3.Connection:
    """每次请求开一个连接（接口在线程池里跑）；WAL 让读写互不挡"""
    c = sqlite3.connect(DB, timeout=30, isolation_level=None)
    c.row_factory = sqlite3.Row
    c.execute("PRAGMA busy_timeout = 30000")
    return c


def init():
    """建库：按 schema.sql 建表（都是 IF NOT EXISTS，已有的库不动）"""
    SYNC.mkdir(parents=True, exist_ok=True)
    c = db()
    c.execute("PRAGMA journal_mode = WAL")
    c.executescript((HERE / "schema.sql").read_text())
    # 老库补后加的列（CREATE TABLE IF NOT EXISTS 不会给已有的表加列）
    if "llm_until" not in {r["name"] for r in c.execute("PRAGMA table_info(users)")}:
        c.execute("ALTER TABLE users ADD COLUMN llm_until INTEGER NOT NULL DEFAULT 0")
    c.close()


def now_ms() -> int:
    return int(time.time() * 1000)


def blob_path(user: str, device: str, segment: str) -> Path:
    return BLOBS / user / device / segment


def write_file(p: Path, data: bytes):
    """先写同目录临时文件再改名，半截不会留下坏文件"""
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=p.parent, prefix=".up-")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
        os.replace(tmp, p)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def cleanup_audio():
    """删超期的录音块（原来是 Worker 每天的定时任务）"""
    cutoff = now_ms() - AUDIO_MAX_DAYS * 86400 * 1000
    c = db()
    rows = c.execute("SELECT user, device, segment FROM blobs WHERE segment >= 'audio.' AND segment < 'audio/' AND updated < ?", (cutoff,)).fetchall()
    for r in rows:
        blob_path(r["user"], r["device"], r["segment"]).unlink(missing_ok=True)
        c.execute("DELETE FROM blobs WHERE user = ? AND device = ? AND segment = ?", (r["user"], r["device"], r["segment"]))
    c.close()


def backup_db():
    """库备份成 backup/beayy-<日期>.db（SQLite 在线备份，不停服务），只留最近 BACKUP_KEEP 份"""
    BACKUP.mkdir(parents=True, exist_ok=True)
    dst = sqlite3.connect(BACKUP / f"beayy-{time.strftime('%Y%m%d')}.db")
    src = db()
    src.backup(dst)
    src.close()
    dst.close()
    for old in sorted(BACKUP.glob("beayy-*.db"))[:-BACKUP_KEEP]:
        old.unlink()


def _daily_loop():
    while True:
        for job in (cleanup_audio, backup_db):
            try:
                job()
            except Exception as e:  # 定时任务出错不影响接口
                print(f"{job.__name__} 失败：{e!r}", flush=True)
        time.sleep(86400)


def startup():
    """server.py 启动时调：建库、起每天清理录音块和备份库的线程"""
    init()
    threading.Thread(target=_daily_loop, daemon=True).start()


# ---------- 小工具 ----------

def json_resp(obj, status: int = 200) -> JSONResponse:
    return JSONResponse(obj, status_code=status)


def client_ip(request: Request) -> str:
    """真实来源 IP：经 Cloudflare 隧道的看 CF-Connecting-IP，经反向代理的看 X-Forwarded-For 第一个"""
    h = request.headers
    ip = h.get("cf-connecting-ip") or (h.get("x-forwarded-for") or "").split(",")[0].strip()
    return ip or (request.client.host if request.client else "")


def disk_free() -> int:
    return shutil.disk_usage(SYNC).free


def quota_of(c: sqlite3.Connection, user: str) -> int:
    """这个账号最多存多少字节（0＝不限）：admin.py 单独设过的为准，没设用 BEAYY_USER_QUOTA_MB"""
    r = c.execute("SELECT storage FROM quotas WHERE user = ?", (user,)).fetchone()
    return USER_QUOTA if r is None else max(0, r["storage"])


def users_count() -> int:
    c = db()
    try:
        return c.execute("SELECT COUNT(*) FROM users").fetchone()[0]
    finally:
        c.close()


def stored_of(c: sqlite3.Connection, user: str) -> int:
    return c.execute("SELECT COALESCE(SUM(size), 0) FROM blobs WHERE user = ?", (user,)).fetchone()[0]


# 注册次数：{日期: {ip: 次数}}，进程内记（重启清零，够挡住刷号）
_SIGNUPS: dict = {}


def signup_gate(request: Request, invited: bool) -> str | None:
    """能不能注册：返回拦下的原因，能注册返回 None"""
    if not (OPEN_SIGNUP or invited):
        return "这台服务器要邀请码才能注册" if INVITE else "这台服务器没开放注册"
    if disk_free() < MIN_FREE:
        return "服务器存储快满了，暂停注册。可以先不登录（记录都在本机），或者自己部署服务端"
    if MAX_USERS and users_count() >= MAX_USERS:
        return "注册名额满了。可以先不登录（记录都在本机），或者在「设置 → 连接 → 同步方式」里选同步文件夹"
    day = time.strftime("%Y-%m-%d")
    for k in [k for k in _SIGNUPS if k != day]:
        del _SIGNUPS[k]
    m = _SIGNUPS.setdefault(day, {})
    if sum(m.values()) >= SIGNUP_PER_DAY:
        return "今天注册的人太多了，明天再试"
    if m.get(client_ip(request), 0) >= SIGNUP_PER_IP:
        return "这个网络今天注册得太多了，明天再试"
    return None


def count_signup(request: Request):
    m = _SIGNUPS.setdefault(time.strftime("%Y-%m-%d"), {})
    ip = client_ip(request)
    m[ip] = m.get(ip, 0) + 1


def sha256(s: str) -> str:
    return hashlib.sha256(s.encode()).hexdigest()


def same(a: str, b: str) -> bool:
    return hmac.compare_digest(sha256(a), sha256(b))


def b64url(b: bytes) -> str:
    return base64.urlsafe_b64encode(b).decode().rstrip("=")


def sign(msg: str) -> str:
    return b64url(hmac.new(TICKET_SECRET.encode(), msg.encode(), hashlib.sha256).digest())


def month() -> str:
    """这个月（UTC），额度按它清零"""
    return time.strftime("%Y-%m", time.gmtime())


async def body_json(request: Request) -> dict:
    try:
        b = await request.json()
        return b if isinstance(b, dict) else {}
    except ValueError:
        return {}


def kind_of(device: str, segment: str) -> str:
    """块的种类：按块名前缀（分段名以年份开头）"""
    if device == "account":
        return "account"
    for k in ("audio", "transcript", "places", "health", "sum", "daily"):
        if segment.startswith(k + "."):
            return "summary" if k == "sum" else k
    return "account" if segment == "names.json" else "segment"


def auth(request: Request) -> dict | None:
    """Authorization: Bearer <user>:<token> → {user, session}。令牌可以是设备登录的（sessions），也可以是 v1 共用令牌"""
    m = BEARER.match(request.headers.get("authorization") or "")
    if not m:
        return None
    user, h = m.group(1), sha256(m.group(2))
    c = db()
    try:
        s = c.execute("SELECT id, last_seen FROM sessions WHERE user = ? AND token_hash = ?", (user, h)).fetchone()
        if s:
            if now_ms() - s["last_seen"] > 3600 * 1000:
                c.execute("UPDATE sessions SET last_seen = ? WHERE id = ?", (now_ms(), s["id"]))
            return {"user": user, "session": s["id"]}
        row = c.execute("SELECT token_hash FROM users WHERE user = ?", (user,)).fetchone()
        return {"user": user, "session": None} if row and same(h, row["token_hash"]) else None
    finally:
        c.close()


def log_upload(c: sqlite3.Connection, user: str, device: str, size: int, ok: bool):
    """上传记一笔（按小时合计），看每台设备什么时候在传、有没有被拒"""
    c.execute("INSERT INTO uploads (user, device, hour, n, bytes, rejected) VALUES (?, ?, ?, ?, ?, ?) "
              "ON CONFLICT (user, device, hour) DO UPDATE SET n = n + excluded.n, bytes = bytes + excluded.bytes, rejected = rejected + excluded.rejected",
              (user, device, int(time.time()) // 3600, int(ok), size if ok else 0, int(not ok)))


def unauthorized() -> JSONResponse:
    return json_resp({"error": "没登录或令牌不对"}, 401)


# ---------- 官方服务：模型（token）、转写（音频秒）、地图（次），按账号开通，每项每月一个上限 ----------

def usage_of(c: sqlite3.Connection, user: str) -> dict:
    return {r["service"]: r["used"] for r in c.execute("SELECT service, used FROM usage WHERE user = ? AND month = ?", (user, month()))}


def add_usage(user: str, service: str, n: float):
    """记用量。转写进程（worker.py）也直接调它"""
    if not n or n <= 0 or service not in SERVICE_NAMES:
        return
    c = db()
    c.execute("INSERT INTO usage (user, month, service, used) VALUES (?, ?, ?, ?) "
              "ON CONFLICT (user, month, service) DO UPDATE SET used = used + excluded.used", (user, month(), service, round(n)))
    c.execute("INSERT INTO usage_daily (user, day, service, used) VALUES (?, ?, ?, ?) "
              "ON CONFLICT (user, day, service) DO UPDATE SET used = used + excluded.used", (user, time.strftime("%Y-%m-%d", time.gmtime()), service, round(n)))
    c.close()


def quota(user: str, service: str) -> dict:
    """这一项能不能用：{plan, used, quota} 或 {error, status}"""
    c = db()
    try:
        u = c.execute("SELECT llm_plan, llm_quota, asr_quota, map_quota, llm_until FROM users WHERE user = ?", (user,)).fetchone()
        if not u or not u["llm_plan"]:
            return {"error": "这个账号还没开通官方服务：联系管理员开通，或在设置里改用自己的", "status": 403}
        if u["llm_plan"] == "stopped":
            return {"error": "这个账号的官方服务已停用", "status": 403}
        if u["llm_until"] and now_ms() > u["llm_until"]:
            return {"error": "官方服务试用已结束：在设置 → AI → 模型里改用自己的", "status": 403}
        used = usage_of(c, user).get(service, 0)
        q = u[QUOTA_COL[service]] or 0
        if q < 0:
            return {"error": f"这个账号没开通官方{SERVICE_NAMES[service]}：联系管理员开通，或在设置里改用自己的", "status": 403}
        if q > 0 and used >= q:
            return {"error": f"本月官方{SERVICE_NAMES[service]}额度用完了，下个月 1 号恢复", "status": 429}
        return {"plan": u["llm_plan"], "used": used, "quota": q}
    finally:
        c.close()


def services(c: sqlite3.Connection, user: str) -> dict:
    """设置页显示：{plan, month, llm: {used, quota}, asr: {…}, map: {…}}"""
    u = c.execute("SELECT llm_plan, llm_quota, asr_quota, map_quota, llm_until FROM users WHERE user = ?", (user,)).fetchone()
    used = usage_of(c, user)
    one = lambda k: {"used": used.get(k, 0), "quota": (u[QUOTA_COL[k]] if u else 0) or 0}
    # until：试用到哪一刻（毫秒），不限期的没有这个键
    until = {"until": u["llm_until"]} if u and u["llm_until"] else {}
    return {"plan": (u["llm_plan"] if u else None) or None, "month": month(), **until, "llm": one("llm"), "asr": one("asr"), "map": one("map")}


def ticket(user: str, kind: str) -> tuple[str, int]:
    exp = int(time.time()) + 3600
    payload = b64url(json.dumps({"u": user, "exp": exp, "k": "map"} if kind == "map" else {"u": user, "exp": exp}, separators=(",", ":")).encode())
    return f"{payload}.{sign(payload)}", exp


# ---------- 公开：自动更新、隐私政策 ----------

@router.api_route("/update/{name}", methods=["GET", "HEAD"])
def update_file(name: str):
    """自动更新：公开只读，latest.json 和安装包（放这里不放 GitHub，国内不翻墙也能下）"""
    p = UPDATE / name
    if not NAME.match(name) or not p.is_file():
        return json_resp({"error": "没有这个文件"}, 404)
    return FileResponse(p, media_type="application/json; charset=utf-8" if name.endswith(".json") else "application/octet-stream",
                        headers={"cache-control": "no-cache" if name == "latest.json" else "public, max-age=86400"})


@router.get("/privacy")
def privacy():
    """隐私政策（官方实例给 App Store 填的链接）"""
    return Response((HERE / "privacy.html").read_text(), media_type="text/html; charset=utf-8", headers={"cache-control": "public, max-age=3600"})


# ---------- v1：数据块 ----------

@router.post("/v1/register")
def register_v1(request: Request):
    # 第一版要邀请码，防止被陌生人注册
    if not INVITE or not same(request.headers.get("x-invite") or "", INVITE):
        return json_resp({"error": "邀请码不对"}, 403)
    user, token = secrets.token_hex(12), secrets.token_hex(32)
    c = db()
    c.execute("INSERT INTO users (user, token_hash, created) VALUES (?, ?, ?)", (user, sha256(token), now_ms()))
    c.close()
    return {"user": user, "token": token}


@router.put("/v1/blob/{device}/{segment}")
async def put_blob(device: str, segment: str, request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    if not NAME.match(device) or not NAME.match(segment):
        return json_resp({"error": "设备名或分段名不合法"}, 400)
    data = await request.body()
    if len(data) > MAX_BLOB:
        return json_resp({"error": "太大了"}, 413)
    c = db()
    if disk_free() < MIN_FREE // 2:
        log_upload(c, who["user"], device, len(data), False)
        c.close()
        return json_resp({"error": "服务器存储满了，暂停上传，记录都还在本机"}, 507)
    cap = quota_of(c, who["user"])
    if cap:
        old = c.execute("SELECT size FROM blobs WHERE user = ? AND device = ? AND segment = ?", (who["user"], device, segment)).fetchone()
        if stored_of(c, who["user"]) - (old["size"] if old else 0) + len(data) > cap:
            log_upload(c, who["user"], device, len(data), False)
            c.close()
            return json_resp({"error": f"这个账号的同步空间满了（{round(cap / MB):g} MB），记录都还在本机。可以换成同步文件夹或自己部署服务端"}, 507)
    write_file(blob_path(who["user"], device, segment), data)
    now = now_ms()
    c.execute("INSERT INTO blobs (user, device, segment, size, updated) VALUES (?, ?, ?, ?, ?) "
              "ON CONFLICT (user, device, segment) DO UPDATE SET size = excluded.size, updated = excluded.updated",
              (who["user"], device, segment, len(data), now))
    log_upload(c, who["user"], device, len(data), True)
    c.close()
    return {"updated": now}


@router.get("/v1/blob/{device}/{segment}")
def get_blob(device: str, segment: str, request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    if not NAME.match(device) or not NAME.match(segment):
        return json_resp({"error": "设备名或分段名不合法"}, 400)
    p = blob_path(who["user"], device, segment)
    if not p.is_file():
        return json_resp({"error": "没有这个块"}, 404)
    return FileResponse(p, media_type="application/octet-stream")


@router.delete("/v1/blob/{device}/{segment}")
def delete_blob(device: str, segment: str, request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    if not NAME.match(device) or not NAME.match(segment):
        return json_resp({"error": "设备名或分段名不合法"}, 400)
    blob_path(who["user"], device, segment).unlink(missing_ok=True)
    c = db()
    c.execute("DELETE FROM blobs WHERE user = ? AND device = ? AND segment = ?", (who["user"], device, segment))
    c.close()
    return {"ok": True}


@router.get("/v1/list")
def list_blobs(request: Request, since: str = "0", prefix: str = "", device: str = ""):
    who = auth(request)
    if not who:
        return unauthorized()
    try:
        since_n = int(float(since or 0))
    except ValueError:
        since_n = 0
    # 可按块名前缀（transcript.、audio.、places.）和设备筛
    if (prefix and not PREFIX.match(prefix)) or (device and not NAME.match(device)):
        return json_resp({"error": "筛选条件不合法"}, 400)
    # 从头按前缀列走 (user, segment)，只读这一类；增量列走 (user, updated)，只读新来的
    index = "blobs_seg" if prefix and not since_n else "blobs_updated"
    sql = f"SELECT device, segment, size, updated FROM blobs INDEXED BY {index} WHERE user = ? AND updated > ?"
    args: list = [who["user"], since_n]
    if prefix:
        sql += " AND segment >= ? AND segment < ?"
        args += [prefix, prefix + "￿"]
    if device:
        sql += " AND device = ?"
        args.append(device)
    c = db()
    items = [dict(r) for r in c.execute(sql + " ORDER BY updated LIMIT 5000", args)]
    c.close()
    return {"items": items, "now": now_ms()}


@router.get("/v1/usage")
def blob_usage(request: Request):
    """这个账号在服务器上存了什么：按设备和种类合计（设置「数据」页用）"""
    who = auth(request)
    if not who:
        return unauthorized()
    c = db()
    m: dict = {}
    for r in c.execute("SELECT device, segment, size, updated FROM blobs WHERE user = ?", (who["user"],)):
        k = (r["device"], kind_of(r["device"], r["segment"]))
        x = m.setdefault(k, {"device": r["device"], "kind": k[1], "count": 0, "bytes": 0, "oldest": r["updated"], "newest": r["updated"]})
        x["count"] += 1
        x["bytes"] += r["size"]
        x["oldest"], x["newest"] = min(x["oldest"], r["updated"]), max(x["newest"], r["updated"])
    c.close()
    return {"items": list(m.values()), "audio_max_days": AUDIO_MAX_DAYS, "now": now_ms()}


@router.get("/v1/devices")
def devices(request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    c = db()
    rows = [dict(r) for r in c.execute("SELECT device, COUNT(*) AS segments, SUM(size) AS bytes, MAX(updated) AS updated "
                                       "FROM blobs WHERE user = ? GROUP BY device ORDER BY device", (who["user"],))]
    c.close()
    return {"devices": rows}


@router.delete("/v1/account")
def delete_account(request: Request):
    """删账号：先删这个账号的全部块，再删索引和账号"""
    who = auth(request)
    if not who:
        return unauthorized()
    user = who["user"]
    d = BLOBS / user
    if d.exists():
        for p in sorted(d.rglob("*"), reverse=True):
            p.unlink() if p.is_file() else p.rmdir()
        d.rmdir()
    c = db()
    for t in ("blobs", "uploads", "sessions", "users"):
        c.execute(f"DELETE FROM {t} WHERE user = ?", (user,))
    c.close()
    return {"ok": True}


# ---------- v2：账号 ----------

def new_session(c: sqlite3.Connection, user: str, name, device) -> dict:
    """这台设备登录：发一个设备令牌"""
    sid, token, now = secrets.token_hex(8), secrets.token_hex(32), now_ms()
    c.execute("INSERT INTO sessions (id, user, name, device, token_hash, created, last_seen) VALUES (?, ?, ?, ?, ?, ?, ?)",
              (sid, user, str(name or "")[:80], str(device or "")[:80], sha256(token), now, now))
    return {"session": sid, "token": token}


def creds_ok(b: dict) -> bool:
    return (isinstance(b.get("salt"), str) and bool(SALT.match(b["salt"])) and bool(HEX64.match(str(b.get("auth") or "")))
            and isinstance(b.get("wrapped_key"), str) and len(b["wrapped_key"]) < 400)


def email_of(b) -> str:
    return str(b or "").strip().lower()


@router.get("/v2/prelogin")
def prelogin(email: str = ""):
    """登录前：取这个邮箱的盐和派生参数（没注册的邮箱给一个固定的假盐，免得被拿来试哪些邮箱注册过）"""
    email = email_of(email)
    c = db()
    row = c.execute("SELECT salt, kdf FROM users WHERE email = ?", (email,)).fetchone()
    c.close()
    if row and row["salt"]:
        return {"salt": row["salt"], "kdf": json.loads(row["kdf"])}
    return {"salt": sha256(f"beayy-fake-salt:{INVITE}:{email}")[:32], "kdf": KDF}


@router.post("/v2/register")
async def register(request: Request):
    b = await body_json(request)
    code = str(b.get("invite") or "").strip()
    invited = bool(INVITE) and same(code, INVITE)
    if code and not invited and not OPEN_SIGNUP:
        return json_resp({"error": "邀请码不对"}, 403)
    why = signup_gate(request, invited)
    if why:
        return json_resp({"error": why}, 403)
    email = email_of(b.get("email"))
    if not EMAIL.match(email) or not creds_ok(b) or not HEX64.match(str(b.get("recovery") or "")):
        return json_resp({"error": "注册信息不全"}, 400)
    c = db()
    try:
        if c.execute("SELECT 1 FROM users WHERE email = ?", (email,)).fetchone():
            return json_resp({"error": "这个邮箱已经注册过了"}, 409)
        user = secrets.token_hex(12)
        c.execute("INSERT INTO users (user, token_hash, created, email, salt, kdf, auth_hash, wrapped_key, recovery_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                  (user, sha256(secrets.token_hex(32)), now_ms(), email, b["salt"], json.dumps(KDF, separators=(",", ":")),
                   sha256(b["auth"]), b["wrapped_key"], sha256(b["recovery"])))
        if NEW_USER_LLM > 0:
            until = now_ms() + int(NEW_USER_TRIAL_DAYS * 86400 * 1000) if NEW_USER_TRIAL_DAYS > 0 else 0
            c.execute("UPDATE users SET llm_plan = 'official', llm_quota = ?, asr_quota = -1, map_quota = -1, llm_until = ? WHERE user = ?", (NEW_USER_LLM, until, user))
        count_signup(request)
        return {"user": user, **new_session(c, user, b.get("name"), b.get("device"))}
    finally:
        c.close()


@router.get("/v2/signup")
def signup_info():
    """注册前问一下：要不要邀请码、每个账号能存多少（注册框据此显示邀请码一栏和说明）"""
    full = disk_free() < MIN_FREE or bool(MAX_USERS and users_count() >= MAX_USERS)
    return {"open": OPEN_SIGNUP and not full, "invite": bool(INVITE) and not OPEN_SIGNUP, "quota": USER_QUOTA, "trial_days": NEW_USER_TRIAL_DAYS if NEW_USER_LLM > 0 else 0}


@router.post("/v2/login")
async def login(request: Request):
    b = await body_json(request)
    email = email_of(b.get("email"))
    c = db()
    try:
        row = c.execute("SELECT user, auth_hash, wrapped_key, fails, locked_until FROM users WHERE email = ?", (email,)).fetchone()
        if row and row["locked_until"] > now_ms():
            return json_resp({"error": "密码错太多次，15 分钟后再试"}, 429)
        a = str(b.get("auth") or "")
        if not row or not row["auth_hash"] or not HEX64.match(a) or not same(sha256(a), row["auth_hash"]):
            if row:
                fails = row["fails"] + 1
                c.execute("UPDATE users SET fails = ?, locked_until = ? WHERE user = ?",
                          (fails % 10, now_ms() + 15 * 60 * 1000 if fails >= 10 else 0, row["user"]))
            return json_resp({"error": "邮箱或主密码不对"}, 401)
        c.execute("UPDATE users SET fails = 0, locked_until = 0 WHERE user = ?", (row["user"],))
        return {"user": row["user"], "wrapped_key": row["wrapped_key"], **new_session(c, row["user"], b.get("name"), b.get("device"))}
    finally:
        c.close()


@router.post("/v2/recover")
async def recover(request: Request):
    """忘了主密码：用恢复码重设（设备上用恢复码拿回数据密钥，重新包一层）；之前登录的设备全部登出"""
    b = await body_json(request)
    email = email_of(b.get("email"))
    c = db()
    try:
        row = c.execute("SELECT user, recovery_hash FROM users WHERE email = ?", (email,)).fetchone()
        r = str(b.get("recovery") or "")
        if not row or not row["recovery_hash"] or not HEX64.match(r) or not same(sha256(r), row["recovery_hash"]) or not creds_ok(b):
            return json_resp({"error": "邮箱或恢复码不对"}, 401)
        c.execute("BEGIN")
        c.execute("UPDATE users SET salt = ?, auth_hash = ?, wrapped_key = ?, fails = 0, locked_until = 0 WHERE user = ?",
                  (b["salt"], sha256(b["auth"]), b["wrapped_key"], row["user"]))
        c.execute("DELETE FROM sessions WHERE user = ?", (row["user"],))
        c.execute("COMMIT")
        return {"ok": True}
    finally:
        c.close()


@router.post("/v2/claim")
async def claim(request: Request):
    """v1 账号设邮箱和主密码（迁移）：用旧的共用令牌登录着，把同一把数据密钥包一层存上来"""
    who = auth(request)
    if not who:
        return unauthorized()
    user = who["user"]
    b = await body_json(request)
    email = email_of(b.get("email"))
    if not EMAIL.match(email) or not creds_ok(b) or not HEX64.match(str(b.get("recovery") or "")):
        return json_resp({"error": "信息不全"}, 400)
    c = db()
    try:
        me = c.execute("SELECT email FROM users WHERE user = ?", (user,)).fetchone()
        if me and me["email"]:
            return json_resp({"error": "这个账号已经有邮箱了"}, 409)
        if c.execute("SELECT 1 FROM users WHERE email = ?", (email,)).fetchone():
            return json_resp({"error": "这个邮箱已经注册过了"}, 409)
        c.execute("UPDATE users SET email = ?, salt = ?, kdf = ?, auth_hash = ?, wrapped_key = ?, recovery_hash = ? WHERE user = ?",
                  (email, b["salt"], json.dumps(KDF, separators=(",", ":")), sha256(b["auth"]), b["wrapped_key"], sha256(b["recovery"]), user))
        return {"user": user, **new_session(c, user, b.get("name"), b.get("device"))}
    finally:
        c.close()


@router.get("/v2/ticket")
def get_ticket(request: Request):
    """转写、地图票据：给官方服务认账号用，1 小时有效。只拿到这张票，拿不到能读写数据的同步令牌。
    没开通、停用、这一项本月用完就不发"""
    who = auth(request)
    if not who:
        return unauthorized()
    if not TICKET_SECRET:
        return json_resp({"error": "服务器没配转写票据"}, 503)
    kind = "map" if request.query_params.get("for") == "map" else "asr"
    q = quota(who["user"], kind)
    if "error" in q:
        return json_resp({"error": q["error"]}, q["status"])
    t, exp = ticket(who["user"], kind)
    return {"ticket": t, "expires": exp * 1000}


@router.get("/v2/amap-js")
def amap_js_config(request: Request):
    """能缩放的地图：公开的地图 Key＋安全密钥中转地址（路径里带 1 小时的票据）；每打开一次地图算 1 次"""
    who = auth(request)
    if not who:
        return unauthorized()
    if not AMAP_JS_KEY or not AMAP_JS_CODE or not TICKET_SECRET:
        return json_resp({"error": "服务器没配官方地图"}, 503)
    q = quota(who["user"], "map")
    if "error" in q:
        return json_resp({"error": q["error"]}, q["status"])
    t, _ = ticket(who["user"], "map")
    add_usage(who["user"], "map", 1)
    host = OFFICIAL_HOST or str(request.base_url).rstrip("/")
    return {"key": AMAP_JS_KEY, "service_host": f"{host}/_AMapService/{t}"}


@router.post("/v2/password")
async def password(request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    b = await body_json(request)
    if not creds_ok(b):
        return json_resp({"error": "信息不全"}, 400)
    c = db()
    c.execute("UPDATE users SET salt = ?, auth_hash = ?, wrapped_key = ? WHERE user = ?", (b["salt"], sha256(b["auth"]), b["wrapped_key"], who["user"]))
    c.close()
    return {"ok": True}


@router.post("/v2/llm/chat/completions")
async def llm(request: Request):
    """官方模型：开通了的账号经这里调，真正的模型密钥只在服务器上。按月记 token，超额、没开通、停用都拒绝"""
    who = auth(request)
    if not who:
        return unauthorized()
    if not LLM_GATEWAY or not LLM_KEY:
        return json_resp({"error": "服务器没配官方模型"}, 503)
    q = quota(who["user"], "llm")
    if "error" in q:
        return json_resp({"error": q["error"]}, q["status"])
    b = await body_json(request)
    if not isinstance(b.get("messages"), list) or not b["messages"]:
        return json_resp({"error": "缺 messages"}, 400)
    # 只放行默认模型和这几个参数，别的不转
    out = {"model": LLM_MODEL or b.get("model"), "messages": b["messages"], "stream": False}
    for k in ("temperature", "max_tokens", "response_format"):
        if k in b:
            out[k] = b[k]
    req = urllib.request.Request(LLM_GATEWAY.rstrip("/") + "/chat/completions", data=json.dumps(out).encode(), method="POST",
                                 headers={"content-type": "application/json", "authorization": f"Bearer {LLM_KEY}", "user-agent": LLM_UA})
    try:
        with urllib.request.urlopen(req, timeout=170) as r:
            status, text = r.status, r.read()
    except urllib.error.HTTPError as e:
        status, text = e.code, e.read()
    except Exception:
        return json_resp({"error": "官方模型出错（连不上）"}, 502)
    try:
        v = json.loads(text)
    except ValueError:
        v = None
    if status >= 400 or not isinstance(v, dict):
        return json_resp({"error": f"官方模型出错（{status}）"}, 502)
    add_usage(who["user"], "llm", float((v.get("usage") or {}).get("total_tokens") or 0))
    return v


@router.get("/v2/account")
def account(request: Request):
    who = auth(request)
    if not who:
        return unauthorized()
    user = who["user"]
    c = db()
    try:
        me = c.execute("SELECT email, display_name, avatar, profile_updated, llm_plan, llm_quota FROM users WHERE user = ?", (user,)).fetchone()
        sessions = [dict(r) for r in c.execute("SELECT id, name, device, created, last_seen FROM sessions WHERE user = ? ORDER BY created", (user,))]
        return {
            "user": user, "email": me["email"] if me else None, "session": who["session"], "sessions": sessions,
            "profile": {"name": me["display_name"], "avatar": me["avatar"], "updated": me["profile_updated"]} if me else None,
            "llm": {"plan": me["llm_plan"] or None, "quota": me["llm_quota"], "used": usage_of(c, user).get("llm", 0), "month": month()} if me else None,
            "services": services(c, user),
            "storage": {"used": stored_of(c, user), "quota": quota_of(c, user)},
        }
    finally:
        c.close()


@router.post("/v2/profile")
async def profile(request: Request):
    """显示名称和头像"""
    who = auth(request)
    if not who:
        return unauthorized()
    b = await body_json(request)
    name = str(b.get("name") or "").strip()[:40]
    avatar = None if b.get("avatar") is None else str(b["avatar"])
    if avatar and (not avatar.startswith("data:image/") or len(avatar) > 300000):
        return json_resp({"error": "头像要是图片，且不超过 300KB"}, 400)
    now = now_ms()
    c = db()
    c.execute("UPDATE users SET display_name = ?, avatar = ?, profile_updated = ? WHERE user = ?", (name or None, avatar, now, who["user"]))
    c.close()
    return {"updated": now}


@router.delete("/v2/sessions/{sid}")
def delete_session(sid: str, request: Request):
    """移除一台设备的登录（自己退出登录也走这个，id 写 self）"""
    who = auth(request)
    if not who:
        return unauthorized()
    sid = who["session"] if sid == "self" else sid
    if not sid:
        return json_resp({"error": "这个令牌不是设备登录的"}, 400)
    c = db()
    c.execute("DELETE FROM sessions WHERE user = ? AND id = ?", (who["user"], sid))
    c.close()
    return {"ok": True}


@router.post("/v2/internal/usage")
async def internal_usage(request: Request):
    """别处的转写服务回报用量：body {u, service, n, t}，头 x-sig＝HMAC(票据密钥, body)，t 在 10 分钟内。本机的 worker.py 直接写库，不走这里"""
    raw = (await request.body()).decode(errors="replace")
    if not TICKET_SECRET or not hmac.compare_digest(request.headers.get("x-sig") or "", sign(raw)):
        return json_resp({"error": "签名不对"}, 401)
    try:
        b = json.loads(raw)
    except ValueError:
        return json_resp({"error": "不是 JSON"}, 400)
    if not USER.match(str(b.get("u") or "")) or b.get("service") not in SERVICE_NAMES or abs(time.time() - float(b.get("t") or 0)) > 600:
        return json_resp({"error": "内容不对"}, 400)
    add_usage(b["u"], b["service"], float(b.get("n") or 0))
    return {"ok": True}
