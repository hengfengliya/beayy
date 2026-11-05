#!/usr/bin/env python3
"""管理账号的官方服务（这台服务器上的模型、转写、地图）。账号要开通才拿得到转写、地图票据，才能用服务器配的模型。
  python admin.py list                                     所有账号：开通情况、上限、本月用量
  python admin.py open <邮箱> [模型 token] [转写分钟] [地图次数]   开通（不限期），上限默认 0＝不限，-1＝这一项不开
  python admin.py trial <邮箱> <天>                         试用期改成从现在起这么多天（0＝不限期）；没开通的按新账号的额度开通
  python admin.py stop <邮箱>                              停用
  python admin.py storage <邮箱> <MB|default>               同步空间上限：0＝不限，default＝跟 BEAYY_USER_QUOTA_MB
Docker 里：docker compose exec -u beayy beayy-server python admin.py list
"""
import json
import sys

import syncapi

COLS = ("SELECT u.email, u.llm_plan AS plan, u.llm_quota, CASE WHEN u.llm_until > 0 THEN datetime(u.llm_until / 1000, 'unixepoch') END AS trial_until, CASE WHEN u.asr_quota < 0 THEN -1 ELSE u.asr_quota / 60 END AS asr_quota_min, u.map_quota, "
        "(SELECT COALESCE(SUM(used), 0) FROM usage g WHERE g.user = u.user AND g.month = ? AND g.service = 'llm') AS llm_used, "
        "(SELECT COALESCE(SUM(used), 0) FROM usage g WHERE g.user = u.user AND g.month = ? AND g.service = 'asr') / 60 AS asr_used_min, "
        "(SELECT COALESCE(SUM(used), 0) FROM usage g WHERE g.user = u.user AND g.month = ? AND g.service = 'map') AS map_used, "
        "(SELECT ROUND(COALESCE(SUM(size), 0) / 1000000.0, 1) FROM blobs b WHERE b.user = u.user) AS stored_mb, "
        "(SELECT ROUND(storage / 1000000.0, 1) FROM quotas q WHERE q.user = u.user) AS storage_mb "
        "FROM users u WHERE u.email IS NOT NULL")


def set_open(c, user: str, llm: int, asr_min: int, mp: int) -> int:
    return c.execute("UPDATE users SET llm_plan = 'official', llm_quota = ?, asr_quota = ?, map_quota = ?, llm_until = 0 WHERE user = ?",
                     (llm, asr_min * 60, mp, user)).rowcount


def set_trial(c, user: str, days: float) -> int:
    until = syncapi.now_ms() + int(days * 86400 * 1000) if days > 0 else 0
    c.execute("UPDATE users SET llm_plan = 'official', llm_quota = ?, asr_quota = -1, map_quota = -1 WHERE user = ? AND (llm_plan IS NULL OR llm_plan = '')",
              (syncapi.NEW_USER_LLM, user))
    return c.execute("UPDATE users SET llm_until = ? WHERE user = ?", (until, user)).rowcount


def set_stop(c, user: str) -> int:
    return c.execute("UPDATE users SET llm_plan = 'stopped' WHERE user = ?", (user,)).rowcount


def set_storage(c, user: str, mb: float | None):
    """mb：上限 MB（0＝不限）；None＝删掉单独设的，跟 BEAYY_USER_QUOTA_MB"""
    if mb is None:
        c.execute("DELETE FROM quotas WHERE user = ?", (user,))
    else:
        c.execute("INSERT INTO quotas (user, storage) VALUES (?, ?) ON CONFLICT (user) DO UPDATE SET storage = excluded.storage", (user, int(mb * syncapi.MB)))


def main(a: list[str]) -> int:
    if not a or a[0] not in ("list", "open", "stop", "storage", "trial"):
        print(__doc__)
        return 2
    syncapi.init()
    c = syncapi.db()
    m = syncapi.month()
    if a[0] == "list":
        for r in c.execute(COLS + " ORDER BY u.email", (m, m, m)):
            print(json.dumps(dict(r), ensure_ascii=False))
        return 0
    if len(a) < 2:
        print(__doc__)
        return 2
    email = syncapi.email_of(a[1])
    row = c.execute("SELECT user FROM users WHERE email = ?", (email,)).fetchone()
    if not row:
        print(f"没有 {email} 这个账号")
        return 1
    if a[0] == "storage":
        if len(a) < 3:
            print(__doc__)
            return 2
        try:
            set_storage(c, row["user"], None if a[2] == "default" else float(a[2]))
        except ValueError:
            print("上限写 MB 数（0＝不限）或 default")
            return 2
        print("改好了")
        return 0
    if a[0] == "trial":
        try:
            n = set_trial(c, row["user"], float(a[2]))
        except (IndexError, ValueError):
            print("写天数，比如 trial a@b.com 7")
            return 2
        print(f"改了 {n} 个账号")
        return 0
    if a[0] == "stop":
        n = set_stop(c, row["user"])
    else:
        try:
            llm, asr, mp = (int(x) for x in (a[2:] + ["0", "0", "0"])[:3])
        except ValueError:
            print("上限要是整数")
            return 2
        n = set_open(c, row["user"], llm, asr, mp)
    print(f"改了 {n} 个账号")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
