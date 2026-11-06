# Tauri 命令清单（网页界面 ↔ Rust）

网页用 `window.__TAURI__.core.invoke(命令名, 参数)` 调用（`app.withGlobalTauri = true`，不用打包工具）。不在 Tauri 里（直接用浏览器开 `app/ui/index.html` 调样式）时，读 `app/ui/mock/<命令名>.json`。

所有时间：日期用本地日 `YYYY-MM-DD`；时刻用 ISO8601（UTC，带 Z），页面自己换成本地时间显示。所有时长单位是秒。

| 命令 | 参数 | 返回 |
|---|---|---|
| `get_status` | — | `{ "permissions": {"accessibility": bool, "input_monitoring": bool, "full_disk_access": bool}, "capturing": bool, "paused": bool, "events_today": int, "phone_sync": "iphone-1A2B3C4D 同步截至 …", "relay_sync": "新增 12 条，服务器最近一天 2026-10-16"（旧 v1 录音服务器上一轮拉取的结果或错误，过渡期）, "cloud_sync": "新增 3 条，转写 2 段"（录音和停留点走账号同步的上一轮结果，见 beayy-remote/src/cloud.rs；没登录是「没登录账号」）, "device": "mac-…", "version": "0.2.0", "paused_until": ISO 或 null, "login_item": bool }`。`events_today` 是今天本机写的总条数（不是本次启动以来） |
| `list_days` | — | `[{ "date": "2026-10-15", "line": "今天的一句话（没有日总结为 null）", "active_sec": int, "phone_sec": int, "hc": {"09": 秒, …}, "hp": {"09": 秒, …} }]`，新的在前；`hc`／`hp` 是每小时电脑／手机分钟数（同网站索引），给作息图用 |
| `get_day` | `{ "date": "YYYY-MM-DD" }` | `{ "date", "pack": <日数据/日数据_YYYYMMDD.json 原样，没有为 null>, "summary_md": <日总结 Markdown 原文或 null>, "segments": [{ "start": ISO, "end": ISO, "kind": "10min"/"20min"/"30min"/"60min"（摘要间隔调长后一份管多长，看文件名）/"6h"/"idle"（连续「不在电脑前」合成的一段）, "source": "beayy"/"codex", "title", "description", "memory", "context": [..] }], "phone": { "total_sec", "pickups", "late_sec", "night_sec", "apps": [{"bundle_id", "name", "sec"}], "hours": {"09": 秒, …}, "synced_until": ISO, "sessions": [{ "start", "end", "sec", "kind": "session"/"brief"（连着的零碎拿起合成一行）, "count", "apps": [{"bundle_id", "name", "sec"}] }] }, "live": 今天（或还没有数据包的那天）从自采事件实时算的电脑 `{ "active_sec", "first", "last", "switches", "active_hours": {"09": 秒}, "spans": [[ISO, ISO]], "apps": [{"name", "bundle_id", "sec"}] }` 或 null, "speech": 对话（手表录的，和 iPhone 自己录的；见下）或 null（没有 watch-* 设备、iPhone 也没录过）, "places": { "visits": [{ "id", "start": ISO, "end": ISO 或 null（还在那儿）, "arrive_known": bool, "arrive": 真实到达时刻 ISO（前一天到的也给）或 null, "name": 显示名（自己起的标签，没有就是反查的地名）, "place_name", "label", "type": "home"/"work"/"other"/null, "source": "visit"/"slc"/"watch", "device": 记它的设备（iphone-…／watch-…）, "lat", "lon" }] }（读 iphone-*、watch-* 两种设备；读法同 脚本/日数据包.py 的 read_places／load_stays：同 id 取 rev 最大的一版；可信程度 iPhone CLVisit > slc > 手表，低的和高的重叠过半丢掉；没离开的算到下一个到达或现在；同一地点 15 分钟内并段。标签读 Beayy/places/labels.json，没有再读外部数据目录 raw/places/labels.json：[{label, type, lat, lon, r（米，默认 150）}] } 或 null }`；segments 每段另有 `"apps": [bundle id]`。`speech`：`{ "recorded_sec", "talk_sec"（所有人说话秒数）, "self": 本人在转写里的名字或 null, "self_sec", "speakers_sec": {"他人1": 秒}, "hours": {"09": 说话秒}, "first", "last", "recorded": [[ISO, ISO]], "gaps": [[ISO, ISO]]（录音断档，空 5 分钟以上）, "battery": [首, 末] 或 null, "segments": 段数, "conversations": [{ "start", "end", "device": 录它的设备（watch-…；没戴手表时 iPhone 自己录的是 iphone-…）, "talk": [{"speaker", "sec"}], "lines": [{"start", "end", "speaker", "text"}]（同一个人 20 秒内接着说的并成一行） }]（两句之间空 3 分钟以上算另一段，同 脚本/日数据包.py） }` |
| `get_rules` | — | `[{ "id": "r1", "text": "21:00 硬停", "enabled": true }]`（存 `Beayy/rules.json`） |
| `save_rules` | `{ "rules": [...] }` | `null` |
| `get_settings` | — | `{ "gateway": "https://api.openai.com/v1", "model_segment": "gpt-4.1-mini", "model_daily": "gpt-4.1-mini", "repo_path": "", "blocklist": ["com.1password.1password"], "has_api_key": bool, "model_mode"、"map_mode": "official"/"custom"（官方服务按账号开通，或自己的网关、自己的高德 Key）, "api_key_source": "env"/"keychain"/null, "onboarded": bool, "relay_server": "https://audio.example.com"（录音服务器，没填为空）, "has_relay_token": bool, "has_amap_key": bool, "has_amap_js": bool, "default_gateway"/"default_model"（编译时的默认，BEAYY_DEFAULT_GATEWAY／BEAYY_DEFAULT_MODEL）, "sync_server"（注册登录用的同步服务器，没填＝官方；手机也能改）, "default_sync_server", "summary_minutes": 10/20/30/60（摘要间隔；没选过的官方模型 60、自己的模型 10；调长后一份摘要管几个分段，问一次模型）, "summary_minutes_set": false（自己选过间隔没有） }`（没填的网关、模型按默认值给；存 `Beayy/settings.json`，密钥在钥匙串，不经网页回传） |
| `save_settings` | `{ "settings": {...} }`（不含密钥；只写传进来的键，可以只传一两个） | `null` |
| `set_api_key` | `{ "key": "sk-…" }` | `null`（写钥匙串） |
| `llm_status` | — | `{plan: null\|"official"\|"stopped", month, llm: {used, quota}, asr: {used, quota}（秒）, map: {used, quota}（次）}` 或 `{error}`（官方服务在这个账号上开通了没有、三项本月各用了多少；要联网，异步；方案见 报告/方案_20261021_官方模型开通.md） |
| `phone_read_now` | — | `null`（iPhone 屏幕使用时间不等 5 分钟，马上读一次；「怎么设置」弹层的「现在读取」） |
| `storage_stats` | — | `{local: {path, total, items: [{key, name, bytes, keep}]}, cloud: {server, total, items: [{key, name, count, bytes, oldest, keep}]} 或 {error}, transcribe: {server, jobs, bytes, keep} 或 {error}, repo: {path, bytes, keep} 或 null, at}`（设置「数据」页：本机、Cloudflare、转写服务、人生记录仓库各存了什么、多大、留多久；要联网，异步） |
| `set_relay_token` | `{ "token": "…" }`（空字符串＝删掉） | `null`（录音服务器口令写钥匙串，账号 relay_token；Windows 写数据目录 secrets/relay_token） |
| `set_paused` | `{ "paused": bool, "minutes": int?（给了就是定时暂停，到点自动继续） }` | `null` |
| `set_login_item` | `{ "on": bool }` | `null`；失败返回系统给的错误 |
| `list_fonts` | — | `[{ "family": CSS font-family 用的名字, "name": 显示名（macOS 本地化，如「苹方-简」） }]`，按显示名排、去重，不含 . 开头的系统私有字体和 @ 开头的竖排字体（设置 → 通用 → 字体；macOS 问 NSFontManager，Windows 用 GDI 枚举；手机核心没有这个命令） |
| `delete_since` | `{ "minutes": int }` 或 `{ "today": true }` | 删掉的条数。只删本机电脑事件和对应分段的 10 分钟摘要；iPhone 记录、已归档进仓库的不动 |
| `list_devices` | — | `[{ "id": "mac-…"/"iphone-…"/"watch-…", "kind": "mac"/"iphone"/"win"/"watch", "name", "this": 是否本机, "first": ISO, "last": ISO, "events": 条数, "bytes": 占用 }]`，本机排第一（读数据目录 devices/，缓存 60 秒） |
| `set_place_label` | `{ "lat", "lon", "label": "家", "kind": "home"/"work"/"other" }`（label 空＝删掉这个名字） | `null`；写 Beayy/places/labels.json：半径内已有的改它，没有就新加一条（r 150 米）。第一次写时先抄外部数据目录 raw/places/labels.json 里已有的 |
| `recent_events` | `{ "device": "mac-…", "before": ISO?, "limit": 200?, "types": ["keyboard.", "mouse.click"]? }` | `{ "events": [原始事件，新的在前], "more": bool }`。设备详情「事件」页签用：从最新分段往回读，够 limit 条（最多 1000）或翻了 48 个分段就停；`before` 接着读更早的；`types` 以「.」结尾按前缀。打的字、界面文字原样返回（采集时密码框已打码），只给本机界面，不进网站 |
| `set_place_pick` | `{ "id": 停留点 id, "shop": "店名", "kind": 高德分类?, "lat", "lon" }` | `null`；确认这段停留去了这家店，写 Beayy/places/picks.json（stays[id].shops 加一家，remembered 里记一条，同一处同名只留一条）。一段停留可以认定好几家；已认定的同一家再调一次＝取消这家（remembered 不动）。旧格式 stays[id] = {shop, type} 照样认。以后 30 米内的停留，候选里有它就当已确认 |
| `clear_place_pick` | `{ "id" }` | `null`；撤销这段停留认定的所有店，名字退回只写区域 |
| `list_place_labels` | — | `[{label, type, lat, lon, r, address?}]` 常去的地方（同 set_place_label 读的名单） |
| `edit_place_label` | `{ "index", "label", "kind", "r", "address"? }` | `null`；改名单里的一项（label 空＝删掉），r 限 30–1000 米 |
| `set_amap_key` | `{ "key" }` | `null`；高德「Web 服务」Key 存钥匙串（账号 amap_key），Windows 写数据目录；空字符串＝删掉。不回传网页 |
| `amap_around` | `{ "lat", "lon", "radius": 150? }` | `[{id, name, type, distance, lat, lon}]` 高德附近的去处（按距离，WGS-84） |
| `amap_search` | `{ "text", "lat"?, "lon"? }` | 同上；有位置按距离（50 公里内），没有全国按相关度 |
| `amap_map` | `{ "points": [[lat, lon], …], "width"?, "height"? }` | `"data:image/png;base64,…"` 高德静态地图，停留点按先后编号、连线 |
| `sync_status` | — | `{ configured, mode, email, server, folder, webdav, user, last, last_error, uploaded, need_login, remote_devices, progress, running, shadow }`；`mode`＝`"server"`／`"folder"`／`"webdav"`／`"icloud"`（没开同步是 null）；`server` 只在 server 模式有；`folder` 是同步文件夹或 iCloud 的路径；`webdav` 是 WebDAV 地址 |
| `webdav_state` | `{ "url", "user", "pass" }` | `{ ok, vault, error? }`：连不连得上、里面有没有 vault.json（有＝用加入，没有＝新开） |
| `webdav_create` / `webdav_join` / `webdav_recover` | `{ url, user, pass, password }`（recover 另要 `code`） | create 返回恢复码，另两个 `null`。地址、用户名、密码只存进账号（钥匙串），不进设置；坚果云要用第三方应用密码 |
| `icloud_state` | — | `{ available, path?, vault, error? }`：Mac 要 iPhone 先用过 iCloud（容器同步过来才有）；Windows、安卓 `available: false` |
| `icloud_create` / `icloud_join` / `icloud_recover` | `{ password }`（recover 另要 `code`） | create 返回恢复码，另两个 `null` |
| `sync_now` | — | `{ uploaded, downloaded, errors }` |
| `account_signup` / `account_login` / `account_claim` / `account_recover` / `account_password` | 邮箱、主密码（signup 另要 invite，recover 另要 code） | signup、claim 返回恢复码 |
| `account_info` | — | `{ user, email, session, sessions: [{id, name, device, created, last_seen}], profile: {name, avatar, updated} }` |
| `account_remove_device` | `{ "id" }` | `null` |
| `account_logout` / `account_recovery_code` | — | `null` ／ 恢复码（account_logout 也用来退出同步文件夹、iCloud、WebDAV：本机忘掉账号，那边的数据不动） |
| `get_profile` / `save_profile` | — ／ `{ name, avatar: data URL 或 null }` | `{ name, avatar, email }` ／ `null` |
| `list_projects` | — | `[{ name, domain, sec, last, aliases }]` |
| `merge_project` / `set_project_domain` | `{ from, into }` ／ `{ project, domain }` | `null` |
| `get_icons` | `{ "ids": [bundle id], "phone": bool }` | `{ id: { "icon": "data:image/png;base64,…" 或 null, "name": App Store 上的名字或 null } }`。电脑取本机图标；手机查 App Store（先中国区再美区），查不到的苹果自带 app 用 Mac 上对应的；缓存在 `Beayy/icons/` |
| `open_permission`（kind 另可为 screentime：打开系统设置的屏幕使用时间页） | `{ "kind": "accessibility" \| "input_monitoring" \| "full_disk_access" }` | `null`（打开系统设置对应页） |
| `open_data_dir` | — | `null` |
| `pick_bill_files` | — | `["路径", …]`（选账单文件，可多选；macOS 用 osascript、Windows 用 PowerShell 的选文件窗口；取消是 `[]`） |
| `import_bills` | `{ "paths": ["…"] }` | `{ files: [{ name, src, format: "微信"/"支付宝"/"支付宝网页版", export, rows, added, updated, skipped } 或 { name, error }], added, updated }`。存 `Beayy/bills/bills.json`；同一笔按交易号认，重复导入不重复记，导出晚的为准 |
| `get_bills` | `{ "month": "YYYY-MM" 或 "YYYY"? }`（不给＝有账单的最近一个月） | `{ month, months, years, items: [{ id, src, time, dir, fen, refund_fen, counted, is_refund, ttype（交易类型）, ttype_by, ctype（消费类型，只给消费）, ctype_by（manual 你改的／bill 账单上的／rule 你定的／keyword 按名字认的／ai AI 分的）, who（对方，转账红包按人并过）, counterparty, item, account（付款账户）, credit, discount, channel, fee, status, note, kind }], coverage, imports: [{ …, stored（原文件，相对 Beayy/bills/）, check: {bill_in, bill_out, ours_in, ours_out, n_in, n_out, bill_n_in, bill_n_out, ok}, overlap: [{file, format, export, n}]（和同一来源别的账单重合几笔）, only（只在这份里的几笔）}], ttypes, ctypes, unclassified（没消费类型或只有账单写的「其他」）}`。金额都是分。算法见 `src-tauri/src/money.rs` 开头 |
| `get_bill` | `{ "id" }` | 同上一笔，另加 `raw`（原表格这一行的全部列）、`export`、`remembered`（这个对方有没有你定的规则） |
| `set_bill_class` | `{ id, ttype?, ctype?, remember? }`（ttype、ctype 都不给＝撤销手改） | `null`。手改存 `Beayy/bills/overrides.json`；remember＝同一个对方以后都这样，存 `bills/rules.json` |
| `classify_bills_ai` | — | `{ asked, done, error }`。规则分不出的消费交给模型（只发对方名和说明），结果存 `bills/ai.json`，用量记在「钱」名下 |
| `money_overview` | `{ "month": "YYYY-MM" 或 "YYYY"? }` | `{ period, is_month, months, upto（这个月的账单只到哪天，过完了是 null）, totals: {out, in, net, rate, consume, n, refund}, prev（前三个月平均；upto 不空时是同期）, by_ttype, by_ctype: [{name, fen, n, avg3, change, share, series（近 12 个月）}], trend: [{month, consume, out, income, has, by（消费按消费类型 {类: 分}）}], palette（图上上色的 5 个消费类型，按全部账单的消费排，别的画成「其余」）, days: [{date, consume, out, by}], merchants, people（转账、红包、亲友代付的来往，按人并）, accounts, credit, credit_prev, late: {n, fen, prev_n}, recurring: [{name, ctype, months, avg, last, last_month, rise, new}], alerts: [{level, text, ctype?, id?, q?, action?}], budgets: [{text, name, limit, spent, ok}]（规矩里「X每月不超过 N」）, unclassified, insight（这个月的 AI 总结 Markdown 或 null） }` |
| `generate_money_insight` | `{ "month": "YYYY-MM" }` | Markdown（「发现」「建议」两段）；AI 写这个月的「钱」总结：图上已有的数不复述，写变化怎么来的、什么时候花的、和作息的关系。材料只给汇总和代码算的线索（`beayy --dump money_material YYYY-MM` 看），存 `bills/insights/YYYY-MM.md` |
| `rebuild_bills` | — | `{ files, bills, missing, errors }`；从存下的原文件（`bills/files/`）把账单全部重算 |
| `delete_bill_import` | `{ "format": "…", "export": "YYYY-MM-DD HH:MM:SS" }` | `{ bills, removed }`；删掉这份导入和它的原文件，用剩下几份的原文件从头重算（只在这份里的那些笔就没了）。剩下的有没存原文件的，报错不删 |
| `games_overview` | `{ "game": "lol" 或 "steam"? }`（不给就选最近玩过的那款） | 英雄联盟：`{ game: "lol", games: [{id, n, last}]（记过的游戏才列）, account: {name, tag, level, region, platform, at, device} 或 null, rank: {at, queues: [{queue, tier, division, lp, wins, losses}]} 或 null, friends: [{puuid, name, tag, n, w, last, games（一起打的 gameId）}]（同队 2 局以上，页面按时间段再数、5 局以上才显示）, matches: [{id, start, dur, device, queue_id, queue, mode, map, version, champ_id, champ, win, remake（不到 5 分钟）, k, d, a, cs, dmg, gold, vision, level, lane, role, kp, dmg_share, spells, items（7 格，第 7 格是饰品，0＝空）, badges（mvp、svp、penta、quadra、triple、fb、top_dmg、top_tank）, night（23:00–5:00 开的）, phases?: {queue, select, load, post}（秒；补写的历史局没有）, position?, rank?: {lp} 或 {to: {tier, division, lp}}（排位局前后段位一比）}]（新的在前）, days: [{date, n（不含重开）, w, remakes, play, wait（排队＋选人＋读条，含没进成游戏的）, night, games: [{id, start, dur, win, remake}]}]（新的在前） }`；英雄联盟，汇总各台 Windows 电脑同步来的 `game.*` 事件（`src-tauri/src/games.rs`），电脑版才有；`get_settings`／`save_settings` 的 `games`（设置 → 功能 → 游戏，总开关，默认关）和 `games_lol`、`games_steam`（每款的开关，默认开）只管采不采、侧栏出不出、页面列哪几款，不挡这个命令。Steam：`{ game: "steam", games, account: {name（Steam 昵称）} 或 null, sessions: [{appid, name, start, dur, device, live（还在玩、算到现在）, night_s（23:00–5:00 的秒数）}]（新的在前）, days: [{date, n, play, night_s, games: [{appid, name, start, dur}]}]（新的在前）, library: [{appid, name（Steam 缓存里也没有的为 null）, total_min, last}]（按总时长排） }`，汇总 `steam.*` 事件 |
| `get_game` | `{ "id": gameId }` | `{ match（同上一局）, teams: [{id, mine, win, kills, dmg, gold, bans: [{id, name}], players: [{name, tag, me, champ_id, champ, level, k, d, a, cs, dmg, gold, vision, win, taken（承伤）, items, spells, badges}]}]（自己那队在前） }`；没有这局给 null |
| `game_icons` | `{ "kind": "champ"｜"item"｜"spell", "ids": [编号] }` | `{编号: data URL 或 null}`；英雄联盟的英雄、装备、召唤师技能图标，取自 CommunityDragon（按编号；装备、技能先查它的清单），存在 `Beayy/icons/lol/<kind>/`，服务器回 404 的记 `.none` 不再取，没连上的下次再试 |

