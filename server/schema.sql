-- Beayy 同步服务的账号、设备登录和索引（数据块本身是 data/sync/blobs/ 下的文件，都是密文）。启动时自动建，已有的表不动
CREATE TABLE IF NOT EXISTS users (
  user TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL,      -- v1 的共用令牌（迁移到邮箱登录前的设备还在用）
  created INTEGER NOT NULL,
  email TEXT,
  salt TEXT,                     -- 主密码派生用的盐（hex）
  kdf TEXT,                      -- 派生参数（JSON）
  auth_hash TEXT,                -- 登录校验值的 SHA-256；校验值由主密码在设备上派生，服务器拿不到主密码
  wrapped_key TEXT,              -- 用主密码派生的密钥包了一层的数据密钥（base64），服务器解不开
  recovery_hash TEXT,            -- 恢复码派生的校验值的 SHA-256
  fails INTEGER NOT NULL DEFAULT 0,
  locked_until INTEGER NOT NULL DEFAULT 0,
  display_name TEXT,             -- 账号显示名称（v3）
  avatar TEXT,                   -- 头像 data URL（v3）
  profile_updated INTEGER NOT NULL DEFAULT 0,
  llm_plan TEXT,                 -- 官方模型：空＝没开通，official＝开通，stopped＝停用（v4）
  llm_quota INTEGER NOT NULL DEFAULT 0,  -- 每月 token 额度（v4）
  llm_used INTEGER NOT NULL DEFAULT 0,   -- 本月已用（v4）
  llm_month TEXT,                -- llm_used 记的是哪个月，YYYY-MM（v4，v5 起用量记在 usage 表）
  asr_quota INTEGER NOT NULL DEFAULT 0,  -- 每月转写秒数上限（v5）
  map_quota INTEGER NOT NULL DEFAULT 0,  -- 每月地图调用次数上限（v5）
  llm_until INTEGER NOT NULL DEFAULT 0   -- 官方服务试用到哪一刻（毫秒，0＝不限；v9，老库由 init 补这一列）
);
CREATE TABLE IF NOT EXISTS usage (
  user TEXT NOT NULL,
  month TEXT NOT NULL,
  service TEXT NOT NULL,         -- llm（token）／asr（音频秒）／map（次）
  used INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user, month, service)
);
CREATE UNIQUE INDEX IF NOT EXISTS users_email ON users (email);
CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  user TEXT NOT NULL,
  name TEXT NOT NULL,
  device TEXT NOT NULL,
  token_hash TEXT NOT NULL,
  created INTEGER NOT NULL,
  last_seen INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions (user);
CREATE TABLE IF NOT EXISTS blobs (
  user TEXT NOT NULL,
  device TEXT NOT NULL,
  segment TEXT NOT NULL,
  size INTEGER NOT NULL,
  updated INTEGER NOT NULL,
  PRIMARY KEY (user, device, segment)
);
CREATE INDEX IF NOT EXISTS blobs_updated ON blobs (user, updated);
CREATE INDEX IF NOT EXISTS blobs_seg ON blobs (user, segment);   -- v6：按前缀列
CREATE TABLE IF NOT EXISTS quotas (     -- v7：单个账号另设的同步空间上限（字节，0＝不限）；没有这一行用 BEAYY_USER_QUOTA_MB
  user TEXT PRIMARY KEY,
  storage INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_daily (  -- v8：用量按天（UTC 日），看走势用；额度仍按 usage 的月合计
  user TEXT NOT NULL,
  day TEXT NOT NULL,             -- YYYY-MM-DD
  service TEXT NOT NULL,
  used INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user, day, service)
);
CREATE TABLE IF NOT EXISTS uploads (      -- v8：每台设备每小时传了几块、多少字节、被拒几次（空间满、硬盘满），看哪台设备哪天断了
  user TEXT NOT NULL,
  device TEXT NOT NULL,
  hour INTEGER NOT NULL,         -- Unix 秒 / 3600
  n INTEGER NOT NULL DEFAULT 0,
  bytes INTEGER NOT NULL DEFAULT 0,
  rejected INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (user, device, hour)
);
CREATE INDEX IF NOT EXISTS uploads_hour ON uploads (hour);