**实现状态（10-15）**：表里命令都已在 Rust 实现（`src-tauri/src/main.rs`、`src-tauri/src/data.rs`），和 `ui/mock/生成样例.py` 的输出逐项对过：31 天 `list_days`、10-15 的 `get_day` 完全相同。调试可以不开界面直接看结果：`app/target/release/beayy --dump list_days`、`--dump get_day 2026-10-15`、`--dump get_settings`、`--dump get_rules`。`settings.json` 里沿用 Python 摘要脚本的键名（summary_model、daily_model、blacklist），由 Rust 换成上表的叫法。

**并跑版自检（只在影子模式下有效）**：
- 不经菜单栏打开主窗口：`open -a ~/Applications/Beayy-rs.app`（已在运行时会打开主窗口），或启动参数 `--open`。
- 截图钩子：往 `~/Library/Application Support/Beayy/shadow/snapshots/request.json` 写 `{"name": "…", "js": "…"}`。app 每秒看一次：打开主窗口 → 执行 js → 等 1 秒 → 截主窗口写 `snapshots/<name>.png`，结果写 `<name>.result.txt`，请求文件随即删除。锁屏时系统不给窗口内容，截出来是白的。
- `dev_report`：`invoke("dev_report", {name, text})` 把页面状态写到 `snapshots/<name>.txt`，配合截图钩子读页面里的数据。
- 主窗口标题栏是 Overlay、隐藏标题，系统窗口按钮叠在侧栏左上角。

`pack` 的字段以 `网站/build.py` 和 `网站/static/app.js` 现在用的为准（computer、phone、codex、git、cover 等）。新增字段只加不改名。

**手表对话、iPhone 停留点（10-16）**：`crates/beayy-remote` 每 10 分钟从录音服务器（`GET /v1/days`、`/v1/transcripts/<日>`、`/v1/places/<日>`，Bearer 口令）拉回来，写成 `speech.segment`／`speech.utterance`（`devices/watch-<id>/`）和 `place.visit`（`devices/iphone-<id>/`，同一台 iPhone 的目录）。调试：`beayy --relay-pull` 拉一轮；`beayy --relay-import transcripts|places <文件>` 把本地 jsonl 当服务器数据写进去；`echo 口令 | beayy --set-relay-token` 存口令。

**停留点取名（10-21）**：地名、候选都是手机用高德算的（见 录音/README.md「停留点取名」），随停留点传到录音服务器（`guesses` 字段）再拉回来。`get_day` 的 `places.visits` 每项多了 `acc`、`guesses`（最多 5 个：name、type、p、distance）、`doing`（{kind, p}：候选按高德大类加总）、`confirmed`（"picked" 在电脑上确认的／"phone" 在手机上确认的（名字三层）／"auto" 电脑算的第一名把握到 50% 以上自动写上的／"remembered" 同 auto 但是 30 米内确认过的店／null）、`shops`（认定的店，几家都列，没认定是 []；名字写「区域 · A、B」，三家以上「A、B 等 N 家」）。在电脑上认定过的也照样给 `guesses`，好再加别的店。同一段两条记录时，地名和候选取定位更准的那条（到达、离开都相差 10 分钟内才算同一段）。

**候选电脑为准（10-21 定：电脑端为准、其他端同步）**：`guesses` 由电脑自己算（`amap::guesses_for`：高德周边 50–120 米＋同手机的打分，缓存在 Beayy/places/guesses.json，按「停留点 id:版本:离开时刻」；问不到 10 分钟内不再试），手机传来的只在电脑问不到时兜底。没离开的记录按下一段到达算待了多久。

**地点分级、常去（10-21）**：`get_day` 的 `places.visits` 每项多 `admin`（{country, province, city, district, township, adcode（GB/T 2260 六位）, towncode, area}，电脑用高德逆地理编码取，缓存 Beayy/places/admin.json，按坐标四位小数＋误差档）和 `city_prefix`（和种类是家的那个起名地方不在一个城市时给城市名）；名字的「区县 · 区域」由电脑算，手机传来的只兜底。`place_stats { days?: 30 }` → `{days, named: [{name, count, sec, last, type, address, r, index}], frequent: [{name, district, count, sec, last, lat, lon}]}`：最近 N 天按地方合计，起过名的按名字、没起名的按区域（去过 5 次以上），只用缓存不问高德。

**能缩放的地图（10-21）**：`amap_js_config` → `{key, code}`／null（高德「Web端(JS API)」Key 和安全密钥，钥匙串 amap_js_key／amap_js_code，页面用来加载 JS API 2.0）；`set_amap_js { key, code }`。地点 · 地图用它画可拖动缩放的地图，没存或加载失败退回 `amap_map` 静态图。`amap_map` 多了 `often: [bool]`（常去的点深一点）。

**日总结、周总结（10-29）**：`generate_daily { date }` → Markdown（写 Beayy/daily/日总结_YYYYMMDD.md）；材料多了去了哪（停留点，前一天到的从 0 点算）和对话（按小时摘录，说得多的 15 个小时、6000 字以内）。`daily_status { date }` → 那天没写成时 `{error, at}`，否则 null。`get_week { date }` → 那天所在那周 `{monday, sunday, summary_md, error, days: [{date, line（日总结的一句话或 null）}]}`；`generate_weekly { date }` → 周总结 Markdown（Beayy/daily/周总结_<周一>.md，用那周的日总结写）。定时：21:30 写当天；0:15 以后每 10 分钟看最近 7 天（缺的、旧写法的、当天晚上写的、之后别的设备又来了 32KB 以上数据的重写）和前两周的周总结（那周日总结比周总结新就重写），一轮最多 2 份；每次结果记 Beayy/daily/state.json，失败隔 30 分钟起翻倍重试、最多 6 小时，日志 Beayy/logs/daily.log。命令行 `beayy --daily catch-up` 跑一遍补写，`--weekly [日期]` 写那周的周总结。get_day 的 `summary_md` 以 Beayy/daily/ 的为准，没有再读外部目录的 日总结/。

**苹果健康（10-29）**：`get_day` 多 `health`（没有健康数据为 null）：`{ device, sleep: 昨晚（前一天 18:00 到当天 18:00）{ start, end, asleep_sec, inbed_sec, awake_sec, core_sec, deep_sec, rem_sec, staged（有手表分期）, blocks: [{start, end, stage: "asleep"/"awake"/"core"/"deep"/"rem"}] } 或 null, sleep_blocks: 当天 0–24 点内的睡眠段（一天的形状画它）, activity: { steps, active_kcal, basal_kcal, exercise_min, stand_hours, distance_m, flights, step_hours: {"09": 步} }, heart: { avg, min, max, n, resting, hrv_ms }, workouts: [{ name, start, end, sec, kcal, distance_m, avg_hr, source }], moods: [{ at, valence（-1～1）, kind, labels, associations }], mindful_sec, others: [{ type, name, agg: "sum"/"avg"/"count", value, min, max, unit, n }] }`（`src-tauri/src/health.rs`，数据见 数据源/数据源_20261029_苹果健康.md）。`get_range` 每天多 `sleep_sec`、`steps`。MCP 的 `get_day` 也带 `health`（不含睡眠段）。
