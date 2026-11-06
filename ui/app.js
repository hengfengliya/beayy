// Beayy 网页界面：今天、日总结、规矩、设置四页。纯 JS，不用打包工具。
// 取数：Tauri 里用 window.__TAURI__.core.invoke（命令见 app/src-tauri/COMMANDS.md）；
// 浏览器里读 mock/<命令名>.json（双击 file:// 打开时改读 mock/mock.js）。
// 作息图、一天的形状、读数的画法取自 网站/static/app.js（设计系统「人生记录」）。

const $app = document.getElementById("app");
const TAURI = window.__TAURI__ && window.__TAURI__.core;
trDom(document.querySelector(".shell"));  // 侧栏、窗口按钮这些写在 index.html 里的字（i18n.js）
// 手机版没有开发者工具：网页报错交给外壳记进日志（电脑版没有这个命令，调用失败就算了）
if (TAURI) {
  const report = (t) => { try { TAURI.invoke("web_log", { text: t }).catch(() => {}); } catch (e) {} };
  window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => report(`未处理：${e.reason && (e.reason.stack || e.reason.message || e.reason)}`));
}

// ---------- 取数 ----------

let MOCK = null;
const mockState = {};

async function loadMock(name) {
  if (location.protocol !== "file:") {
    try {
      const r = await fetch(`mock/${name}.json`, { cache: "no-cache" });
      if (r.ok) return r.json();
    } catch (e) { /* 退回 mock.js */ }
  }
  if (!MOCK) {
    await new Promise((ok, fail) => {
      const s = document.createElement("script");
      s.src = "mock/mock.js"; s.onload = ok; s.onerror = () => fail(new Error("读不到 mock/mock.js，先跑 mock/make_mock.py"));
      document.head.appendChild(s);
    });
    MOCK = window.BEAYY_MOCK;
  }
  return JSON.parse(JSON.stringify(MOCK[name]));
}

/** 后台给的拼好的一句（报错、状态）：英文界面先整句查表，查不到就拿表里带 {…} 的原句当模板对，
 *  对上了把各段值也翻了再填进英文；和 Rust 的 beayy_core::i18n::tr_msg 同一套规则 */
const HAN = /[\u4e00-\u9fff]/;
let MSG_TPL = null;
function trMsg(s) {
  if (LANG !== "en" || typeof s !== "string" || !HAN.test(s)) return s;
  const dict = window.I18N_EN || {};
  if (dict[s] != null) return tr(s);
  if (!MSG_TPL) {
    const ph = /\{([^{}]*)\}/g;
    // 两个占位符挨着的（{e}{BUSY}）对不准，不当模板
    MSG_TPL = Object.keys(dict).filter((k) => /\{[^{}]*\}/.test(k) && !/\}\{/.test(k) && typeof dict[k] === "string").map((k) => {
      const names = [...k.matchAll(ph)].map((m) => m[1].split(":")[0]);
      const lits = k.split(ph).filter((_, i) => i % 2 === 0);
      const re = new RegExp("^" + lits.map((l) => l.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("([\\s\\S]+?)") + "$");
      return { re, names, en: dict[k], len: lits.join("").length };
    }).sort((a, b) => b.len - a.len);
  }
  for (const t of MSG_TPL) {
    const m = t.re.exec(s);
    if (!m) continue;
    const vals = m.slice(1).map(trMsg);
    let nth = 0;
    const pos = t.names.map((n, i) => (n ? -1 : i)).filter((i) => i >= 0);
    return t.en.replace(/\{([^{}]*)\}/g, (_, n) => { const name = n.split(":")[0]; const i = name ? t.names.indexOf(name) : pos[nth++]; return i >= 0 && i != null ? vals[i] : ""; });
  }
  return s;
}
/** 后台回来的结果里报错类的字段（error、errors、last_error）走 trMsg */
function trResult(v, depth = 0) {
  if (LANG !== "en" || !v || typeof v !== "object" || depth > 4) return v;
  for (const k of Object.keys(v)) {
    const x = v[k];
    if ((k === "error" || k === "last_error") && typeof x === "string") v[k] = trMsg(x);
    else if (k === "errors" && Array.isArray(x)) v[k] = x.map(trMsg);
    else if (x && typeof x === "object") trResult(x, depth + 1);
  }
  return v;
}

async function call(cmd, args = {}) {
  try { return trResult(await callRaw(cmd, args)); } catch (e) { throw typeof e === "string" ? trMsg(e) : e && typeof e.message === "string" ? new Error(trMsg(e.message)) : e; }
}

async function callRaw(cmd, args = {}) {
  if (TAURI) return TAURI.invoke(cmd, args);
  // 浏览器里：写操作只改内存，读操作读样例
  switch (cmd) {
    case "save_rules": mockState.rules = args.rules; return null;
    case "save_settings": mockState.settings = { ...(mockState.settings || await loadMock("get_settings")), ...args.settings }; return null;
    case "set_api_key": mockState.hasKey = true; return null;
    case "set_paused": mockState.paused = args.paused; return null;
    case "open_permission": case "open_data_dir": console.info(`样例模式：${cmd}`, args); return null;
    case "list_fonts": return [["PingFang SC", "苹方-简"], ["Songti SC", "宋体-简"], ["Kaiti SC", "楷体-简"], ["Hiragino Sans GB", "冬青黑体简体中文"], ["Helvetica Neue", "Helvetica Neue"], ["Avenir Next", "Avenir Next"], ["Georgia", "Georgia"], ["Menlo", "Menlo"], ["Courier New", "Courier New"], ["Times New Roman", "Times New Roman"]].map(([family, name]) => ({ family, name }));
    case "get_rules": return mockState.rules || loadMock("get_rules");
    case "get_bills": case "money_overview": { const all = await loadMock(cmd); return all[args.month] || all[Object.keys(all).filter((k) => k.length === 7).sort().pop()]; }
    case "get_bill": {
      const one = await loadMock("get_bill").catch(() => ({}));
      if (one[args.id]) return one[args.id];
      for (const p of Object.values(await loadMock("get_bills"))) { const b = p.items.find((x) => x.id === args.id); if (b) return { raw: {}, remembered: false, ...b }; }
      return null;
    }
    case "get_game": { const all = await loadMock("get_game").catch(() => ({})); return all[args.id] || null; }
    case "games_overview": { const all = await loadMock(cmd); return all[args.game] || all[all.default]; }
    case "game_icons": return {}; // 样例不带英雄、装备图（那是 Riot 的图），只显示首字
    case "pick_bill_files": return [];
    case "set_bill_class": case "rebuild_bills": console.info(`样例模式：${cmd}`, args); return null;
    case "classify_bills_ai": return { asked: 0, done: 0, error: null };
    case "generate_money_insight": return "## 钱去哪了\n\n样例模式不调模型。";
    case "get_settings": {
      const s = mockState.settings || await loadMock("get_settings");
      if (mockState.hasKey) s.has_api_key = true;
      return s;
    }
    case "get_status": {
      const s = await loadMock("get_status");
      if (mockState.paused !== undefined) s.paused = mockState.paused;
      return s;
    }
    case "get_day": return (await loadMock("get_day"))[args.date] || null;
    case "daily_status": return null;
    // 同步：换方式、新开、加入只改内存，方便在浏览器里点一遍
    case "sync_status": return mockState.sync || loadMock("sync_status");
    case "account_logout": mockState.sync = { configured: false }; return null;
    case "account_stop_sync": { const cur = mockState.sync || await loadMock("sync_status"); mockState.sync = { configured: false, service_email: cur.email }; return null; }
    case "account_service_login": case "account_service_signup": mockState.sync = { ...(mockState.sync || await loadMock("sync_status")), service_email: args.email }; return cmd.endsWith("signup") ? "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ-2345-6789" : null;
    case "account_service_logout": mockState.sync = { ...(mockState.sync || {}), service_email: null }; return null;
    case "icloud_state": return { available: true, path: "~/Library/Mobile Documents/iCloud~com~fengli~beayy/Documents", vault: false };
    case "webdav_state": return { ok: true, vault: false };
    case "icloud_create": case "webdav_create": case "folder_create": case "icloud_join": case "webdav_join": case "folder_join": {
      const mode = cmd.split("_")[0];
      mockState.sync = { service_email: (mockState.sync || {}).service_email, configured: true, mode, folder: mode === "folder" ? "/Users/demo/OneDrive/Beayy" : null, webdav: mode === "webdav" ? args.url : null, last: new Date().toISOString() };
      return cmd.endsWith("_create") ? "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ-2345-6789" : null;
    }
    case "account_recovery_code": return "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ-2345-6789";
    case "get_week": {
      // 样例里有这一周就用（按周一取），没有就给空的一周
      const m = addDays(args.date, -((toDate(args.date).getDay() + 6) % 7));
      return (await loadMock("get_week"))[m] || { monday: m, sunday: addDays(m, 6), summary_md: null, error: null, days: Array.from({ length: 7 }, (_, k) => ({ date: addDays(m, k), line: null })) };
    }
    case "get_month": {
      const first = args.date.slice(0, 8) + "01", last = addDays(addDays(first, 32).slice(0, 8) + "01", -1);
      const got = (await loadMock("get_month"))[first.slice(0, 7)];
      if (got) return got;
      const weeks = [];
      for (let m = addDays(first, -((toDate(first).getDay() + 6) % 7)); m <= last; m = addDays(m, 7)) weeks.push({ monday: m, sunday: addDays(m, 6), line: null });
      return { first, last, summary_md: null, error: null, weeks };
    }
    case "get_range": {
      // 样例：用 get_day 样例里有的那几天拼，时间分类按天加起来
      const all = await loadMock("get_day"), days = [], sum = { domains: {}, activities: {} };
      let tagged = 0;
      for (let d = args.from; d <= args.to; d = addDays(d, 1)) {
        const x = all[d]; if (!x) continue;
        const c = x.live || (x.pack && x.pack.computer) || {}, ph = x.phone;
        days.push({ date: d, active_sec: c.active_sec ?? null, phone_sec: ph ? ph.total_sec : null, pickups: ph ? ph.pickups : null, night_sec: ph ? (ph.late_sec || 0) + (ph.night_sec || 0) : null, visits: (x.places && x.places.visits) || [] });
        if (!x.work) continue;
        tagged += x.work.tagged_sec || 0;
        for (const k of ["domains", "activities"]) for (const g of x.work[k]) for (const c2 of g.children) {
          const o = (sum[k][g.name] = sum[k][g.name] || {});
          o[c2.name] = (o[c2.name] || 0) + c2.sec;
        }
      }
      const tree = (o) => Object.entries(o).map(([name, ch]) => ({ name, sec: Object.values(ch).reduce((a, b) => a + b, 0), children: Object.entries(ch).map(([n, s]) => ({ name: n, sec: s })).sort((a, b) => b.sec - a.sec) })).sort((a, b) => b.sec - a.sec);
      return { from: args.from, to: args.to, days, work: { domains: tree(sum.domains), activities: tree(sum.activities), tagged_sec: tagged, untagged_sec: 0 } };
    }
    case "get_icons": {
      // 样例默认不带图标（显示首字母）；mock/get_icons.json 放了图标就用
      const all = await loadMock("get_icons").catch(() => null);
      return all ? Object.fromEntries(args.ids.filter((b) => all[b]).map((b) => [b, all[b]])) : {};
    }
    default: return loadMock(cmd);
  }
}

// ---------- 小工具 ----------

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const hh = (sec) => (sec == null ? "—" : (sec / 3600).toFixed(1));
const mins = (sec) => (sec == null ? "—" : String(Math.round(sec / 60)));
// 时间都按这台电脑的本地时区显示（以前写死了东八区）
const pad2 = (n) => String(n).padStart(2, "0");
const toDate = (d) => { const [y, m, dd] = d.split("-").map(Number); return new Date(y, m - 1, dd); };
const weekday = (d) => (LANG === "zh" ? "周" + "日一二三四五六"[toDate(d).getDay()] : ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][toDate(d).getDay()]);
const short = (d) => `${d.slice(5)} ${weekday(d)}`;
const localDay = (t = new Date()) => `${t.getFullYear()}-${pad2(t.getMonth() + 1)}-${pad2(t.getDate())}`;
const hm = (iso) => { if (!iso) return ""; const t = new Date(iso); return `${pad2(t.getHours())}:${pad2(t.getMinutes())}`; };
const mdhm = (iso) => { if (!iso) return ""; const t = new Date(iso); return `${pad2(t.getMonth() + 1)}-${pad2(t.getDate())} ${hm(iso)}`; };
/** 带完整年月日的时刻（墨线文案：「同步至 2026-10-22 09:27」） */
const ymdhm = (iso) => { if (!iso) return ""; const t = new Date(iso); return `${t.getFullYear()}-${pad2(t.getMonth() + 1)}-${pad2(t.getDate())} ${hm(iso)}`; };
/** 列表里的时刻：今天只写时刻，今年写月日，更早带年 */
const whenAt = (iso) => { if (!iso) return ""; const t = new Date(iso), now = new Date(); return localDay(t) === localDay(now) ? hm(iso) : t.getFullYear() === now.getFullYear() ? mdhm(iso) : ymdhm(iso); };
const addDays = (d, n) => localDay(new Date(toDate(d).getTime() + 12 * 3600e3 + n * 864e5));

/** 每小时秒数（电脑）：数据包的 active_hours {"09": 秒}，或老数据包的 hours {"09": {app: 秒}} */
function hourSecs(c) {
  c = c || {};
  const h = c.active_hours || Object.fromEntries(Object.entries(c.hours || {}).map(([k, v]) => [k, typeof v === "number" ? v : Object.values(v).reduce((a, b) => a + b, 0)]));
  return Array.from({ length: 24 }, (_, i) => h[String(i).padStart(2, "0")] || 0);
}

/** 区间 [[ISO, ISO]] 落在某天 hour 点以后的秒数 */
function secsAfter(spans, date, hour) {
  const from = toDate(date).getTime() + hour * 3600e3, to = toDate(date).getTime() + 864e5;
  return spans.reduce((s, [a, b]) => s + Math.max(0, Math.min(Date.parse(b), to) - Math.max(Date.parse(a), from)) / 1000, 0);
}

/** 每小时分钟数：接受数组（分钟）或 {"09": 秒} */
function hourMinutes(h) {
  if (Array.isArray(h)) return Array.from({ length: 24 }, (_, i) => h[i] || 0);
  return Array.from({ length: 24 }, (_, i) => Math.min(60, Math.round(((h || {})[String(i).padStart(2, "0")] || 0) / 60)));
}

const ICON = {
  chevL: '<svg class="mx-ico" viewBox="0 0 16 16"><path d="M10 3.5 5.5 8l4.5 4.5"/></svg>',
  chevR: '<svg class="mx-ico" viewBox="0 0 16 16"><path d="M6 3.5 10.5 8 6 12.5"/></svg>',
  chevD: '<svg class="mx-ico" viewBox="0 0 16 16"><path d="m4 6 4 4 4-4"/></svg>',
  check: '<svg class="mx-ico" viewBox="0 0 16 16"><path d="m3.5 8.5 3 3 6-7"/></svg>',
  close: '<svg class="mx-ico" viewBox="0 0 16 16"><path d="m4.5 4.5 7 7M11.5 4.5l-7 7"/></svg>',
  more: '<svg class="mx-ico" viewBox="0 0 16 16"><circle cx="3.5" cy="8" r="1" fill="currentColor" stroke="none"/><circle cx="8" cy="8" r="1" fill="currentColor" stroke="none"/><circle cx="12.5" cy="8" r="1" fill="currentColor" stroke="none"/></svg>',
};

// 极简 Markdown：小标题、列表、段落，行内粗体和代码。先转义再加标记。
const mdInline = (s) => esc(s).replace(/\*\*(.+?)\*\*/g, "<b>$1</b>").replace(/`([^`]+)`/g, "<code>$1</code>");
function md(text) {
  const inline = mdInline;
  const out = []; let list = null;
  const close = () => { if (list) { out.push(`</${list}>`); list = null; } };
  for (const raw of String(text || "").split("\n")) {
    const line = raw.trim();
    if (!line) { close(); continue; }
    let m;
    if ((m = line.match(/^#{1,6}\s+(.*)/))) { close(); out.push(`<h5>${inline(m[1])}</h5>`); }
    else if ((m = line.match(/^[-*]\s+(.*)/))) { if (list !== "ul") { close(); out.push("<ul>"); list = "ul"; } out.push(`<li>${inline(m[1])}</li>`); }
    else if ((m = line.match(/^\d+[.、]\s*(.*)/))) { if (list !== "ol") { close(); out.push("<ol>"); list = "ol"; } out.push(`<li>${inline(m[1])}</li>`); }
    else if (line.startsWith(">")) { close(); out.push(`<p class="note">${inline(line.replace(/^>\s*/, ""))}</p>`); }
    else { close(); out.push(`<p>${inline(line)}</p>`); }
  }
  close();
  return `<div class="md">${out.join("")}</div>`;
}

/** 总结的叙述段（日总结、周总结共用）：账本式排版，和上面的一句话、点评同一个路子——
 *  左边一窄列写段名（宋体）和几项（灰色小字），右边是条目，段与段之间一条细线；不用底色、图标。窄屏段名在上 */
function story(parts) {
  const row = (p) => {
    const items = p.body.split("\n").map((l) => l.trim().match(/^[-*]\s+(.*)/)).filter(Boolean).map((m) => m[1]);
    const body = items.length ? `<ul class="lg-list">${items.map((t) => `<li>${mdInline(trMsg(t))}</li>`).join("")}</ul>` : md(trMsg(p.body));
    return `<section class="lg-row"><div class="lg-k"><h2>${esc(tr(p.title))}</h2>${items.length ? `<span>${tr("{n} 项", { n: items.length })}</span>` : ""}</div>${body}</section>`;
  };
  return `<div class="story">${parts.map(row).join("")}</div>`;
}

/** 点评：开头引的那句（「……」（出处））单独一行、出处小字跟在后面，下面是点评正文（引号留给上面的一句话） */
function remarkHtml(text) {
  // 「原句」（出处）正文，出处可以只是人名；早先写的有的用 “”、少了前引号或没引号，那种出处要带《》才认，免得把普通括号当出处
  const t = String(text || "").trim();
  const m = t.match(/^「([^」]{2,80})」\s*[（(]([^）)]{1,30})[）)][，,。\s]*(.*)$/s)
    || t.match(/^[“"]?([^「」“”"（(]{2,40}?)[」”"]?\s*[（(]([^）)《]{0,12}《[^）)]{1,30})[）)][，,。\s]*(.*)$/s);
  if (!m) return `<blockquote class="remark"><p>${esc(text)}</p></blockquote>`;
  const [l, r] = LANG === "zh" ? ["「", "」"] : ["“", "”"];
  return `<blockquote class="remark"><p class="rm-quote">${l}${esc(m[1])}${r}<span class="rm-src">${esc(m[2])}</span></p>${m[3] ? `<p>${esc(m[3])}</p>` : ""}</blockquote>`;
}

/** 按 ## 切日总结：开头的引用块是口径说明 */
function splitSummary(text) {
  const parts = []; let note = []; let cur = null;
  for (const line of String(text || "").split("\n")) {
    if (line.startsWith("## ")) { cur = { title: line.slice(3).trim(), body: [] }; parts.push(cur); }
    else if (cur) cur.body.push(line);
    else if (line.startsWith(">")) note.push(line.replace(/^>\s*/, ""));
  }
  return { note: note.join(" "), parts: parts.map((p) => ({ title: p.title, body: p.body.join("\n").trim() })) };
}

// ---------- 页面骨架 ----------

function header(title, right = "") {
  return `<header class="mx-head"><div class="mx-head-row" data-tauri-drag-region><h1 class="mx-title">${esc(title)}</h1><div class="mx-head-right">${right}</div></div></header>`;
}
const section = (title, body, { count, aside } = {}) =>
  `<section class="mx-sec"><div class="mx-sec-head"><h2>${esc(title)}</h2>${count != null ? `<span class="mx-count">${count}</span>` : ""}${aside ? `<span class="mx-sec-aside">${aside}</span>` : ""}</div>${body}</section>`;
/** 设置里一页只有一组：不写标题（标题不重复页签名） */
const group = (body) => `<section class="mx-sec">${body}</section>`;
const emptyInline = (text) => `<p class="mx-empty-inline">${esc(text)}</p>`;
/** 设计系统的插画（vendor/moxian/illus.js，按名字取：day、notebook、map、sync、update、health、key）；sm＝弹层里的小号 */
const illus = (name, sm = false) => { const svg = (window.MX_ILLUS || {})[name] || ""; return sm ? svg.replace('class="mx-illus"', 'class="mx-illus mx-illus--sm"') : svg; };
/** 整块空状态：插画＋一句话＋可选的按钮（设计系统 Empty） */
const emptyBlock = (name, text, btn = "") => `<div class="mx-empty">${illus(name)}<div>${esc(text)}</div>${btn}</div>`;
/** 「怎么设置」链接（墨线文案：要分步骤的设置，界面一句话＋这个链接，步骤在弹层里） */
const howto = (kind, text) => `<button type="button" class="howto" data-howto="${kind}">${esc(text)}</button>`;
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-howto]"); if (!b) return;
  e.preventDefault(); e.stopPropagation();
  ({ screentime: screenTimeDialog, amap: amapHowtoDialog, bills: billHowtoDialog })[b.dataset.howto]?.();
}, true);

/** iCloud 屏幕使用时间怎么同步到这台 Mac：四步＋打开系统设置、现在读取 */
function screenTimeDialog() {
  // 手机版不读屏幕使用时间（电脑读），没有这两个按钮
  const desk = TAURI && !(S.status && S.status.mobile);
  openDialog({
    title: tr("iPhone 屏幕使用时间"), cancel: tr("关闭"), ok: desk ? tr("现在读取") : null, busy: tr("读取中…"),
    body: `<ol class="steps">
      <li>${tr("iPhone：设置 → 屏幕使用时间 → 打开「跨设备共享」")}</li>
      <li>${tr("Mac：系统设置 → 屏幕使用时间 → 打开「跨设备共享」")}</li>
      <li>${tr("两台用同一个 Apple ID")}</li>
      <li>${tr("想快点同步：在 iPhone 和 Mac 上各打开一次「屏幕使用时间」")}</li></ol>
      ${desk ? `<button class="mx-btn mx-btn--secondary mx-btn--sm" style="align-self:flex-start" id="st-open">${tr("打开系统设置")}</button>` : ""}`,
    onOpen: (el) => { const b = el.querySelector("#st-open"); if (b) b.onclick = () => call("open_permission", { kind: "screentime" }); },
    submit: async () => {
      await call("phone_read_now");
      await new Promise((r) => setTimeout(r, 4000));
      S.status = await call("get_status").catch(() => S.status);
      route();
    },
  });
}

/** 高德 Key 怎么获取 */
function amapHowtoDialog() {
  openDialog({
    title: tr("怎么获取高德 Key"), cancel: tr("关闭"), ok: null,
    body: `<ol class="steps">
      <li>${tr("打开 console.amap.com，用高德账号登录，完成开发者认证")}</li>
      <li>${tr("应用管理 → 创建新应用")}</li>
      <li>${tr("添加 Key，服务平台选「Web 服务」：这是 Web 服务 Key")}</li>
      <li>${tr("再添加一个 Key，服务平台选「Web端(JS API)」：这是地图 Key，它下面的安全密钥一起填")}</li></ol>
      <p class="note">${tr("不想填 Key，可以开通官方服务。")}</p>`,
  });
}

// ---------- 弹层（墨线 Dialog 小窗口）：新建、编辑、填几项再提交都在这里，页面上不展开编辑框 ----------
// 同一时间只开一个，挂在 body 上，页面重画不影响它。点遮罩、Esc、关闭都等于取消；提交中不关。

let DLG = null;

/** title、body（HTML）、ok（主按钮文字）、okClass（primary／danger）、busy（提交中的按钮文字）、cancel（false＝不要取消钮）、
 *  closable（false＝只能点主按钮关）、submit(el)：抛错就把错写在窗口里、不关，返回 false 也不关；onOpen(el, d) 绑窗口里的交互；
 *  onClose()：窗口关掉时（取消、Esc、点遮罩、换页、提交成功）调 */
function openDialog({ title, body, ok, okClass = "primary", busy, cancel = tr("取消"), closable = true, submit, onOpen, onClose }) {
  closeDialog();
  const root = document.createElement("div");
  root.className = "mx-scrim dlg-root";
  root.innerHTML = `<div class="mx-dialog mx-dialog--sm" role="dialog" aria-modal="true" aria-labelledby="dlg-title">
    <div class="mx-dialog-head"><h2 id="dlg-title">${esc(title)}</h2>${closable ? `<button class="mx-iconbtn" data-dlg="close" aria-label="${tr("关闭")}">${ICON.close}</button>` : ""}</div>
    <div class="mx-dialog-body">${body}<div class="mx-field-error" data-dlg="err" hidden></div></div>
    <div class="mx-dialog-foot">${cancel ? `<button class="mx-btn mx-btn--secondary" data-dlg="close">${esc(cancel)}</button>` : ""}${ok ? `<button class="mx-btn mx-btn--${okClass}" data-dlg="ok">${esc(ok)}</button>` : ""}</div></div>`;
  document.body.appendChild(root);
  const el = root.querySelector(".mx-dialog"), okBtn = root.querySelector('[data-dlg="ok"]'), err = root.querySelector('[data-dlg="err"]');
  const d = (DLG = { root, el, closable, busy: false, back: document.activeElement, onClose });
  d.setErr = (m) => { err.hidden = !m; err.textContent = m || ""; };
  d.setOk = (text) => { if (okBtn) okBtn.textContent = text; };
  d.go = async () => {
    if (d.busy || !okBtn || okBtn.disabled) return;
    const label = okBtn.textContent;
    d.busy = true; okBtn.disabled = true; d.setErr(null); if (busy) okBtn.textContent = busy;
    try { if ((await submit(el, d)) !== false) { d.busy = false; return closeDialog(); } }
    catch (x) { d.setErr(String(x && x.message ? x.message : x)); }
    d.busy = false; okBtn.disabled = false; okBtn.textContent = label;
  };
  const cancelable = () => closable && !d.busy;
  root.addEventListener("mousedown", (e) => { if (e.target === root && cancelable()) closeDialog(); });
  root.addEventListener("click", (e) => { const b = e.target.closest("[data-dlg]"); if (!b) return; if (b.dataset.dlg === "close" && cancelable()) closeDialog(); else if (b.dataset.dlg === "ok") d.go(); });
  root.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") { if (cancelable()) closeDialog(); return; }
    // 单行输入框里回车、任何地方 ⌘↩ 都等于点主按钮；输入法选字时的回车不算
    if (e.key === "Enter" && !e.isComposing && (e.metaKey || e.target.tagName === "INPUT")) { e.preventDefault(); d.go(); return; }
    if (e.key === "Tab") {
      const fs = [...el.querySelectorAll("button:not(:disabled), input:not([hidden]), textarea, [tabindex='0']")];
      if (!fs.length) return;
      const i = fs.indexOf(document.activeElement);
      if (e.shiftKey && i <= 0) { e.preventDefault(); fs[fs.length - 1].focus(); }
      else if (!e.shiftKey && i === fs.length - 1) { e.preventDefault(); fs[0].focus(); }
    }
  });
  if (onOpen) onOpen(el, d);
  const first = el.querySelector(".mx-dialog-body input:not([hidden]), .mx-dialog-body textarea") || okBtn || el.querySelector("[data-dlg]");
  if (first) { first.focus(); if (first.setSelectionRange && first.value) first.setSelectionRange(first.value.length, first.value.length); }
  return d;
}

function closeDialog() {
  const d = DLG; if (!d) return;
  DLG = null; d.root.remove();
  if (d.onClose) d.onClose();
  if (d.back && d.back.isConnected && d.back.focus) d.back.focus();
}

/** 选一个再做的弹层（暂停多久、删多少）：一排分段选项＋一句提醒＋主按钮，照设计系统「一个决定一个控件」 */
function choiceDialog({ title, options, note, ok, okClass = "primary", go }) {
  let pick = options[0][0];
  openDialog({
    title, ok, okClass,
    body: `<div class="mx-seg" role="tablist" id="ch-seg">${options.map(([k, v], i) => `<button role="tab" aria-selected="${!i}" class="${i ? "" : "is-on"}" data-v="${k}">${v}</button>`).join("")}</div>${note ? `<p class="note" style="margin:0">${esc(note)}</p>` : ""}`,
    onOpen: (el) => {
      el.querySelector("#ch-seg").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        pick = b.dataset.v;
        el.querySelectorAll("#ch-seg button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
      };
    },
    submit: () => go(pick),
  });
}

/** 弹层里的一项输入 */
const dlgField = (id, label, { type = "text", ph = "", value = "", mono = false, help = "" } = {}) =>
  `<div class="mx-field"><label for="${id}">${label}</label><input class="mx-input${mono ? " mono" : ""}" id="${id}" type="${type}" autocomplete="off" placeholder="${esc(ph)}" value="${esc(value)}">${help ? `<p class="note">${help}</p>` : ""}</div>`;
const dlgVal = (el, id) => { const x = el.querySelector("#" + id); return x ? x.value.trim() : ""; };

// ---------- 人生记录：读数、一天的形状 ----------

const shade = (min, v) => (min ? `background:color-mix(in srgb, var(${v}) ${Math.round(30 + (Math.min(min, 60) / 60) * 70)}%, var(--rec-track))` : "");

/** 读数：两行，标签和数值（单位跟在后面）。caption（时段、同步时刻这类补充）和 tip 都放悬停里，不占第三行
 *  （三行太累赘；墨线：读数不配解释文字，要解释放悬停） */
function readout(k, v, unit, caption = "", over = false, tip = "") {
  const hint = [caption, tip].filter(Boolean).join("\n");
  // 没有数（「—」）不跟单位：「— h」像坏了；读屏念「没有数据」
  const none = v === "—" || v == null;
  return `<div class="ro${over ? " over" : ""}${none ? " none" : ""}"${hint ? ` title="${esc(hint)}"` : ""}><div class="label">${esc(k)}</div><div class="v">${none ? `<span aria-hidden="true">—</span><span class="sr-only">${tr("没有数据")}</span>` : `${esc(v)}${unit ? `<small>${esc(unit)}</small>` : ""}`}</div></div>`;
}

/** 一天的形状。只给电脑、手机两排时照旧（上排电脑、下排手机）；给了 opt（手表说话的分钟数 hw、停留点 visits、日期 date）
 *  就按设备一排一排、行首写设备名，最上面一排地点。电脑、手机、说话每小时一格占满，用得越久越深；
 *  地点是一整条，每段停留按起止画、颜色跟着地方走，够宽的写地名。今天还没到的小时格只留空框；
 *  轴上每 3 小时写一个钟点（03:00） */
function dayStrip(hc, hp, clickable = false, opt = null) {
  const day0 = opt && opt.date ? toDate(opt.date).getTime() : 0, now = Date.now(), today = opt && now >= day0 && now < day0 + 864e5;
  const futH = today ? new Date(now).getHours() : 24;
  // 能点时每格、每段都是按钮：Tab 只停在整条上一次，方向键换格，回车看那个小时的时间线（bindStripKeys）
  const act = (tip) => (clickable ? ` role="button" tabindex="-1" aria-label="${tr("{tip}，看这个小时的时间线", { tip: esc(tip) })}"` : "");
  const cells = (arr, v, name, unit = tr("分钟")) => arr.map((m, h) => `<span${h > futH ? ' class="fut"' : ""} style="${shade(m, v)}" title="${h}:00 ${name} ${m} ${unit}"${clickable ? ` data-h="${h}"` : ""}${act(`${h}:00 ${name} ${m} ${unit}`)}></span>`).join("");
  const wrap = (label, body) => (clickable ? `<div class="strip-wrap" role="toolbar" aria-label="${tr("{label}。方向键换格，回车看那个小时的时间线", { label })}">${body}</div>` : `<div role="img" aria-label="${label}">${body}</div>`);
  const axis = (n) => `<div class="strip-axis" aria-hidden="true">${Array.from({ length: n }, (_, h) => (h % 3 ? "<span></span>" : `<span class="t">${pad2(h)}<small>:00</small></span>`)).join("")}</div>`;
  if (!opt) return wrap(tr("一天的形状：上排电脑，下排手机"), `<div class="strip">${cells(hc, "--rec-computer", tr("电脑"))}${cells(hp, "--rec-phone", tr("手机"))}</div>${axis(24)}`);
  const frac = (ms) => Math.max(0, Math.min(1, (ms - day0) / 864e5));
  // 地点：一整条，每段停留按起止画，颜色跟着地方走（同 placeTones，和下面「地点」列表一个颜色）；
  // 够宽的写地名，窄的只在悬停里有；两段之间的路上是灰色块
  const placeRow = () => {
    const tones = placeTones(opt.visits, opt.date);
    const pos = (a, z) => `left:${(frac(a) * 100).toFixed(3)}%;width:${(Math.max(frac(z) - frac(a), 0.002) * 100).toFixed(3)}%`;
    const bars = placeBlocks(opt.visits, opt.date).map((b) => {
      if (b.kind === "go") return `<i class="go" style="${pos(b.a, b.z)}" title="${tr("路上")} ${hmMs(b.a)}–${hmMs(b.z)} ${spanText((b.z - b.a) / 1000)}"${clickable ? ` data-h="${new Date(Math.max(b.a, day0)).getHours()}"` : ""}${act(`${tr("路上")} ${hmMs(b.a)}–${hmMs(b.z)} ${spanText((b.z - b.a) / 1000)}`)}>${(b.z - b.a) / 3600e3 >= 1.2 ? tr("路上") : ""}</i>`;
      if (b.kind !== "stay") return "";
      const name = placeName(b.v), wide = (b.z - b.a) / 3600e3 >= 1.2;
      return `<i style="${pos(b.a, b.z)};background:var(--pl-${tones.get(placeKey(b.v))});--edge:var(--pl-${tones.get(placeKey(b.v))}-s)" title="${esc(name)} ${visitSpan(b.v)}"${clickable ? ` data-h="${new Date(Math.max(b.a, day0)).getHours()}"` : ""}${act(`${name} ${visitSpan(b.v)}`)}>${wide ? esc(name) : ""}</i>`;
    }).join("");
    return `<div class="place-bar">${bars}</div>`;
  };
  // 睡眠：苹果健康（多是手表记的，颜色跟手表），深睡最深、核心次之、快速眼动浅，醒着留空
  const sleepRow = () => `<div class="place-bar sleep-bar">${opt.sleep.map((b) => {
    const a = Date.parse(b.start), z = Date.parse(b.end), k = SLEEP_STAGE[b.stage] || SLEEP_STAGE.asleep;
    return `<i style="left:${(frac(a) * 100).toFixed(3)}%;width:${(Math.max(frac(z) - frac(a), 0.002) * 100).toFixed(3)}%;${k[1] ? `background:color-mix(in srgb, var(--rec-watch) ${k[1]}%, var(--rec-track))` : "background:transparent"}" title="${k[0]} ${hm(b.start)}–${hm(b.end)}"${clickable ? ` data-h="${new Date(Math.max(a, day0)).getHours()}"` : ""}${act(`${k[0]} ${hm(b.start)}–${hm(b.end)}`)}></i>`;
  }).join("")}</div>`;
  const rows = [];
  if (opt.visits && opt.visits.length) rows.push([tr("地点"), placeRow()]);
  if (opt.sleep && opt.sleep.length) rows.push([tr("睡眠"), sleepRow()]);
  rows.push([tr("电脑"), `<div class="strip">${cells(hc, "--rec-computer", tr("电脑"))}</div>`]);
  rows.push([tr("手机"), `<div class="strip">${cells(hp, "--rec-phone", tr("手机"))}</div>`]);
  // 说话：手表录的，和没戴手表时 iPhone 自己录的，合在一排
  if (opt.hw) rows.push([tr("说话"), `<div class="strip">${cells(opt.hw, "--rec-watch", tr("说话"))}</div>`]);
  return wrap(tr("一天的形状：{rows}各一排", { rows: rows.map((r) => r[0]).join(tr("、")) }), `<div class="strip-rows">${rows.map(([k, v, c]) => `<span class="k${c ? " " + c : ""}">${k}</span>${v}`).join("")}<span></span>${axis(24)}</div>`);
}

/** 一天的形状能点时的键盘：整条只占一个 Tab 停靠点（停在上次的格子），←→ 换格、↑↓ 跳到上下一排的同一时刻、Home/End 到头尾，回车或空格＝点 */
function bindStripKeys(root, go) {
  for (const box of root.querySelectorAll(".strip-wrap")) {
    const items = [...box.querySelectorAll("[data-h][role=button]")];
    if (!items.length) continue;
    items[0].tabIndex = 0;
    const rows = [...new Set(items.map((x) => x.parentElement))];
    const move = (to) => { if (!to) return; items.forEach((x) => (x.tabIndex = -1)); to.tabIndex = 0; to.focus(); };
    box.onkeydown = (e) => {
      const i = items.indexOf(document.activeElement); if (i < 0) return;
      const cur = items[i], k = e.key;
      if (k === "Enter" || k === " ") { e.preventDefault(); go(+cur.dataset.h); return; }
      let to = null;
      if (k === "ArrowRight") to = items[i + 1];
      else if (k === "ArrowLeft") to = items[i - 1];
      else if (k === "Home") to = items[0];
      else if (k === "End") to = items[items.length - 1];
      else if (k === "ArrowDown" || k === "ArrowUp") {
        const r = rows[rows.indexOf(cur.parentElement) + (k === "ArrowDown" ? 1 : -1)];
        if (r) { const xs = items.filter((x) => x.parentElement === r); to = xs.find((x) => +x.dataset.h >= +cur.dataset.h) || xs[xs.length - 1]; }
      } else return;
      e.preventDefault(); move(to);
    };
  }
}

// ---------- 苹果健康（iPhone 传来，电脑汇总：health.rs） ----------

/** 睡眠分期：名字、在一天的形状里的深浅（手表色的百分比，0＝留空） */
const SLEEP_STAGE = { deep: [tr("深睡"), 100], core: [tr("核心睡眠"), 70], asleep: [tr("睡着"), 70], rem: [tr("快速眼动"), 45], awake: [tr("醒着"), 0] };
const HEALTH_UNIT = { "count": "", "count/min": tr("次/分"), "kcal": tr("千卡"), "m": tr("米"), "min": tr("分钟"), "s": tr("秒"), "ms": tr("毫秒"), "kg": tr("公斤"), "g": tr("克"), "mg": tr("毫克"),
  "degC": "°C", "dBASPL": tr("分贝"), "dBHL": tr("分贝"), "mmHg": tr("毫米汞柱"), "L": tr("升"), "mL": tr("毫升"), "%": "%" };
const healthDur = (sec) => (sec == null ? "" : sec < 3600 ? tr("{n} 分钟", { n: Math.round(sec / 60) }) : tr("{h} 小时", { h: hh(sec) }));
const healthNum = (v, unit) => { if (v == null) return "—"; const x = unit === "%" ? v * 100 : v; return Math.abs(x) >= 100 ? numText(Math.round(x)) : String(Math.round(x * 10) / 10); };
/** 心情的愉快程度（-1 到 1），按苹果的七档 */
const valenceText = (v) => [tr("非常不愉快"), tr("不愉快"), tr("有点不愉快"), tr("中性"), tr("有点愉快"), tr("愉快"), tr("非常愉快")][Math.min(6, Math.max(0, Math.floor(((v || 0) + 1) / (2 / 7))))];

/** 「健康」：昨晚睡眠、活动、心率、锻炼、心情；其他类型收在「其他 N 项」里 */
function healthSection(h) {
  if (!h) return "";
  const s = h.sleep, a = h.activity || {}, hr = h.heart || {}, rows = [];
  if (s) rows.push([tr("睡眠"), tr("{a}–{b}，睡着 {d}", { a: hm(s.start), b: hm(s.end), d: healthDur(s.asleep_sec) })
    + (s.staged ? tr("（深睡 {deep}、核心 {core}、快速眼动 {rem}{awake}）", { deep: healthDur(s.deep_sec), core: healthDur(s.core_sec), rem: healthDur(s.rem_sec), awake: s.awake_sec ? tr("，醒着 {d}", { d: healthDur(s.awake_sec) }) : "" }) : "")
    + (s.inbed_sec > s.asleep_sec + 600 ? ` · ${tr("在床上 {d}", { d: healthDur(s.inbed_sec) })}` : "")]);
  const act = [a.steps != null && tr("{n} 步", { n: numText(a.steps) }), a.distance_m && tr("{n} 公里", { n: (a.distance_m / 1000).toFixed(1) }), a.active_kcal != null && tr("活动 {n} 千卡", { n: numText(a.active_kcal) }),
    a.exercise_min != null && tr("锻炼 {n} 分钟", { n: a.exercise_min }), a.stand_hours != null && tr("站立 {n} 小时", { n: a.stand_hours }), a.flights && tr("爬了 {n} 层", { n: a.flights })].filter(Boolean);
  if (act.length) rows.push([tr("活动"), act.join(" · ")]);
  const heart = [hr.n && tr("平均 {avg}，{min}–{max}", { avg: hr.avg, min: hr.min, max: hr.max }), hr.resting != null && tr("静息 {n}", { n: hr.resting }), hr.hrv_ms != null && tr("心率变异性 {n} 毫秒", { n: hr.hrv_ms })].filter(Boolean);
  if (heart.length) rows.push([tr("心率"), heart.join(" · ")]);
  for (const w of h.workouts || []) rows.push([tr("锻炼"), `${hm(w.start)}–${hm(w.end)} ${esc(trMsg(w.name))} ${healthDur(w.sec)}`
    + [w.distance_m && ` · ${tr("{n} 公里", { n: (w.distance_m / 1000).toFixed(1) })}`, w.kcal && ` · ${tr("{n} 千卡", { n: w.kcal })}`, w.avg_hr && ` · ${tr("平均心率 {n}", { n: w.avg_hr })}`].filter(Boolean).join("")]);
  for (const m of h.moods || []) rows.push([tr("心情"), `${hm(m.at)} ${valenceText(m.valence)}`]);
  if (h.mindful_sec) rows.push([tr("正念"), healthDur(h.mindful_sec)]);
  const kv = (rs, cls = "kv") => `<dl class="${cls}">${rs.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join("")}</dl>`;
  const other = (h.others || []).map((o) => [esc(trMsg(o.name)), o.agg === "count" ? tr("{n} 次", { n: o.n })
    : `${o.agg === "sum" ? tr("共 ") : o.n > 1 ? tr("平均 ") : ""}${healthNum(o.value, o.unit)}${o.min != null ? tr("（{a}–{b}）", { a: healthNum(o.min, o.unit), b: healthNum(o.max, o.unit) }) : ""}${o.unit === "%" ? "%" : " " + esc(HEALTH_UNIT[o.unit] ?? o.unit ?? "")}`]);
  return section(tr("健康"), (rows.length ? kv(rows) : "") + (other.length ? `<details class="adv"><summary>${tr("其他 {n} 项", { n: other.length })}</summary>${kv(other, "kv kv-wide")}</details>` : ""));
}

/** 「最近」：最近 14 天（照 Wise 支出洞察），每天一组两根柱子（电脑、手机），看的这一天实色、其他天淡；点哪天去哪天；
 *  不配平均之类的说明（不加非必要的注释）。范围到今天为止，看的这一天太早就以它为最后一天；没有记录的天不画柱子 */
function recentDays(date, cur) {
  const by = Object.fromEntries(S.days.map((x) => [x.date, x]));
  by[date] = { ...(by[date] || {}), date, active_sec: cur.c ?? (by[date] || {}).active_sec, phone_sec: cur.p ?? (by[date] || {}).phone_sec };
  const now = localDay(), end = date >= addDays(now, -13) ? now : date;
  const ds = Array.from({ length: 14 }, (_, i) => addDays(end, i - 13));
  const has = (x) => by[x] && (by[x].active_sec != null || by[x].phone_sec != null);
  const max = Math.max(3600, ...ds.filter(has).flatMap((x) => [by[x].active_sec || 0, by[x].phone_sec || 0]));
  const bar = (sec, cls) => `<i class="${cls}" style="height:${sec ? Math.max(2, Math.round((sec / max) * 100)) : 0}%"></i>`;
  const cols = ds.map((x) => {
    const v = by[x], ok = has(x), tip = ok ? tr("{d} · 电脑 {c}h · 手机 {p}h", { d: short(x), c: hh(v.active_sec), p: hh(v.phone_sec) }) : tr("{d} · 取不到", { d: short(x) });
    return `<button class="rd-col${x === date ? " is-cur" : ""}${ok ? "" : " none"}" ${ok || x === now ? `data-day="${x}"` : "disabled"} title="${esc(tip)}" aria-label="${esc(tip)}"${x === date ? ' aria-current="date"' : ""}><span class="rd-bars">${ok ? bar(v.active_sec, "c") + bar(v.phone_sec, "p") : ""}</span><span class="rd-d mono">${x === now && LANG === "zh" ? "今" : +x.slice(8)}</span></button>`;
  }).join("");
  return section(tr("最近"), `<div class="rd" role="group" aria-label="${tr("最近 14 天每天在电脑前和用手机的时长")}">${cols}</div>`);
}

/** 地点的颜色：一共 6 种（--pl-1…6），按地名的哈希固定分配，同一个地方每天同一个颜色；
 *  同一天里撞色的往后挪一格（待得久的先挑）。一天的形状和「地点」列表共用 */
const PLACE_TONES = 6;
const placeKey = (v) => v.label || placeName(v).split(" · ")[0];
const hashStr = (s) => [...s].reduce((h, c) => (h * 31 + c.codePointAt(0)) >>> 0, 7);
function placeTones(visits, date) {
  const tot = new Map();
  for (const b of placeBlocks(visits, date)) if (b.kind === "stay") tot.set(placeKey(b.v), (tot.get(placeKey(b.v)) || 0) + (b.z - b.a));
  return toneMap(tot);
}

/** 地点 → 颜色序号：待得久的先挑（按地名哈希，撞了往后挪） */
function toneMap(tot) {
  const used = new Set(), out = new Map();
  for (const [k] of [...tot].sort((a, b) => b[1] - a[1])) {
    let t = hashStr(k) % PLACE_TONES;
    for (let i = 0; i < PLACE_TONES && used.has(t); i++) t = (t + 1) % PLACE_TONES;
    used.add(t);
    out.set(k, t + 1);
  }
  return out;
}

/** 两个停留点之间不超过 3 小时算「路上」，更长的和首尾算「没记到」（同 脚本/日数据包.py 的 TRANSIT_MAX） */
const TRANSIT_MAX = 3 * 3600e3;
/** 地点的显示名：起过名的用名字；没起名的不带区名，只有区域时写区域，确认了是哪家店写「区域 · 店」
 *  （手机用高德取的地名是「区 · 区域」，确认了店是「区 · 区域 · 店」；10-21 前苹果取的是「区 · 区域 · 门牌或店」） */
const placeName = (v) => {
  if (!v) return tr("没有地名");
  if (v.label) return v.label;
  const parts = (v.name || "").split(" · ");
  const n = (parts.length > 1 ? parts.slice(1).join(" · ") : parts[0]) || tr("没有地名");
  // 和家不在一个城市：前面写城市（「上海 · 静安寺」）
  return v.city_prefix ? `${String(v.city_prefix).replace(/市$/, "")} · ${n}` : n;
};
/** 停留按天切开（跨天来的 start 是当天 0 点），还没走的只算到现在、最多到当天结束：翻看过去的日子不会出现超过 24 小时 */
const dayEndOf = (v) => { const d = new Date(v.start); d.setHours(24, 0, 0, 0); return d.getTime(); };
const ongoing = (v) => !v.end && Date.now() < dayEndOf(v);
const visitEnd = (v) => (v.end ? Date.parse(v.end) : Math.min(Date.now(), dayEndOf(v)));
const pctText = (p) => `${Math.max(1, Math.round(p * 100))}%`;
/** 店名去掉括号里的分店名：「星巴克(某某广场店)」→「星巴克」（同手机） */
const shopShort = (n) => String(n || "").replace(/^(.+?)\s*[（(][^（()）]*[)）]$/, "$1").trim();
/** 还没确认是哪家店时，名字下面一行：在做什么（高德大类，置信度加总，手机算的）和最像的两家 */
function guessLine(v) {
  if (v.label || v.confirmed || !(v.guesses || []).length) return "";
  const parts = [];
  if (v.doing) parts.push(`${v.doing.kind} ${pctText(v.doing.p)}`);
  parts.push(tr("可能是 {name}", { name: v.guesses.slice(0, 2).map((g) => `${shopShort(g.name)} ${pctText(g.p)}`).join(tr("、")) }));
  return parts.join(" · ");
}
/** 区域名（常去按它合计）：电脑取的分级里的区域，没有就用地名的第二层 */
const areaOf = (v) => (v.admin && v.admin.area) || ((v.place_name || v.name || "").split(" · ")[1]) || (v.place_name || v.name || "").split(" · ")[0];
/** 完整分级「中国 › 浙江省 › 杭州市 › 西湖区 › 北山街道 › 万象广场」（直辖市省市同名只写一次） */
function chainOf(v) {
  const a = v.admin || {};
  const xs = [a.country, a.province, a.city !== a.province ? a.city : null, a.district, a.township, a.area].filter(Boolean);
  return xs.join(" › ");
}
const cityShort = (c) => String(c || "").replace(/市$/, "");
/** 角标：起过名的写种类（家、工作、其他），没起名但最近 30 天去过 5 次以上的写「常去」，第一次去的不标 */
function badgeOf(v) {
  if (v.label) return PL_ICON[v.type] || "其他";
  const f = PL.stats && PL.stats.frequent;
  return f && f.some((r) => r.name === areaOf(v)) ? "常去" : "";
}
const badgeHtml = (v) => { const b = badgeOf(v); return b ? `<span class="pl-badge${b === "常去" ? " often" : ""}">${esc(tr(b))}</span>` : ""; };

/** 地图上停留点的编号：1–9、A–Z（同高德静态地图的标注） */
const MARKS = "123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const hmMs = (ms) => hm(new Date(ms).toISOString());
/** 停留的起止：从前一天待过来的写真实到达时刻（「昨天 22:10」），还没走的写「现在」，那天已经过完的写到「24:00」 */
function visitSpan(v) {
  let a = hm(v.start);
  if (!v.arrive_known && v.arrive) {
    const t = new Date(v.arrive), y = new Date(Date.parse(v.start) - 864e5);
    a = (localDay(t) === localDay(y) ? tr("昨天") + " " : `${pad2(t.getMonth() + 1)}-${pad2(t.getDate())} `) + hm(v.arrive);
  }
  return `${a}–${v.end ? hm(v.end) : ongoing(v) ? tr("现在") : "24:00"}`;
}
const spanText = (sec) => (sec < 3600 ? tr("{n} 分钟", { n: Math.max(1, Math.round(sec / 60)) }) : `${hh(sec)}h`);

/** 把一天按停留点切开：停留、两段之间的路上（≤ 3 小时）、没记到（更长的空档和首尾），按时间先后。只算到现在 */
function placeBlocks(visits, date) {
  const day0 = toDate(date).getTime(), stop = Math.min(Date.now(), day0 + 864e5);
  const vs = [...visits].sort((a, b) => Date.parse(a.start) - Date.parse(b.start));
  const out = [];
  let cur = day0;
  for (const v of vs) {
    const a = Date.parse(v.start), z = Math.min(visitEnd(v), stop);
    if (a > cur) out.push({ kind: cur > day0 && a - cur <= TRANSIT_MAX ? "go" : "none", a: cur, z: a });
    out.push({ kind: "stay", v, a, z });
    cur = Math.max(cur, z);
  }
  if (stop > cur) out.push({ kind: "none", a: cur, z: stop });
  return out.filter((b) => b.z > b.a);
}

/** 某段时间里待得最久的停留点 */
function placeAt(visits, a, z) {
  let best = null, most = 0;
  for (const v of visits || []) {
    const o = Math.min(z, visitEnd(v)) - Math.max(a, Date.parse(v.start));
    if (o > most) { most = o; best = v; }
  }
  return best;
}

/** 说话的时长：不到一小时写分钟 */
const talkText = (sec) => (sec < 3600 ? tr("{n} 分钟", { n: Math.max(1, Math.round(sec / 60)) }) : `${hh(sec)}h`);
/** 说话的人：按说话时长排，本人排第一 */
function speakers(talk, self) {
  return [...talk].sort((a, b) => (b.speaker === self) - (a.speaker === self) || b.sec - a.sec).map((t) => t.speaker);
}

/** 概览里的「去了哪」（照地图类产品的「时间线」）：上面一行汇总；左边按时间的竖线时间线（停留和路上交替，停留里套着那时的对话），
 *  右边地图（停留点按先后编号，点哪一行地图上那个点变成强调色）。名字只写有把握的；还没认出是哪家店的，下面一行列最像的几家，
 *  点一下就认定；每行右边「⋯」改地点（候选、附近、搜索、直接输入、起名）。不显示百分比（悬停能看） */
function placesSection(visits, date, convs, mode = "stays") {
  if (!visits.length) return "";
  const blocks = placeBlocks(visits, date);
  let go = 0;
  for (const b of blocks) if (b.kind === "go") go += (b.z - b.a) / 1000;
  const spots = new Set(visits.map((v) => v.label || placeName(v).split(" · ")[0])).size;
  const talk = (convs || []).length;
  // 当天在哪些城市、区：一个城市写「杭州 · 西湖区、滨江区」，跨城写「杭州 → 上海」
  const cities = [...new Set(visits.map((v) => v.admin && v.admin.city).filter(Boolean))];
  const where = cities.length > 1 ? cities.map(cityShort).join(" → ")
    : cities.length ? `${cityShort(cities[0])} · ${[...new Set(visits.map((v) => v.admin && v.admin.district).filter(Boolean))].join(tr("、"))}` : "";
  const sum = [where, tr("{n} 个地方", { n: spots }), tr("{n} 段停留", { n: visits.length }), go ? tr("路上 {d}", { d: spanText(go) }) : "", talk ? tr("说话 {n} 段", { n: talk }) : ""].filter(Boolean).join(" · ");
  const idx = new Map(visits.map((v, i) => [v, i]));
  let first = true;
  const rows = blocks.map((b) => {
    if (b.kind === "stay") { first = false; return stayItem(b.v, idx.get(b.v), (convs || []).filter((c) => Date.parse(c.start) >= b.a && Date.parse(c.start) < b.z)); }
    if (first) return "";  // 一天开头没记到的不画
    const dur = spanText((b.z - b.a) / 1000);
    return b.kind === "go" ? `<li class="ptl-go"><span class="ptl-t"></span><span class="ptl-dot"></span><span class="ptl-b">${tr("路上 {d}", { d: dur })}</span></li>`
      : `<li class="ptl-none"><span class="ptl-t"></span><span class="ptl-dot"></span><span class="ptl-b">${tr("没记到 {d}", { d: dur })}</span></li>`;
  }).join("");
  if (mode === "map") {
    // 地图：左边一列简短的停留（编号、名字、起止），点哪段地图上那个点变成强调色；右边大地图
    const list = visits.map((v, i) => `<li><button class="pm-row" data-sel="${i}"><span class="ptl-dot">${MARKS[Math.min(i, MARKS.length - 1)]}</span><span class="pm-b"><span class="pm-n" title="${esc(chainOf(v))}">${esc(placeName(v))}${badgeHtml(v)}</span>${!v.label && !v.confirmed && (v.guesses || [])[0] ? `<span class="pm-m">${tr("可能是 {name}", { name: esc(shopShort(v.guesses[0].name)) })} ${pctText(v.guesses[0].p)}</span>` : ["auto", "remembered"].includes(v.confirmed) && (v.guesses || [])[0] ? `<span class="pm-m">${v.confirmed === "auto" ? tr("自动认出") : tr("按上次认定")} ${pctText(v.guesses[0].p)}</span>` : ""}<span class="pm-m">${visitSpan(v)} · ${spanText((visitEnd(v) - Date.parse(v.start)) / 1000)}</span></span></button></li>`).join("");
    return `<p class="ptl-sum">${sum}</p><div class="pm-wrap"><ol class="pm-list">${list}</ol>
      <div class="pm-map"><div class="pl-live" id="pl-live"><p class="note">${tr("在打开地图…")}</p></div><p class="note">${tr("地图是{who}的，能拖、能缩放。只画停留的地方，虚线只表示先后；认定是哪家、改地点在「停留」里。", { who: S.settings && S.settings.map_provider === "osm" ? "OpenStreetMap" : tr("高德") })}</p></div></div>`;
  }
  return `<p class="ptl-sum">${sum}</p><ol class="ptl" id="ptl">${rows}</ol>`;
}

/** 地点 · 常去：最近 30 天按地方合计。起过名的（家、公司……，可改名字、地址、种类、范围、删），常去但没起名的（去过 5 次以上，可起名），
 *  底下「添加一个地方」搜着加。不跟着翻日期，总是最近 30 天 */
function oftenView(st) {
  if (!st) return emptyInline(tr("没取到常去的地方。"));
  const md = (d) => (d ? d.slice(5) : "");
  const meta = (r) => [r.count ? tr("{n} 次", { n: r.count }) : tr("最近没来过"), r.sec ? tr("共 {d}", { d: spanText(r.sec) }) : "", r.last ? tr("最近 {d}", { d: md(r.last) }) : ""].filter(Boolean).join(" · ");
  // 按「城市 › 区县」分组；起过名但最近没去过的（不知道在哪个区）放最后一组
  const groups = new Map();
  const put = (k, html, sec) => { if (!groups.has(k)) groups.set(k, { rows: [], sec: 0 }); const g = groups.get(k); g.rows.push(html); g.sec += sec; };
  for (const r of st.named) {
    const k = r.city || r.district ? [cityShort(r.city), r.district].filter(Boolean).join(" › ") : "最近没去过";
    put(k, `<li class="pf-row"><span class="pf-k">${esc(tr(PL_ICON[r.type] || "其他"))}</span>
      <span class="pf-b"><span class="pf-n">${esc(r.name)}${r.address ? `<span class="pf-a"> · ${esc(r.address)}</span>` : ""}</span><span class="pf-m">${esc(meta(r))} · ${tr("范围 {n} 米", { n: Math.round(r.r || 150) })}</span></span>
      <button class="mx-btn mx-btn--text mx-btn--sm" data-pl="${r.index}">${tr("改")}</button></li>`, r.sec || 0);
  }
  st.frequent.forEach((r, i) => put([cityShort(r.city), r.district].filter(Boolean).join(" › ") || "其他", `<li class="pf-row"><span class="pf-k often">${tr("常去")}</span>
      <span class="pf-b"><span class="pf-n">${esc(r.name)}</span><span class="pf-m">${esc(meta(r))} · ${tr("还没起名")}</span></span>
      <button class="mx-btn mx-btn--text mx-btn--sm" data-name-it="${i}">${tr("起名")}</button></li>`, r.sec || 0));
  const order = [...groups].sort((a, b) => (a[0] === "最近没去过") - (b[0] === "最近没去过") || b[1].sec - a[1].sec);
  return `<p class="ptl-sum">${tr("最近 {days} 天 · 起过名的 {named} 个 · 常去没起名的 {freq} 个", { days: st.days, named: st.named.length, freq: st.frequent.length })}</p>
    ${order.map(([k, g]) => section(tr(k), `<ol class="pf-list">${g.rows.join("")}</ol>`)).join("") || emptyInline(tr("还没有常去的地方"))}
    <p class="note">${tr("按城市 › 区分组。没起名的列最近 {n} 天去过 5 次以上的区域。", { n: st.days })}</p>
    <div class="inline" style="margin-top:var(--space-4)"><button class="mx-btn mx-btn--secondary" id="pl-add">${tr("添加一个地方")}</button></div>`;
}

function bindOften() {
  $app.querySelectorAll(".pf-list [data-pl]").forEach((b) => (b.onclick = () => labelDialog(+b.dataset.pl)));
  $app.querySelectorAll("[data-name-it]").forEach((b) => (b.onclick = () => {
    const r = PL.stats.frequent[+b.dataset.nameIt];
    placeDialog({ lat: r.lat, lon: r.lon, name: `${r.district || ""} · ${r.name}`, label: null, type: "other" });
  }));
  const add = document.getElementById("pl-add");
  if (add) add.onclick = addPlaceDialog;
}

/** 全面页的「地点」：按地点合计的条形图（加上路上、没记到），明细在地点页签；不放跳转按钮 */
function placeTotals(visits, date) {
  return placeTotalsDays([{ visits, date }]);
}

/** 「地点」：几天的停留合在一起算（周、月总结用），每天按自己那天的起止切；没有停留点的日子不算 */
function placeTotalsDays(days) {
  days = days.filter((d) => d.visits && d.visits.length);
  if (!days.length) return "";
  const tot = new Map();
  let go = 0, none = 0;
  for (const d of days) for (const b of placeBlocks(d.visits, d.date)) {
    const sec = (b.z - b.a) / 1000;
    if (b.kind === "go") go += sec;
    else if (b.kind === "none") none += sec;
    else { const n = placeKey(b.v); tot.set(n, (tot.get(n) || 0) + sec); }
  }
  const tones = toneMap(tot);
  let rows = [...tot].sort((a, b) => b[1] - a[1]).map(([n, sec]) => ({ n, sec, c: "pl", tone: tones.get(n) }));
  // 几天合起来地方多：列前 10 个，其余合成一行
  if (days.length > 1 && rows.length > 11) rows = [...rows.slice(0, 10), { n: tr("其他 {n} 个地方", { n: rows.length - 10 }), sec: rows.slice(10).reduce((a, r) => a + r.sec, 0), c: "pl", other: true }];
  if (go) rows.push({ n: tr("路上"), sec: go, c: "go" });
  if (none) rows.push({ n: tr("没记到"), sec: none, c: "none" });
  const max = Math.max(...rows.map((r) => r.sec), 1), all = rows.reduce((a, r) => a + r.sec, 0) || 1;
  const pct = (sec) => { const p = (sec / all) * 100; return p < 1 ? "<1%" : Math.round(p) + "%"; };
  const bars = `<div class="bars pl-bars">${rows.map((r) => `<div class="bar-row"><span class="bar-name${r.c === "pl" && !r.other ? "" : " dim"}" title="${esc(r.n)}">${esc(r.n)}</span><span class="bar"><i class="${r.c}" style="width:${Math.max(2, Math.round((r.sec / max) * 100))}%${r.tone ? `;background:var(--pl-${r.tone}-s)` : ""}"></i></span><span class="bar-v mono">${r.sec < 3600 ? Math.round(r.sec / 60) + "m" : hh(r.sec) + "h"}</span><span class="bar-p mono">${pct(r.sec)}</span></div>`).join("")}</div>`;
  return section(tr("地点"), bars);
}

/** 时间线上的一段停留 */
function stayItem(v, i, convs) {
  const dur = spanText((visitEnd(v) - Date.parse(v.start)) / 1000);
  const meta = [visitSpan(v), dur];
  if (!v.label && v.doing && v.doing.p >= 0.5 && !["picked", "phone"].includes(v.confirmed)) meta.push(v.doing.kind);
  // 自动写上的也标把握（第一名的）
  const topP = (v.guesses || [])[0] ? ` ${pctText(v.guesses[0].p)}` : "";
  if (v.confirmed === "auto") meta.push(`${tr("自动认出")}${topP}`);
  if (v.confirmed === "remembered") meta.push(`${tr("按上次认定")}${topP}`);
  const ok = v.confirmed === "picked" || v.confirmed === "phone";
  const pct = (g) => tr("{name}：把握 {p}，离记到的位置 {m} 米", { name: shopShort(g.name), p: pctText(g.p), m: Math.round(g.distance) });
  // 还没认定：名字后面淡色写最可能的一家和把握
  const top = !v.label && !v.confirmed && (v.guesses || [])[0];
  const likely = top ? `<span class="ptl-likely">· ${tr("可能是 {name}", { name: esc(shopShort(top.name)) })} <span class="p">${pctText(top.p)}</span></span>` : "";
  // 候选：一段停留可以认定好几家（商场里逛了几家），点一下加上（打 ✓），再点取消。认定过的排前面，再补最像的凑满 3 个；
  // 一家没认定时问「是哪家？」，认定过的问「还去了？」。在手机上认定的不在这里列（改在 ⋯ 里）
  const shops = v.confirmed === "picked" ? (v.shops || []) : [];
  const chips = [...shops.map((n) => (v.guesses || []).find((g) => g.name === n) || { name: n }), ...(v.guesses || []).filter((g) => !shops.includes(g.name))].slice(0, Math.max(3, shops.length + 1));
  const guess = !v.label && v.confirmed !== "phone" && chips.length
    ? `<div class="ptl-guess">${shops.length ? tr("还去了？") : tr("是哪家？")}${chips.map((g) => { const on = shops.includes(g.name); return `<button class="${on ? "on" : ""}" data-chip="${i}" data-shop="${esc(g.name)}" aria-pressed="${on}" title="${esc(on ? tr("{name}：认定过，再点取消", { name: shopShort(g.name) }) : g.p != null ? pct(g) : shopShort(g.name))}">${on ? ICON.check : ""}${esc(shopShort(g.name))}${g.p != null ? `<span class="p">${pctText(g.p)}</span>` : ""}</button>`; }).join("")}<button class="other" data-edit="${i}">${tr("其他…")}</button></div>` : "";
  const talk = convs.length ? `<details class="ptl-talk"><summary>${tr("说话 {n} 段", { n: convs.length })} · ${talkText(convs.reduce((a, c) => a + (c.talk || []).reduce((x, t) => x + t.sec, 0), 0))}</summary><div class="tl">${convs.map((c) => convRow(c, "", `<span class="t">${hm(c.start)}</span>`)).join("")}</div></details>` : "";
  return `<li class="ptl-stay${ongoing(v) ? " now" : ""}" data-sel="${i}"><span class="ptl-t mono">${v.arrive_known === false && v.arrive ? tr("昨天") : hm(v.start)}</span><span class="ptl-dot">${MARKS[Math.min(i, MARKS.length - 1)]}</span>
    <div class="ptl-b"><div class="ptl-name"><span title="${esc(chainOf(v))}">${esc(placeName(v))}</span>${likely}${badgeHtml(v)}${ok ? `<span class="ok" title="${v.confirmed === "phone" ? tr("在手机上认定的") : tr("你认定的")}">${ICON.check}</span>` : ""}</div>
      <div class="ptl-meta">${esc(meta.join(" · "))}</div>${guess}${talk}</div>
    <div class="ptl-act"><button class="mx-iconbtn" data-edit="${i}" aria-label="${tr("改地点")}" title="${tr("改地点")}">${ICON.more}</button></div></li>`;
}

/** 点候选：加上这家（打 ✓）；已认定的再点＝取消这家 */
async function pickChip(visits, i, name) {
  const v = visits[i];
  if (!v || !name) return;
  const g = (v.guesses || []).find((x) => x.name === name) || {};
  await call("set_place_pick", { id: v.id, shop: name, kind: g.type || null, lat: v.lat, lon: v.lon });
  renderToday();
}

/** 改地点（挂在那一行下面的小弹窗）：搜索框在最上面；下面是认定的、最像的几家（电脑算的，按把握排）和高德附近 150 米的地点（按距离），
 *  点一个就加上（再点认定过的＝取消这家）；搜不到的直接输入店名；最下面给这里起名、认定过的能退回只写区域 */
function placeEditor(v, anchor) {
  closeEditor();
  const box = document.createElement("div");
  box.className = "mx-menu ptl-edit"; box.setAttribute("role", "menu");
  const cands = new Map();
  // 认定过的打 ✓，点一下取消；没认定的点一下加上（一段停留可以认定好几家）
  const shops = v.confirmed === "picked" ? (v.shops || []) : [];
  const row = (p, key, right) => { cands.set(key, p); const on = shops.includes(p.name); return `<button role="menuitemcheckbox" aria-checked="${on}" class="${on ? "on" : ""}" data-pk="${esc(key)}"><span class="pk-n">${esc(shopShort(p.name))}<span class="pk-t">${esc(((p.type || "").split(";")[1]) || (p.type || "").split(";")[0] || "")}</span></span>${on ? `<span class="mx-check">${ICON.check}</span>` : `<span class="mx-menu-val">${right}</span>`}</button>`; };
  const extra = shops.filter((n) => !(v.guesses || []).some((g) => g.name === n)).map((n, j) => row({ name: n }, "x" + j, "")).join("");
  const guesses = (v.guesses || []).map((g, j) => row(g, "g" + j, tr("{n} 米", { n: Math.round(g.distance) }))).join("");
  box.innerHTML = `${chainOf(v) ? `<div class="pe-chain">${esc(chainOf(v))}</div>` : ""}<input class="mx-input" id="pe-q" placeholder="${tr("搜店名，或直接输入")}" autocomplete="off">
    <div id="pe-found"></div>
    ${extra ? `<div class="mx-menu-group">${tr("认定的")}</div>${extra}` : ""}
    ${guesses ? `<div class="mx-menu-group">${tr("最像的")}</div>${guesses}` : ""}
    <div class="mx-menu-group">${tr("附近")}</div><div id="pe-near"><p class="note pe-wait">${tr("在问高德…")}</p></div>
    <hr><button role="menuitem" data-pe="label">${v.label ? tr("改这里的名字…") : tr("给这里起名…")}</button>
    ${shops.length ? `<button role="menuitem" data-pe="clear">${shops.length > 1 ? tr("都不确定，{n} 家全撤销", { n: shops.length }) : tr("不确定，退回只写区域")}</button>` : ""}`;
  anchor.appendChild(box);
  EDITOR = box;
  const choose = async (key) => {
    const c = key.startsWith("t:") ? { name: key.slice(2) } : cands.get(key);
    if (!c) return;
    await call("set_place_pick", { id: v.id, shop: c.name, kind: c.type || null, lat: c.lat ?? v.lat, lon: c.lon ?? v.lon });
    closeEditor(); renderToday();
  };
  box.addEventListener("click", async (e) => {
    e.stopPropagation();
    const b = e.target.closest("[data-pk]"); if (b) return choose(b.dataset.pk);
    const a = e.target.closest("[data-pe]"); if (!a) return;
    if (a.dataset.pe === "label") { closeEditor(); placeDialog(v); }
    if (a.dataset.pe === "clear") { await call("clear_place_pick", { id: v.id }); closeEditor(); renderToday(); }
  });
  const q = box.querySelector("#pe-q"); q.focus();
  let timer = null;
  q.oninput = () => {
    clearTimeout(timer);
    const t = q.value.trim(), out = box.querySelector("#pe-found");
    if (!t) { out.innerHTML = ""; return; }
    timer = setTimeout(async () => {
      const found = await call("amap_search", { text: t, lat: v.lat, lon: v.lon }).catch(() => []);
      if (q.value.trim() !== t) return;
      out.innerHTML = found.filter((p) => (p.distance || 0) <= 500).slice(0, 4).map((p) => row(p, "s:" + p.id, tr("{n} 米", { n: Math.round(p.distance || 0) }))).join("")
        + `<button role="menuitem" data-pk="t:${esc(t)}">${tr("就叫「{name}」", { name: esc(t) })}</button><hr>`;
    }, 350);
  };
  q.onkeydown = (e) => { if (e.key === "Escape") closeEditor(); if (e.key === "Enter" && !e.isComposing && q.value.trim()) choose("t:" + q.value.trim()); };
  call("amap_around", { lat: v.lat, lon: v.lon, radius: 150 })
    .then((near) => { const el = box.querySelector("#pe-near"); if (el) el.innerHTML = near.filter((p) => !(v.guesses || []).some((g) => g.name === p.name)).slice(0, 6).map((p) => row(p, "n:" + p.id, tr("{n} 米", { n: Math.round(p.distance || 0) }))).join("") || `<p class="note pe-wait">${tr("附近 150 米没有地点。")}</p>`; })
    .catch((x) => { const el = box.querySelector("#pe-near"); if (el) el.innerHTML = `<p class="note pe-wait">${esc(String(x))}</p>`; });
}
let EDITOR = null;
function closeEditor() { if (EDITOR) { EDITOR.remove(); EDITOR = null; } }
document.addEventListener("click", (e) => { if (EDITOR && !EDITOR.contains(e.target) && !e.target.closest("[data-edit]")) closeEditor(); });

/** 给一个地方起名（小弹层）：名字＋种类，存进本机的地点名单；清空＝删掉这个名字 */
function placeDialog(v) {
  const TYPES = [["home", tr("家")], ["work", tr("工作")], ["other", tr("其他")]];
  let kind = v.type || "other";
  openDialog({
    title: v.label ? tr("改「{name}」的名字", { name: v.label }) : tr("给「{name}」起名", { name: placeName(v) }), ok: tr("保存"),
    body: dlgField("pl-name", tr("名字"), { value: v.label || "", ph: tr("比如 家、公司、健身房"), help: v.label ? tr("清空就删掉这个名字") : tr("地图上查到的地名是「{name}」。", { name: esc(v.name || tr("没有")) }) })
      + `<div class="mx-field"><label>${tr("种类")}</label><div class="mx-seg" role="tablist" id="pl-kind">${TYPES.map(([k, t]) => `<button role="tab" aria-selected="${k === kind}" class="${k === kind ? "is-on" : ""}" data-v="${k}">${t}</button>`).join("")}</div></div>`,
    onOpen: (el) => {
      el.querySelector("#pl-name").maxLength = 20;
      el.querySelector("#pl-kind").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        kind = b.dataset.v;
        el.querySelectorAll("#pl-kind button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
      };
    },
    submit: async (el) => { await call("set_place_label", { lat: v.lat, lon: v.lon, label: dlgVal(el, "pl-name"), kind }); renderToday(); },
  });
}

// ---------- 状态 ----------

const S = { days: [], status: null, page: "today", rules: null, settings: null, draft: null };

/** 设置有一部分跟着账号同步（sync.rs 的 SHARED）：别的设备改了、这台同步下来以后，窗口回到前台时重读一次，侧栏跟着变 */
window.addEventListener("focus", async () => {
  if (!TAURI || !S.settings || document.querySelector(".dlg-root")) return;
  try {
    const s = await call("get_settings");
    if (JSON.stringify(s) === JSON.stringify(S.settings)) return;
    S.settings = s; S.draft = JSON.parse(JSON.stringify(s));
    renderSide();
    if (S.page === "settings") renderSettings();
  } catch (e) {}
});

async function boot() {
  [S.days, S.status, S.sync, S.profile] = await Promise.all([call("list_days"), call("get_status"), call("sync_status").catch(() => ({})), call("get_profile").catch(() => ({ name: tr("我") }))]);
  renderSide();
  loadUpdate();
  setInterval(loadUpdate, 10 * 60 * 1000);
}

function renderSide() {
  // 侧栏只放导航；记录状态在「设备」页，这里只在缺权限、暂停、没在记录时给「设备」挂一个点
  const st = S.status || {};
  const perm = st.permissions || {};
  // 手机版不采集，没有权限、暂停这些
  const bad = !st.mobile && (st.paused || !perm.accessibility || !perm.input_monitoring || !st.capturing);
  const dot = document.getElementById("dev-dot");
  // 账单只在电脑上导入、存在电脑上，手机版没有收支页
  const money = document.querySelector('#nav [data-page="money"]');
  if (money) money.hidden = !!st.mobile;
  // 游戏是可选功能（设置 → 功能，默认开），关了就不出现；手机版不放
  const games = document.querySelector('#nav [data-page="games"]');
  if (games) games.hidden = !!st.mobile || !(S.settings && S.settings.games);
  if (dot) { dot.hidden = !bad; dot.title = st.paused ? tr("已暂停") : tr("没在记录或缺权限"); }
  const pf = S.profile || { name: tr("我") };
  const av = document.getElementById("me-avatar");
  // 窄屏底栏「我的」用和别的页签一样的线条图标，不用头像
  if (av) { av.classList.toggle("avatar--icon", NARROW.matches); av.innerHTML = NARROW.matches ? lu('<path d="M18 20a6 6 0 0 0-12 0"/><circle cx="12" cy="10" r="4"/><circle cx="12" cy="12" r="10"/>') : avatarHtml(pf); }
  const md = document.getElementById("me-dot");
  if (md) { md.hidden = !updateReady(); md.title = tr("新版本 {v} 已就绪", { v: (S.update || {}).latest || "" }); }
  const nm = document.getElementById("me-name");
  // 窄屏（手机）底栏这一格是「我的」页签，不写名字
  if (nm) nm.textContent = NARROW.matches ? tr("我的") : pf.name || tr("我");
  const sub = document.getElementById("me-sub");
  // 正常记录时不写；暂停、缺权限、没在记录才提示
  if (sub) sub.textContent = st.mobile ? "" : st.paused ? " · " + tr("已暂停") : !perm.accessibility || !perm.input_monitoring ? " · " + tr("缺权限") : st.capturing ? "" : " · " + tr("没在记录");
  renderMeMenu();
}

/** 头像：有图片用图片，没有用名字第一个字 */
const avatarHtml = (pf) => (pf && pf.avatar ? `<img src="${esc(pf.avatar)}" alt="">` : esc(((pf && pf.name) || tr("我")).trim().slice(0, 1).toUpperCase()));

// ---------- 左下角菜单（照 Claude） ----------

// 图标：Lucide（ISC 许可，见 app/THIRD_PARTY_NOTICES.md），24 格画布、细线
const lu = (paths) => `<svg class="mx-ico lu" viewBox="0 0 24 24">${paths}</svg>`;
const MI = {
  gear: lu('<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>'),
  pause: lu('<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>'),
  play: lu('<path d="M6 3l14 9-14 9z"/>'),
  devices: lu('<path d="M18 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h8"/><path d="M10 19v-3.96 3.15"/><path d="M7 19h5"/><rect width="6" height="10" x="16" y="12" rx="2"/>'),
  folder: lu('<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>'),
  info: lu('<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>'),
  update: lu('<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>'),
  user: lu('<circle cx="12" cy="8" r="5"/><path d="M20 21a8 8 0 0 0-16 0"/>'),
  quit: lu('<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>'),
};

// ---------- 自动更新：后台下好了，菜单和设置里出「重启更新」 ----------

/** 新版本下好了、能装（电脑版）：头像和设置「通用」挂一个点，装完就没了 */
const updateReady = () => { const u = S.update || {}; return !!(u.newer && u.ready && !u.updatable) && !(S.status && S.status.mobile); };

/** 左下角菜单里版本那行：平时点了检查更新（菜单不关，就地显示结果），在下就显示进度，下好了点了重启更新 */
let ME_CHECKED = false;
function updateItem(ver) {
  const u = S.update, v = `<span class="mx-menu-val">${esc(ver)}</span>`;
  if (!u) return `<button role="menuitem" disabled>${MI.info}${tr("版本")}${v}</button>`;
  if (u.newer && u.ready && !u.updatable) return `<button role="menuitem" data-me="update">${MI.update}${tr("重启更新到 {v}", { v: esc(u.latest) })}</button>`;
  if (upBusy(u) || (u.newer && !u.updatable)) return `<button role="menuitem" disabled>${MI.update}${esc(upState(u))}</button>`;
  if (ME_CHECKED) return `<button role="menuitem" data-me="check" title="${esc(u.error || "")}">${MI.update}${u.error ? tr("没检查成功") : tr("已是最新版本")}${v}</button>`;
  return `<button role="menuitem" data-me="check">${MI.update}${tr("检查更新")}${v}</button>`;
}

async function loadUpdate() {
  if (!TAURI) return;
  try { S.update = await call("update_status"); renderSide(); updatePrompt(); } catch (e) {}
}

/** 更新说明：latest.json 里是一句话、分号隔开几件事，拆成 1. 2. 一行一件（只有一件就照写） */
/** 更新说明：英文界面用 notes_en（0.2.54 起发版带两份），没有就退回中文 */
const upNotes = (u) => (LANG === "en" && u.notes_en) || u.notes;
function noteList(notes) {
  const items = String(notes || "").split(/[；;\n]+/).map((x) => x.trim().replace(/[。.]$/, "")).filter(Boolean);
  if (!items.length) return "";
  return items.length > 1 ? `<ol class="up-notes">${items.map((x) => `<li>${esc(x)}</li>`).join("")}</ol>` : `<p class="up-notes">${esc(items[0])}</p>`;
}

/** 新版本下好了：开着窗口就弹一次。「稍后」这一版 24 小时内不再弹（菜单和设置里照样能点），换了新版本照弹；
 *  设置 → 通用 → 关于 关了「新版本弹窗提醒」就不弹，只在头像上挂点 */
function updatePrompt() {
  const u = S.update || {};
  if (!u.newer || !u.ready || u.updatable || (S.status && S.status.mobile) || DLG || (S.settings && (!S.settings.onboarded || S.settings.update_prompt === false))) return;
  let later = {};
  try { later = JSON.parse(localStorage.getItem("update-later") || "{}"); } catch (e) {}
  if (later.v === u.latest && Date.now() < later.until) return;
  openDialog({
    title: tr("新版本 {v} 已就绪", { v: u.latest }),
    body: `${illus("update", true)}${noteList(upNotes(u))}<p class="note up-meta">${tr("当前版本 {v}", { v: esc(u.current || "") })}</p>`,
    ok: tr("重启更新"), busy: tr("正在更新…"), cancel: tr("稍后"),
    // 装好了 app 会自己退出重开；没装成把错写在窗口里
    submit: async () => { await call("update_apply"); return false; },
    // 关掉（稍后、Esc、点遮罩、换页）都算稍后
    onClose: () => { try { localStorage.setItem("update-later", JSON.stringify({ v: u.latest, until: Date.now() + 864e5 })); } catch (e) {} },
  });
}

async function applyUpdate() {
  try { await call("update_apply"); } catch (x) { alert(tr("没更新成功：{x}", { x })); }
}

/** 下载进度：「正在下载 0.2.49 · 42%」 */
const upPct = (p) => p && p.total ? ` · ${Math.min(99, Math.floor((p.done / p.total) * 100))}%` : "";

/** 点了「检查更新」、还没返回：这时取到的状态可能还没开始查，按正在检查算 */
let UP_CLICK = false;
const upBusy = (u) => !!(UP_CLICK || u.checking || u.progress);

function upState(u) {
  return u.progress ? (upPct(u.progress) ? tr("正在下载 {v}", { v: u.latest }) + upPct(u.progress) : tr("正在下载 {v}…", { v: u.latest })) : u.checking || UP_CLICK ? tr("正在检查…") : u.updatable ? u.updatable : u.error ? u.error
    : u.newer ? (u.ready ? tr("{v} 已经下好，重启就能用。", { v: u.latest }) : tr("正在下载 {v}…", { v: u.latest }))
    : u.checked ? tr("已是最新版本（{t} 检查过）", { t: mdhm(u.checked) }) : tr("会自动检查更新");
}

/** 检查、下载中：每 1.5 秒刷一下关于里那行字，完了重画设置页 */
let UP_POLL = 0;
function pollUpdate() {
  clearTimeout(UP_POLL);
  const u = S.update || {};
  if (!TAURI || !upBusy(u)) return;
  UP_POLL = setTimeout(async () => {
    const el = document.getElementById("up-state"), menu = document.getElementById("me-menu");
    const menuOpen = menu && !menu.hidden;
    if (!el && !menuOpen) return;
    try { S.update = await call("update_status"); } catch (e) { return; }
    const n = S.update || {};
    if (upBusy(n)) { if (el) el.textContent = upState(n); if (menuOpen) renderMeMenu(); pollUpdate(); }
    else { renderSide(); updatePrompt(); if (document.getElementById("up-state")) renderSettings(); }
  }, 1500);
}

function aboutSection() {
  const u = S.update || {}, st = S.status || {};
  const state = upState(u);
  const btn = u.newer && u.ready && !u.updatable ? `<button class="mx-btn mx-btn--secondary mx-btn--sm" id="up-go">${tr("重启更新")}</button>` : `<button class="mx-btn mx-btn--secondary mx-btn--sm" id="up-check" ${upBusy(u) ? "disabled" : ""}>${tr("检查更新")}</button>`;
  const prompt = !(S.settings && S.settings.update_prompt === false);
  return section(tr("关于"), `<div class="mx-setting"><div class="mx-setting-main"><div class="mx-setting-name">${tr("版本")}</div><div class="mx-setting-desc">${esc(u.current || st.version || "")} · <span id="up-state">${esc(state)}</span></div>${u.newer && upNotes(u) ? `<div class="mx-setting-desc">${esc(upNotes(u))}</div>` : ""}</div>${TAURI ? btn : ""}</div>`
    + setRow(tr("新版本弹窗提醒"), tr("关了只在头像上挂一个点"), `<button class="mx-switch" role="switch" id="up-prompt" aria-checked="${prompt}" aria-label="${tr("新版本弹窗提醒")}"></button>`));
}

function bindAbout() {
  const c = document.getElementById("up-check");
  if (c) c.onclick = async () => {
    UP_CLICK = true; renderSettings();
    try { S.update = await call("update_check"); } catch (x) {}
    UP_CLICK = false; renderSide(); renderSettings(); updatePrompt();
  };
  const g = document.getElementById("up-go");
  if (g) g.onclick = applyUpdate;
  const p = document.getElementById("up-prompt");
  if (p) p.onclick = async () => { await saveSome({ update_prompt: S.settings.update_prompt === false }); renderSettings(); };
  pollUpdate();
}

function renderMeMenu() {
  const menu = document.getElementById("me-menu");
  if (!menu) return;
  const st = S.status || {};
  menu.innerHTML = `<div class="mx-menu-group">${esc((S.sync && S.sync.email) || (st.device || "").replace(/^mac-/, "Mac · "))}</div>
    <button role="menuitem" data-me="profile">${MI.user}${tr("编辑资料")}</button>
    <button role="menuitem" data-me="settings">${MI.gear}${tr("设置")}<span class="mx-menu-val">⌘,</span></button>
    <button role="menuitem" data-me="devices">${MI.devices}${tr("同步与设备")}</button>
    <button role="menuitem" data-me="data">${MI.folder}${tr("打开数据目录")}</button>
    <hr>
    ${st.paused
      ? `<button role="menuitem" data-me="resume">${MI.play}${tr("继续记录")}${st.paused_until ? `<span class="mx-menu-val">${tr("{t} 自动继续", { t: hm(st.paused_until) })}</span>` : ""}</button>`
      : `<button role="menuitem" data-me="pause">${MI.pause}${tr("暂停记录")}<span class="mx-menu-val">${tr("1 小时")}</span></button>`}
    <hr>
    ${updateItem(st.version || "")}
    <button role="menuitem" data-me="quit">${MI.quit}${tr("退出 Beayy")}</button>`;
}

function toggleMe(open) {
  const menu = document.getElementById("me-menu"), btn = document.getElementById("me");
  if (!menu) return;
  menu.hidden = open === undefined ? !menu.hidden : !open;
  if (menu.hidden && ME_CHECKED) { ME_CHECKED = false; renderMeMenu(); }
  btn.setAttribute("aria-expanded", String(!menu.hidden));
}

/** 手机（窄屏、iPhone 版）：左下角菜单变成底部页签里的「我的」，点了直接进设置 */
const NARROW = window.matchMedia("(max-width: 759px)");  // 和 style.css 的手机断点一致
NARROW.addEventListener("change", () => { if (S.status) renderSide(); });
document.getElementById("me").onclick = (e) => { e.stopPropagation(); if (NARROW.matches) { location.hash = "#/settings"; return; } toggleMe(); };
document.getElementById("me-menu").onclick = async (e) => {
  const b = e.target.closest("[data-me]"); if (!b) return;
  const k = b.dataset.me;
  if (k === "check") {
    e.stopPropagation();
    UP_CLICK = true; renderMeMenu();
    try { S.update = await call("update_check"); } catch (x) {}
    UP_CLICK = false; ME_CHECKED = true; renderSide(); updatePrompt(); pollUpdate();
    return;
  }
  toggleMe(false);
  if (k === "settings") { if (S.page !== "settings") location.hash = "#/settings"; }
  else if (k === "devices") { SS.tab = "data"; if (S.page === "settings") renderSettings(); else location.hash = "#/settings"; }
  else if (k === "profile") profileDialog();
  else if (k === "data") call("open_data_dir");
  else if (k === "quit") call("quit_app");
  else if (k === "update") applyUpdate();
  else { await call("set_paused", k === "pause" ? { paused: true, minutes: 60 } : { paused: false }); S.status = await call("get_status"); renderSide(); if (S.page === "settings" || S.page === "devices") route(); }
};
document.addEventListener("click", (e) => { if (!e.target.closest(".me-wrap")) toggleMe(false); });
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { toggleMe(false); if (document.getElementById("set-root") && !DLG) closeSettings(); }
  if (e.metaKey && e.key === ",") { e.preventDefault(); location.hash = "#/settings"; }
});

function markNav(page) {
  document.querySelectorAll("#nav a, .me").forEach((a) => a.classList.toggle("is-on", a.dataset.page === page));
}

// ---------- 今天 ----------

/** 「今天」页页签那一行右边的翻日期：一个整体「‹ 日期 ▾ ›」（点日期展开，只列有数据的日子，同总结页）；
 *  看以前的日子时左边多一个「今天」，同样高度 */
function dayPicker(date, past) {
  const now = localDay();
  const dates = [...new Set([now, ...S.days.map((x) => x.date)])].sort().reverse();
  const i = dates.indexOf(date), prev = dates[i + 1], next = i > 0 ? dates[i - 1] : null;
  return `${past ? `<button class="mx-btn mx-btn--secondary dn-today" data-day="${now}">${tr("今天")}</button>` : ""}
    <div class="daynav pick">
      <button class="dn-step" aria-label="${tr("前一天")}" title="${tr("前一天")}" ${prev ? `data-day="${prev}"` : "disabled"}>${ICON.chevL}</button>
      <button class="dn-date" id="day-pick" aria-haspopup="menu" aria-expanded="false">${short(date)}${ICON.chevD}</button>
      <button class="dn-step" aria-label="${tr("后一天")}" title="${tr("后一天")}" ${next ? `data-day="${next}"` : "disabled"}>${ICON.chevR}</button>
      <div class="mx-menu" role="menu" hidden>${dates.map((x) => `<button role="menuitemradio" aria-checked="${x === date}" data-day="${x}">${x === now ? tr("今天") : short(x)}${x === date ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div>`;
}
const goDay = (x) => { location.hash = x && x !== localDay() ? `#/today/${x}` : "#/today"; };

/** 「今天」页看的是哪一天：null＝今天；地址 #/today/YYYY-MM-DD 看以前的日子（概览、时间线、分类切来切去都停在这天） */
const TD = { date: null };
const isPastDay = () => TD.date && TD.date !== localDay();

async function renderToday() {
  const tk = RT.seq;
  const today = isPastDay() ? TD.date : localDay(), past = today !== localDay();
  // 今天总有实时数据（电脑来自自采事件，手机来自 iPhone 同步），不再等每晚的数据包；以前的日子按日期取
  const d = (await call("get_day", { date: today })) || { date: today };
  const pack = d.pack || {}, c = d.live || pack.computer || {}, ph = d.phone;
  const fda = ((S.status || {}).permissions || {}).full_disk_access;
  // iPhone 的数据是苹果经 iCloud 传到这台 Mac 的，常晚几个小时；落后 3 小时以上就写明在等苹果，免得以为 Beayy 坏了
  const android = d.phone_kind === "android";
  const phoneLag = android ? tr("安卓手机每 15 分钟传一次，手机没联网时会晚") : tr("iPhone 的屏幕使用时间由苹果经 iCloud 传到这台 Mac，常晚几个小时，到了 Beayy 才读得到；这段时间没用手机也会停在这里。\niPhone 的「设置 → 屏幕使用时间 → 在设备间共享」要开着");
  const phoneStale = ph && !past && Date.now() - Date.parse(ph.synced_until) > 3 * 3600e3;
  const phoneCap = ph ? (past ? (ph.pickups != null ? tr("拿起 {n} 次", { n: ph.pickups }) : "") : `${tr("同步至 {t}", { t: ymdhm(ph.synced_until) })}${phoneStale ? `\n${phoneLag}` : ""}`)
    : fda === false ? tr("未授权完全磁盘访问") : past ? tr("取不到") : `${tr("今天还没有手机数据")}\n${phoneLag}`;
  const hc = hourMinutes(c.active_hours || {});
  const hp = hourMinutes(ph ? ph.hours : (pack.phone || {}).hours);
  const sp = d.speech, visits = (d.places && d.places.visits) || [], hl = d.health;
  // 说话按分钟画，不满 1 分钟的也算 1 分钟，免得零碎的几句看不见
  const hw = sp ? Array.from({ length: 24 }, (_, i) => Math.min(60, Math.ceil(((sp.hours || {})[pad2(i)] || 0) / 60))) : null;
  const items = timelineItems(d);
  const counts ={ all: items.length, computer: items.filter((x) => x.dev === "computer").length, phone: items.filter((x) => x.dev === "phone" && !x.visit).length };
  if (sp) counts.watch = items.filter((x) => x.dev === "watch").length;
  const tab = TL.tab in counts ? TL.tab : "all";
  const shown = items.filter((x) => tab === "all" || x.dev === tab);
  // 照 Claude：标题下一行，左边「概览｜时间线」纯文字页签；时间线后面接「全部／电脑／手机」切换块；右边放同步时刻
  // 三个页签：全面（面：这一天的汇总看板）、时间线（线：按小时）、地点（点：按停留，去了哪、认定是哪家）
  const view = TL.view === "timeline" ? "timeline" : ["places", "map"].includes(TL.view) ? "places" : "overview";
  const pview = ["map", "often"].includes(TL.pview) ? TL.pview : "stays";
  const pseg = `<div class="mx-seg" role="tablist" id="pl-tabs">${[["stays", tr("停留")], ["map", tr("地图")], ["often", tr("常去")]].map(([k, v]) =>
    `<button role="tab" aria-selected="${k === pview}" class="${k === pview ? "is-on" : ""}" data-pv="${k}">${v}</button>`).join("")}</div>`;
  const seg = `<div class="mx-seg" role="tablist" id="tl-tabs">${[["all", tr("全部")], ["computer", tr("电脑")], ["phone", tr("手机")], ["watch", tr("手表")]].filter(([k]) => k in counts).map(([k, v]) =>
    `<button role="tab" aria-selected="${k === tab}" class="${k === tab ? "is-on" : ""}" data-tab="${k}">${v}<span class="n">${counts[k]}</span></button>`).join("")}</div>`;
  const toolbar = `<div class="toolbar"><div class="toolbar-left"><div class="tabs" role="tablist" id="view-tabs">${[["overview", tr("全面")], ["timeline", tr("时间线")], ["places", tr("地点")]].map(([k, v]) =>
      `<button role="tab" aria-selected="${k === view}" class="${k === view ? "is-on" : ""}" data-view="${k}">${v}</button>`).join("")}</div>
      ${view === "timeline" ? `<span class="tb-sep"></span>${seg}` : ""}${view === "places" ? `<span class="tb-sep"></span>${pseg}` : ""}</div>
    <div class="toolbar-right">${ph && !past ? (phoneStale ? `<span class="note tb-note" title="${esc(phoneLag)}">${android ? tr("手机停在 {t}", { t: hm(ph.synced_until) }) : tr("手机停在 {t}，在等苹果传来", { t: hm(ph.synced_until) })}</span>` : `<span class="note tb-note">${tr("同步至 {t}", { t: ymdhm(ph.synced_until) })}</span>`) : ""}${view === "places" && pview === "often" ? "" : dayPicker(today, past)}</div></div>`;
  const overview = `<div class="readouts">
        ${readout(tr("在电脑前"), hh(c.active_sec), "h", c.first ? `${c.first}–${c.last}` : tr("还没有记录"))}
        ${readout(tr("手机"), ph || fda !== false ? hh(ph ? ph.total_sec : (pack.phone || {}).total_sec) : "—", "h", phoneCap)}
        ${sp ? readout(tr("说话"), sp.talk_sec < 3600 ? mins(sp.talk_sec) : hh(sp.talk_sec), sp.talk_sec < 3600 ? tr("分钟") : "h", sp.first ? tr("录音 {a}–{b}", { a: hm(sp.first), b: hm(sp.last) }) : past ? tr("这一天没有录音") : tr("今天还没有录音")) : ""}
        ${readout(tr("切换"), c.switches ?? "—", tr("次"), tr("应用之间切换"))}
        ${visits.length ? readout(tr("去了"), new Set(visits.map((v) => v.label || placeName(v).split(" · ")[0])).size, tr("处"), tr("{n} 段停留", { n: visits.length })) : ""}
        ${hl && hl.sleep ? readout(tr("睡眠"), hh(hl.sleep.asleep_sec), "h", tr("昨晚 {a}–{b}", { a: hm(hl.sleep.start), b: hm(hl.sleep.end) })) : ""}
        ${hl && hl.activity && hl.activity.steps != null ? readout(tr("步数"), numText(hl.activity.steps), tr("步"), tr("苹果健康")) : ""}
      </div>
      <div class="secs-plain" style="margin-top:var(--space-6)">`
    + recentDays(today, { c: c.active_sec, p: ph ? ph.total_sec : (pack.phone || {}).total_sec })
    + section(tr("一天"), dayStrip(hc, hp, true, { hw, visits, date: today, sleep: hl && hl.sleep_blocks }) + `<div class="legend"><span><i style="background:var(--rec-computer)"></i>${tr("电脑")}</span><span><i style="background:var(--rec-phone)"></i>${tr("手机")}</span>${sp ? `<span><i style="background:var(--rec-watch)"></i>${tr("说话")}</span>` : ""}${visits.length ? `<span><i style="background:var(--rec-go-s)"></i>${tr("路上")}</span>` : ""}</div>`)
    + placeTotals(visits, today)
    + healthSection(hl)
    + (workSection(d.work) || appsSection(c.apps)) + `</div>`;
  if (view === "places" && (pview === "often" || !PL.stats || Date.now() - (PL.at || 0) > 600e3)) {
    [PL.stats, PL.labels] = await Promise.all([call("place_stats", { days: 30 }).catch(() => null), call("list_place_labels").catch(() => [])]);
    PL.at = Date.now();
  }
  const placesView = pview === "often" ? oftenView(PL.stats) : visits.length ? placesSection(visits, today, sp && sp.conversations, pview)
    : emptyBlock("map", past ? tr("这一天没有停留点") : tr("今天还没有停留点"));
  // 过了摘要间隔再加半小时还没摘要的段：多半是模型没配好或额度用完，说一句、给个去处
  const wait = ((S.settings && S.settings.summary_minutes) || 60) + 30;
  const rawOld = tab !== "phone" && tab !== "watch" && (d.segments || []).some((s) => s.source === "raw" && Date.parse(s.end) < Date.now() - wait * 60e3);
  const rawNote = rawOld ? `<p class="note raw-note">${tr("没有摘要的时段只列了用过的 app")} · <a href="#/settings" data-set-tab="ai">${tr("设置模型")}</a></p>` : "";
  const timeline = shown.length ? rawNote + `<div class="tl" id="tl">${tab === "all" ? hourRows(d, items) : tab === "phone" && visits.length ? phoneByPlace(shown, visits, today) : shown.map(tlRow).join("")}</div>`
    : past ? (tab === "phone" || tab === "watch" ? emptyInline(tab === "phone" ? tr("这一天没有手机记录") : tr("这一天没有对话")) : emptyBlock("day", tr("这一天没有记录")))
    : tab === "phone" ? `<p class="mx-empty-inline">${tr("手机记录还没同步过来")}${android ? "" : ` · ${howto("screentime", tr("怎么设置"))}`}</p>`
    : tab === "watch" ? emptyInline(tr("今天还没有对话")) : emptyBlock("day", tr("今天还没有记录"));

  // 照 Claude 自定义页：标题单独一行；页签和右边的控件（同步时刻、翻日期）同一行
  if (stale(tk)) return; $app.innerHTML = header(past ? short(today) : tr("今天")) + toolbar
    + `<div class="reading">${view === "timeline" ? timeline : view === "places" ? placesView : overview}</div>`;
  // 地图视图铺满窗口（不受阅读宽度限制）
  $app.classList.toggle("wide", view === "places" && pview === "map");
  if (view === "places" && pview === "map" && visits.length) drawLiveMap(visits, today, PL_SEL.date === today ? PL_SEL.i : null);
  bindOften();
  const plTabs = document.getElementById("pl-tabs");
  if (plTabs) plTabs.onclick = (e) => {
    const b = e.target.closest("[data-pv]"); if (!b) return;
    TL.pview = b.dataset.pv;
    try { localStorage.setItem("place-view", TL.pview); } catch (err) {}
    renderToday();
  };

  document.getElementById("view-tabs").onclick = (e) => {
    const b = e.target.closest("[data-view]"); if (!b) return;
    TL.view = b.dataset.view;
    try { localStorage.setItem("today-view", TL.view); } catch (err) {}
    renderToday();
  };
  const tlTabs = document.getElementById("tl-tabs");
  if (tlTabs) tlTabs.onclick = (e) => {
    const b = e.target.closest("[data-tab]"); if (!b) return;
    TL.tab = b.dataset.tab;
    try { localStorage.setItem("tl-tab", TL.tab); } catch (err) {}
    renderToday();
  };
  bindPick(document.getElementById("day-pick"));
  $app.querySelectorAll("[data-set-tab]").forEach((a) => (a.onclick = () => { SS.tab = a.dataset.setTab; try { localStorage.setItem("settings-tab", SS.tab); } catch (x) {} }));
  $app.querySelectorAll("[data-day]").forEach((b) => (b.onclick = () => goDay(b.dataset.day)));
  $app.querySelectorAll("[data-chip]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); pickChip(visits, +b.dataset.chip, b.dataset.shop); }));
  $app.querySelectorAll("[data-edit]").forEach((b) => (b.onclick = (e) => { e.stopPropagation(); const li = b.closest("li"); EDITOR && EDITOR.parentNode === li ? closeEditor() : placeEditor(visits[+b.dataset.edit], li); }));
  // 点一段停留：地图上那个点变成强调色，这一行高亮；再点取消
  $app.querySelectorAll("[data-sel]").forEach((li) => (li.onclick = (e) => {
    if (!li.classList.contains("pm-row") && e.target.closest("button, summary, details .tl, .ptl-edit")) return;
    if (!document.getElementById("pl-map") && !document.getElementById("pl-live")) return;
    const i = +li.dataset.sel, same = PL_SEL.date === today && PL_SEL.i === i;
    PL_SEL = same ? { date: null, i: null } : { date: today, i };
    $app.querySelectorAll("[data-sel]").forEach((x) => x.classList.toggle("is-sel", !same && +x.dataset.sel === i));
    LM.map ? liveSelect(same ? null : i, true) : drawPlaceMap(visits, same ? null : i);
  }));
  if (pview === "map" && PL_SEL.date === today && PL_SEL.i != null) { const li = $app.querySelector(`[data-sel="${PL_SEL.i}"]`); if (li) li.classList.add("is-sel"); }
  // 点一天的形状里的某个小时：切到时间线，滚到那个小时
  const jumpTo = (h) => { TL.view = "timeline"; TL.jump = h; renderToday(); };
  $app.querySelectorAll(".strip [data-h], .place-bar [data-h]").forEach((cell) => (cell.onclick = () => jumpTo(+cell.dataset.h)));
  bindStripKeys($app, jumpTo);
  if (view === "timeline" && TL.jump != null) {
    const h = TL.jump; TL.jump = null;
    const rows = [...$app.querySelectorAll("#tl > [data-h]")];
    const hit = rows.filter((r) => +r.dataset.h === h).pop() || rows.find((r) => +r.dataset.h < h);
    if (hit) {
      hit.scrollIntoView({ block: "center" });
      hit.classList.remove("flash"); void hit.offsetWidth; hit.classList.add("flash");
    }
  }
  loadIcons();
}

// ---------- 时间花在哪：按项目、按类型（10 分钟摘要里模型给的标签 × 在电脑前的秒数） ----------

/** 展开着的一级（「activities:创作」这样的键），记在本机，每分钟自动刷新后保持 */
const WORK_OPEN = new Set((() => { try { return JSON.parse(localStorage.getItem("work-open") || "[]"); } catch (e) { return []; } })());

function workSection(w) {
  if (!w || !((w.domains || []).length || (w.activities || []).length)) return "";
  const dur = (sec) => (sec < 3600 ? Math.round(sec / 60) + "m" : hh(sec) + "h");
  // 两级：一级一行，有二级的点整行展开（不画箭头）；条长按这一列最长的一级比，百分比按这一列合计算
  const tree = (kind, rows) => {
    const max = Math.max(...rows.map((r) => r.sec), 1), total = rows.reduce((a, r) => a + r.sec, 0) || 1;
    const pct = (sec) => { const p = (sec / total) * 100; return p < 1 ? "<1%" : Math.round(p) + "%"; };
    // 类型、领域是固定的几个词（beayy-summary 的 ACTIVITY_TREE、DOMAINS），显示时查译文；项目名查不到，原样
    const cells = (r) => `<span class="bar-name${r.dim ? " dim" : ""}" title="${esc(tr(r.name))}">${esc(tr(r.name))}</span><span class="bar"><i style="width:${Math.max(2, Math.round((r.sec / max) * 100))}%"></i></span><span class="bar-v mono">${dur(r.sec)}</span><span class="bar-p mono">${pct(r.sec)}</span>`;
    return `<div class="bars">${rows.map((r) => {
      const kids = r.children.length > 1 || (r.children[0] && r.children[0].name !== r.name) ? r.children.slice(0, kind === "projects" ? undefined : 6) : [];
      if (!kids.length) return `<div class="bar-row">${cells(r)}</div>`;
      const k = `${kind}:${r.name}`, open = WORK_OPEN.has(k);
      return `<div class="bar-grp"><button class="bar-row top" data-work="${esc(k)}" aria-expanded="${open}">${cells(r)}</button>
        <div class="bar-kids"${open ? "" : " hidden"}>${kids.map((c) => `<div class="bar-row kid">${cells(c)}</div>`).join("")}</div></div>`;
    }).join("")}</div>`;
  };
  // 项目：不再先分领域，所有项目按时长排，占时间长的在前。没归到项目的工作时间叫「未归项目」、灰字放最后；
  // 别的领域（休闲、关系……）里没归到项目的按领域名算一行。项目这列的行数和类型那列一样（类型全列），
  // 放不下的合成「其他 N 个项目」，点开看是哪些（两列一样多）
  const pj = new Map();
  const add = (name, sec) => pj.set(name, (pj.get(name) || 0) + sec);
  for (const d of w.domains || []) {
    const named = d.children.filter((c) => c.name !== "未归项目");
    for (const c of named) add(c.name, c.sec);
    const rest = d.sec - named.reduce((a, c) => a + c.sec, 0);
    if (rest >= 30) add(d.name === "工作" ? "未归项目" : d.name, rest);
  }
  const projects = [...pj].filter(([n]) => n !== "未归项目").map(([name, sec]) => ({ name, sec, children: [] })).sort((a, b) => b.sec - a.sec);
  const slots = Math.max((w.activities || []).length, 3) - (pj.has("未归项目") ? 1 : 0);
  if (projects.length > slots) {
    const more = projects.splice(slots - 1);
    projects.push({ name: tr("其他 {n} 个项目", { n: more.length }), sec: more.reduce((a, r) => a + r.sec, 0), children: more, dim: true });
  }
  if (pj.has("未归项目")) projects.push({ name: tr("未归项目"), sec: pj.get("未归项目"), children: [], dim: true });
  return section(tr("时间"), `<div class="work"><div><h3>${tr("项目")}</h3>${tree("projects", projects)}</div><div><h3>${tr("类型")}</h3>${tree("activities", w.activities || [])}</div></div>`);
}

/** 还没有摘要（没配模型）时，「时间」只按 app 列：电脑上用得最多的 8 个 */
function appsSection(apps) {
  const xs = (apps || []).filter((a) => !NOT_APPS.has(a.bundle_id) && a.sec >= 60).slice(0, 8);
  if (!xs.length) return "";
  const max = xs[0].sec, total = xs.reduce((a, x) => a + x.sec, 0);
  const dur = (sec) => (sec < 3600 ? Math.round(sec / 60) + "m" : hh(sec) + "h");
  return section(tr("时间"), `<div class="bars">${xs.map((a) => `<div class="bar-row"><span class="bar-name" title="${esc(a.name)}">${esc(a.name)}</span><span class="bar"><i style="width:${Math.max(2, Math.round((a.sec / max) * 100))}%"></i></span><span class="bar-v mono">${dur(a.sec)}</span><span class="bar-p mono">${Math.round((a.sec / total) * 100)}%</span></div>`).join("")}</div>`);
}

// 展开／收起一级：直接改这一块，不重画整页
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-work]"); if (!b) return;
  const k = b.dataset.work, open = b.getAttribute("aria-expanded") !== "true";
  b.setAttribute("aria-expanded", open);
  b.nextElementSibling.hidden = !open;
  open ? WORK_OPEN.add(k) : WORK_OPEN.delete(k);
  try { localStorage.setItem("work-open", JSON.stringify([...WORK_OPEN])); } catch (x) {}
});

// ---------- 时间线：电脑的 10 分钟摘要和手机的每次拿起，按设备分页签，新的在上 ----------

/** 「全部」页签按小时：每小时一行（小时摘要＋项目＋电脑和手机用过的 app 图标＋手机用了多久），点开看这一小时的 10 分钟明细和每次拿起；
 *  连着的「不在电脑前、也没碰手机」的小时合成一行灰字 */
function hourRows(d, items) {
  const hourOf = (iso) => { const t = new Date(iso); t.setMinutes(0, 0, 0); return t.getTime(); };
  const roll = Object.fromEntries((d.hours || []).map((h) => [hourOf(h.start), h]));
  const buckets = new Map();
  const put = (k, x) => { if (!buckets.has(k)) buckets.set(k, []); buckets.get(k).push(x); };
  for (const x of items) {
    if (x.dev === "computer" && x.seg.kind === "idle") continue; // 合并过的空段按小时摘要算
    if (x.visit) continue; // 停留点写在每小时行首
    if (x.conv) {
      // 对话按小时切开：每个小时只放这一小时里说的句子
      const by = new Map();
      for (const l of x.conv.lines || []) { const k = hourOf(l.start); if (!by.has(k)) by.set(k, []); by.get(k).push(l); }
      for (const [k, lines] of by) {
        const talk = {};
        for (const l of lines) talk[l.speaker] = (talk[l.speaker] || 0) + (Date.parse(l.end) - Date.parse(l.start)) / 1000;
        put(k, { dev: x.dev, start: lines[0].start, end: lines[lines.length - 1].end, conv: { lines, talk: Object.entries(talk).map(([speaker, sec]) => ({ speaker, sec: Math.round(sec) })) } });
      }
      continue;
    }
    put(hourOf(x.start), x);
  }
  const visits = (d.places && d.places.visits) || [];
  const talkH = (d.speech && d.speech.hours) || {};
  for (const k of Object.keys(roll)) if (!buckets.has(+k)) buckets.set(+k, []);
  const keys = [...buckets.keys()].sort((a, b) => b - a);
  const now = Date.now();
  const out = [];
  let idleRun = null; // 连着的空小时 [最早, 最晚]
  const flushIdle = () => { if (idleRun) { out.push(`<div class="tl-row computer idle" data-h="${new Date(idleRun[0]).getHours()}"><span class="t">${hm(new Date(idleRun[0]).toISOString())}–${hm(new Date(idleRun[1] + 3600e3).toISOString())}</span><span>${tr("不在电脑前")}</span></div>`); idleRun = null; } };
  for (const k of keys) {
    const xs = buckets.get(k), r = roll[k];
    const comp = xs.filter((x) => x.dev === "computer"), ph = xs.filter((x) => x.dev === "phone" && x.ses), wt = xs.filter((x) => x.conv);
    const idle = !comp.length && (!r || r.title === "不在电脑前") && k + 3600e3 <= now;
    if (idle && !ph.length && !wt.length) { idleRun = idleRun ? [k, idleRun[1]] : [k, k]; continue; }
    flushIdle();
    const phSec = ph.reduce((a, x) => a + x.ses.sec, 0);
    const phApps = {};
    for (const x of ph) for (const a of x.ses.apps) phApps[a.name] = (phApps[a.name] || 0) + a.sec;
    const topPh = Object.entries(phApps).sort((a, b) => b[1] - a[1]).slice(0, 3).map(([n]) => n).join(tr("、"));
    let latest = comp[0] && comp[0].seg;
    if (!r && comp.length && comp.every((x) => x.seg.source === "raw")) {
      const sum = {};
      for (const x of comp) for (const a of x.seg.raw_apps || []) (sum[a.bundle_id + a.name] ||= { ...a, sec: 0 }).sec += a.sec;
      latest = { title: rawTitle(Object.values(sum).sort((a, b) => b.sec - a.sec)), description: "" };
    }
    const title = r && r.title !== "不在电脑前" ? r.title : comp.length ? (k + 3600e3 > now ? tr("进行中：{t}", { t: latest.title }) : latest.title) : "不在电脑前";
    const desc = r && r.title !== "不在电脑前" ? r.description : comp.length && k + 3600e3 > now ? latest.description : "";
    // 项目：小时摘要里按秒累计好的，没有（进行中）就用这几段的
    const pj = {};
    for (const p of (r && r.parts) || []) if (p.project) pj[p.project] = (pj[p.project] || 0) + p.sec;
    if (!r) for (const x of comp) for (const p of x.seg.parts || []) if (p.project) pj[p.project] = (pj[p.project] || 0) + (x.seg.active_sec || 0) * p.share;
    const chips = Object.entries(pj).sort((a, b) => b[1] - a[1]).slice(0, 3).map(([n, v]) => `<span class="mx-tag">${esc(n)} ${Math.round(v / 60)}m</span>`).join("");
    // 地点只在和上一个小时不一样时写（到了新地方），免得每小时都重复「公司」
    const here = placeAt(visits, k, k + 3600e3), before = placeAt(visits, k - 3600e3, k);
    const where = here && (!before || placeName(before) !== placeName(here)) ? placeName(here) : null;
    const talkSec = talkH[pad2(new Date(k).getHours())] || 0;
    const talkWho = {};
    for (const x of wt) for (const t of x.conv.talk) talkWho[t.speaker] = (talkWho[t.speaker] || 0) + t.sec;
    const who = speakers(Object.entries(talkWho).map(([speaker, sec]) => ({ speaker, sec })), SELF);
    const inner = xs.sort((a, b) => (a.start < b.start ? 1 : -1)).map(tlRow).join("");
    // 这一小时电脑、手机用过的 app 图标（按时间先后，各最多 10 个）
    const cApps = [...new Set([...comp].reverse().flatMap((x) => x.seg.apps || []))];
    const pNames = Object.fromEntries(ph.flatMap((x) => x.ses.apps.map((a) => [a.bundle_id, a.name])));
    const pApps = [...new Set([...ph].reverse().flatMap((x) => x.ses.apps.map((a) => a.bundle_id)))];
    const cIco = icons(cApps, false), pIco = icons(pApps, true, pNames);
    const appsLine = cIco || pIco ? `<div class="hour-apps">${cIco ? `<span class="lbl">${tr("电脑")}</span>${cIco}` : ""}${pIco ? `<span class="lbl">${tr("手机")}</span>${pIco}` : ""}</div>` : "";
    out.push(`<details class="tl-row hour" data-h="${new Date(k).getHours()}"><summary><span class="t">${hm(new Date(k).toISOString())}–${hm(new Date(k + 3600e3).toISOString())}${where ? `<div class="where" title="${esc(where)}">${esc(where)}</div>` : ""}</span><span>
        <span class="name${title === "不在电脑前" ? " dim" : ""}">${esc(title === "不在电脑前" ? tr(title) : title)}</span>${desc ? `<div class="desc">${esc(desc)}</div>` : ""}
        ${chips ? `<div class="chips">${chips}</div>` : ""}${appsLine}${phSec ? `<div class="ph-line">${tr("手机 {d}", { d: minText(phSec) })}${topPh ? ` · ${esc(topPh)}` : ""}</div>` : ""}${wt.length ? `<div class="ph-line">${tr("对话 {d}", { d: talkText(talkSec) })} · ${esc(who.slice(0, 3).join(tr("、")))}${who.length > 3 ? ` ${tr("等 {n} 人", { n: who.length })}` : ""}</div>` : ""}</span>
        ${inner ? `<span class="chev">${ICON.chevR}</span>` : "<span></span>"}</summary>${inner ? `<div class="hour-in">${inner}</div>` : ""}</details>`);
  }
  flushIdle();
  return out.join("");
}


/** 地点 · 地图：高德 JS API 2.0（Key 和安全密钥在钥匙串，amap_js_config 给页面）。停留点按先后编号、虚线连起来，
 *  常去的点深一点、新地方浅一点，选中的强调色；点点和点清单互相选中；同一天自动刷新时保留缩放和位置。加载不了退回静态图 */
const LM = { map: null, day: null, view: null, loading: null, markers: [], visits: [] };
function loadAmapJs() {
  if (window.AMap) return Promise.resolve(window.AMap);
  if (LM.loading) return LM.loading;
  LM.loading = call("amap_js_config").then((c) => new Promise((ok, fail) => {
    if (!c || !c.key) return fail(new Error(c && c.error ? c.error : tr("还没有地图 Key（设置 → 连接 → 地图）")));
    // 官方：安全密钥留在服务器，地图经中转地址取；自己的：页面里带安全密钥
    window._AMapSecurityConfig = c.service_host ? { serviceHost: c.service_host } : { securityJsCode: c.code };
    const sc = document.createElement("script");
    sc.src = `https://webapi.amap.com/maps?v=2.0&key=${encodeURIComponent(c.key)}&plugin=AMap.Scale,AMap.ToolBar`;
    sc.onload = () => (window.AMap ? ok(window.AMap) : fail(new Error(tr("地图没加载上"))));
    sc.onerror = () => fail(new Error(tr("连不上高德地图")));
    document.head.appendChild(sc);
  })).catch((e) => { LM.loading = null; throw e; });
  return LM.loading;
}
/** WGS-84 → GCJ-02（同电脑和手机的换算） */
function gcjJs(lat, lon) {
  if (lon < 72.004 || lon > 137.8347 || lat < 0.8293 || lat > 55.8271) return [lat, lon];
  const a = 6378245.0, ee = 0.00669342162296594323, x = lon - 105, y = lat - 35, P = Math.PI;
  let dLat = -100 + 2 * x + 3 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * Math.sqrt(Math.abs(x));
  dLat += (20 * Math.sin(6 * x * P) + 20 * Math.sin(2 * x * P)) * 2 / 3 + (20 * Math.sin(y * P) + 40 * Math.sin(y / 3 * P)) * 2 / 3 + (160 * Math.sin(y / 12 * P) + 320 * Math.sin(y * P / 30)) * 2 / 3;
  let dLon = 300 + x + 2 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * Math.sqrt(Math.abs(x));
  dLon += (20 * Math.sin(6 * x * P) + 20 * Math.sin(2 * x * P)) * 2 / 3 + (20 * Math.sin(x * P) + 40 * Math.sin(x / 3 * P)) * 2 / 3 + (150 * Math.sin(x / 12 * P) + 300 * Math.sin(x / 30 * P)) * 2 / 3;
  const rad = lat / 180 * P, m = 1 - ee * Math.sin(rad) ** 2, sq = Math.sqrt(m);
  return [lat + dLat * 180 / ((a * (1 - ee)) / (m * sq) * P), lon + dLon * 180 / (a / sq * Math.cos(rad) * P)];
}
/** 地图点上的小卡片：名字、起止 · 时长、去了哪家（认定的）或最可能的一家和把握 */
function pinCard(v, i) {
  const dur = spanText((visitEnd(v) - Date.parse(v.start)) / 1000);
  const g = (v.guesses || [])[0];
  let shop = "";
  if (v.confirmed === "picked" && (v.shops || []).length) shop = tr("去了 {s}", { s: v.shops.map(shopShort).join(tr("、")) });
  else if (v.confirmed === "auto" || v.confirmed === "remembered") shop = g ? (v.confirmed === "auto" ? tr("自动认出 {s} {p}", { s: shopShort(g.name), p: pctText(g.p) }) : tr("按上次认定 {s} {p}", { s: shopShort(g.name), p: pctText(g.p) })) : "";
  else if (!v.label && !v.confirmed && g) shop = tr("可能是 {s} {p}", { s: shopShort(g.name), p: pctText(g.p) });
  return `<div class="lm-card"><div class="lm-card-n"><span class="ptl-dot">${MARKS[Math.min(i, MARKS.length - 1)]}</span>${esc(placeName(v))}${badgeHtml(v)}</div>
    <div class="lm-card-m">${esc(visitSpan(v))} · ${esc(dur)}</div>${shop ? `<div class="lm-card-s">${esc(shop)}</div>` : ""}</div>`;
}
const pinHtml = (v, i, on) => `<div class="lm-pin${badgeOf(v) ? " often" : ""}${on ? " sel" : ""}${ongoing(v) ? " now" : ""}" >${MARKS[Math.min(i, MARKS.length - 1)]}</div>`;
/** 地图跟窗口一样高（减去上面的标题和页签） */
function fitLive(box) {
  const h = Math.max(420, window.innerHeight - box.getBoundingClientRect().top - 48);
  box.style.height = `${h}px`;
}
window.addEventListener("resize", () => { const b = document.getElementById("pl-live"); if (b) fitLive(b); });
async function drawLiveMap(visits, date, sel) {
  const box = document.getElementById("pl-live"); if (!box) return;
  fitLive(box);
  if (S.settings && S.settings.map_provider === "osm") return drawLiveMapOsm(visits, date, sel, box);
  let A;
  try { A = await loadAmapJs(); }
  catch (e) { box.outerHTML = `<div class="pl-map-img" id="pl-map"><p class="note">${tr("{e}，先用静态图。", { e: esc(String(e.message || e)) })}</p></div>`; return drawPlaceMap(visits, sel); }
  if (!document.body.contains(box)) return;
  const keep = LM.day === date && LM.view;
  const keepAmap = keep && LM.kind === "amap";
  dropLiveMap();
  box.innerHTML = "";
  const dark = isDark();
  LM.kind = "amap";
  const map = new A.Map(box, { zoom: 14, mapStyle: dark ? "amap://styles/dark" : "amap://styles/whitesmoke" });
  map.addControl(new A.Scale());
  map.addControl(new A.ToolBar({ position: { top: "12px", right: "12px" } }));
  const pts = visits.map((v) => { const [la, lo] = gcjJs(v.lat, v.lon); return [lo, la]; });
  if (pts.length > 1) map.add(new A.Polyline({ path: pts, strokeColor: "#73726C", strokeWeight: 2, strokeOpacity: 0.8, strokeStyle: "dashed", strokeDasharray: [6, 6] }));
  LM.visits = visits;
  // 小卡片：悬停时出、移开就收；点过的那个一直开着（再点或点别的才换）
  const card = new A.InfoWindow({ isCustom: true, anchor: "bottom-center", offset: new A.Pixel(0, -22), autoMove: false });
  LM.pinned = null;
  const showCard = (i) => { card.setContent(pinCard(visits[i], i)); card.open(map, pts[i]); };
  LM.card = card;
  LM.markers = visits.map((v, i) => {
    const m = new A.Marker({ position: pts[i], content: pinHtml(v, i, i === sel), offset: new A.Pixel(-13, -13), zIndex: i === sel ? 200 : 100 + i });
    m.on("mouseover", () => showCard(i));
    m.on("mouseout", () => { if (LM.pinned == null) card.close(); else if (LM.pinned !== i) showCard(LM.pinned); });
    m.on("click", () => {
      LM.pinned = LM.pinned === i ? null : i;
      if (LM.pinned == null) card.close(); else showCard(i);
      const same = PL_SEL.i === i; PL_SEL = same ? { date: null, i: null } : { date, i };
      $app.querySelectorAll("[data-sel]").forEach((x) => x.classList.toggle("is-sel", !same && +x.dataset.sel === i));
      liveSelect(same ? null : i, false);
      const row = $app.querySelector(`.pm-row[data-sel="${i}"]`); if (row && !same) row.scrollIntoView({ block: "nearest" });
    });
    return m;
  });
  map.add(LM.markers);
  if (keepAmap) map.setZoomAndCenter(LM.view.zoom, LM.view.center, true);
  else map.setFitView(LM.markers, true, [80, 80, 80, 80]);
  const save = () => { LM.view = { zoom: map.getZoom(), center: map.getCenter() }; };
  map.on("moveend", save); map.on("zoomend", save);
  map.on("click", () => { LM.pinned = null; card.close(); });
  if (sel != null && pts[sel]) { LM.pinned = sel; showCard(sel); }
  LM.map = map; LM.day = date;
}
/** 选中一段：那个点变强调色压在最上面；从清单点的把地图挪过去（不缩小） */
function liveSelect(i, pan) {
  if (LM.kind === "osm") {
    LM.markers.forEach((m, j) => { m.setIcon(m.iconOf(j === i)); m.setZIndexOffset(j === i ? 1000 : j); });
    if (pan && i != null && LM.map) LM.map.setView(LM.markers[i].getLatLng(), Math.max(LM.map.getZoom(), 15));
    if (pan && LM.card && LM.map) { LM.pinned = i; if (i == null) LM.card.close(); else LM.card.show(i); }
    return;
  }
  LM.markers.forEach((m, j) => { m.setContent(pinHtml(LM.visits[j], j, j === i)); m.setzIndex(j === i ? 200 : 100 + j); });
  if (pan && i != null && LM.map) LM.map.setZoomAndCenter(Math.max(LM.map.getZoom(), 15), LM.markers[i].getPosition());
  if (pan && LM.card && LM.map) { LM.pinned = i; if (i == null) LM.card.close(); else { LM.card.setContent(pinCard(LM.visits[i], i)); LM.card.open(LM.map, LM.markers[i].getPosition()); } }
}

const isDark = () => document.documentElement.dataset.theme === "dark" || (!document.documentElement.dataset.theme && matchMedia("(prefers-color-scheme: dark)").matches);
/** 换地图（高德 ↔ OpenStreetMap）或重画前，拆掉旧的 */
function dropLiveMap() {
  if (!LM.map) return;
  try { LM.kind === "osm" ? LM.map.remove() : LM.map.destroy(); } catch (e) {}
  LM.map = null;
}
/** OpenStreetMap（设置 → 连接 → 地图选了它）：Leaflet＋OSM 官方的标准底图（免费、不用 Key）。
 *  坐标就是 WGS-84，不用换；编号点、虚线、悬停小卡片、和清单互相选中同高德的 */
function loadLeaflet() {
  if (window.L) return Promise.resolve(window.L);
  if (LM.leaflet) return LM.leaflet;
  LM.leaflet = new Promise((ok, fail) => {
    const css = document.createElement("link");
    css.rel = "stylesheet"; css.href = "https://unpkg.com/leaflet@1.9.4/dist/leaflet.css";
    document.head.appendChild(css);
    const sc = document.createElement("script");
    sc.src = "https://unpkg.com/leaflet@1.9.4/dist/leaflet.js";
    sc.onload = () => (window.L ? ok(window.L) : fail(new Error(tr("地图没加载上"))));
    sc.onerror = () => fail(new Error(tr("连不上 OpenStreetMap")));
    document.head.appendChild(sc);
  }).catch((e) => { LM.leaflet = null; throw e; });
  return LM.leaflet;
}
async function drawLiveMapOsm(visits, date, sel, box) {
  let L;
  try { L = await loadLeaflet(); }
  catch (e) { box.outerHTML = `<div class="pl-map-img" id="pl-map"><p class="note">${esc(String(e.message || e))}</p></div>`; return; }
  if (!document.body.contains(box)) return;
  const keep = LM.day === date && LM.view && LM.kind === "osm";
  dropLiveMap();
  box.innerHTML = "";
  LM.kind = "osm";
  const map = L.map(box);
  // OSM 官方的标准底图（CARTO 的浅色、深色底图 2026 起要 Key，不用了）；深色模式把底图反色（.lm-osm-dark）
  box.classList.toggle("lm-osm-dark", isDark());
  L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png",
    { maxZoom: 19, attribution: '© <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>' }).addTo(map);
  L.control.scale({ imperial: false }).addTo(map);
  const pts = visits.map((v) => [v.lat, v.lon]);
  if (pts.length > 1) L.polyline(pts, { color: "#73726C", weight: 2, opacity: 0.8, dashArray: "6 6" }).addTo(map);
  LM.visits = visits; LM.pinned = null;
  const card = L.popup({ closeButton: false, autoPan: false, offset: [0, -14], className: "lm-osm-card" });
  const showCard = (i) => card.setLatLng(pts[i]).setContent(pinCard(visits[i], i)).openOn(map);
  LM.card = { close: () => map.closePopup(card), show: showCard };
  LM.markers = visits.map((v, i) => {
    const iconOf = (on) => L.divIcon({ html: pinHtml(v, i, on), className: "lm-osm", iconSize: [26, 26], iconAnchor: [13, 13] });
    const m = L.marker(pts[i], { icon: iconOf(i === sel), zIndexOffset: i === sel ? 1000 : i }).addTo(map);
    m.iconOf = iconOf;
    m.on("mouseover", () => showCard(i));
    m.on("mouseout", () => { if (LM.pinned == null) map.closePopup(card); else if (LM.pinned !== i) showCard(LM.pinned); });
    m.on("click", () => {
      LM.pinned = LM.pinned === i ? null : i;
      if (LM.pinned == null) map.closePopup(card); else showCard(i);
      const same = PL_SEL.i === i; PL_SEL = same ? { date: null, i: null } : { date, i };
      $app.querySelectorAll("[data-sel]").forEach((x) => x.classList.toggle("is-sel", !same && +x.dataset.sel === i));
      liveSelect(same ? null : i, false);
      const row = $app.querySelector(`.pm-row[data-sel="${i}"]`); if (row && !same) row.scrollIntoView({ block: "nearest" });
    });
    return m;
  });
  if (keep) map.setView(LM.view.center, LM.view.zoom);
  else if (pts.length === 1) map.setView(pts[0], 16);
  else map.fitBounds(L.latLngBounds(pts), { padding: [80, 80], maxZoom: 16 });
  map.on("moveend", () => { LM.view = { zoom: map.getZoom(), center: map.getCenter() }; });
  map.on("click", () => { LM.pinned = null; map.closePopup(card); });
  if (sel != null && pts[sel]) { LM.pinned = sel; showCard(sel); }
  LM.map = map; LM.day = date;
}

/** 地图图片缓存：停留点和选中的那段没变就不再问高德（今天页每分钟自动刷新） */
const MAPCACHE = new Map();
/** 今天页选中的那段停留（哪天、第几段） */
let PL_SEL = { date: null, i: null };
function drawPlaceMap(visits, sel) {
  const pts = visits.map((v) => [v.lat, v.lon]), key = JSON.stringify([pts, sel, visits.map((v) => !!badgeOf(v))]);
  const show = (html) => { const box = document.getElementById("pl-map"); if (box) box.innerHTML = html; };
  if (MAPCACHE.has(key)) return show(MAPCACHE.get(key));
  call("amap_map", { points: pts, width: 640, height: 600, selected: sel, often: visits.map((v) => !!badgeOf(v)) })
    .then((src) => { const html = `<img src="${src}" alt="${tr("这天的停留点地图")}">`; MAPCACHE.set(key, html); if (MAPCACHE.size > 40) MAPCACHE.delete(MAPCACHE.keys().next().value); show(html); })
    .catch((x) => show(`<p class="note">${esc(String(x))}${String(x).includes("Key") ? tr("（设置 → 地图）") : ""}</p>`));
}
const TL = {
  tab: (() => { try { return localStorage.getItem("tl-tab") || "all"; } catch (e) { return "all"; } })(),
  view: (() => { try { return localStorage.getItem("today-view") || "overview"; } catch (e) { return "overview"; } })(),
  pview: (() => { try { return localStorage.getItem("place-view") || "stays"; } catch (e) { return "stays"; } })(),
  jump: null,
};
const localHour = (iso) => +hm(iso).slice(0, 2);
/** 系统界面这类不是「在用的 app」，不画图标 */
const NOT_APPS = new Set(["com.apple.dock", "com.apple.loginwindow", "com.apple.notificationcenterui", "com.apple.controlcenter", "com.apple.Spotlight", "com.apple.systemuiserver", "com.apple.WindowManager", "org.p0deje.Maccy"]);

/** 这天有摘要的电脑、设备页里改过的名字：时间线每行标上是哪台 */
let DAY_COMPUTERS = [], DAY_NAMES = {};
/** 设备 → 标签：改过名用改的名；没改过 mac-… 叫 Mac、win-… 叫 Windows，同一种有两台时带上机器名 */
function devLabel(dev) {
  if (DAY_NAMES[dev]) return DAY_NAMES[dev];
  const kind = (x) => (x.startsWith("win-") ? "Windows" : x.startsWith("mac-") ? "Mac" : x);
  const k = kind(dev || "");
  return DAY_COMPUTERS.filter((x) => kind(x) === k).length > 1 ? `${k} · ${dev.replace(/^(mac|win)-/, "")}` : k;
}

/** 手机页签：按停留点分组，新的在上。每组一行标题（地点、起止、待了多久），这段时间里拿手机的记录排在它下面；
 *  两个停留点之间的空档也成一组（路上／没记到位置），没有拿手机就不列 */
function phoneByPlace(items, visits, date) {
  const ses = items.filter((x) => !x.visit);
  const blocks = placeBlocks(visits, date).map((b) => ({ ...b, items: [] }));
  for (const x of ses) {
    const t = Date.parse(x.start);
    const b = blocks.find((y) => t >= y.a && t < y.z) || (t < blocks[0].a ? blocks[0] : blocks[blocks.length - 1]);
    b.items.push(x);
  }
  return blocks.reverse().filter((b) => b.kind === "stay" || b.items.length).map((b) => {
    const h = `data-h="${new Date(b.a).getHours()}"`;
    const head = b.kind === "stay"
      ? `<div class="tl-place" ${h}><span class="t">${visitSpan(b.v)}</span><span class="n"><i></i>${esc(placeName(b.v))}</span><span class="d">${spanText((b.z - b.a) / 1000)}${b.items.some((x) => x.ses) ? ` · ${tr("手机 {d}", { d: minText(b.items.reduce((n, x) => n + (x.ses ? x.ses.sec : 0), 0)) })}` : ""}</span></div>`
      : `<div class="tl-place gap" ${h}><span class="t">${hmMs(b.a)}–${hmMs(b.z)}</span><span class="n">${b.kind === "go" ? tr("路上") : tr("没记到位置")}</span><span class="d">${spanText((b.z - b.a) / 1000)}</span></div>`;
    return head + b.items.sort((x, y) => (x.start < y.start ? 1 : -1)).map(tlRow).join("");
  }).join("");
}

function timelineItems(d) {
  DAY_COMPUTERS = d.computers || [];
  DAY_NAMES = d.device_names || {};
  const out = [];
  for (const s of d.segments || []) {
    if (s.source === "raw") s.title = rawTitle(s.raw_apps);
    out.push({ dev: "computer", start: s.start, end: s.end, seg: s });
  }
  for (const x of (d.phone && d.phone.sessions) || []) out.push({ dev: "phone", start: x.start, end: x.end, ses: x });
  for (const v of (d.places && d.places.visits) || []) out.push({ dev: "phone", start: v.start, end: v.end, visit: v });
  SELF = (d.speech && d.speech.self) || null;
  // 对话按录它的设备归页签：手表录的在「手表」，iPhone 自己录的在「手机」
  for (const c of (d.speech && d.speech.conversations) || []) out.push({ dev: (c.device || "").startsWith("iphone-") ? "phone" : "watch", start: c.start, end: c.end, conv: c });
  return out.sort((a, b) => (a.start < b.start ? 1 : a.start > b.start ? -1 : 0));
}

/** 没有摘要的 10 分钟：用了哪些 app、各多久（不满 1 分钟的不写） */
function rawTitle(apps) {
  const xs = (apps || []).filter((a) => !NOT_APPS.has(a.bundle_id) && a.sec >= 60).slice(0, 3);
  return xs.length ? xs.map((a) => `${a.name} ${tr("{n} 分钟", { n: Math.round(a.sec / 60) })}`).join(" · ") : tr("零碎地用了一会儿");
}

const minText = (sec) => (sec < 60 ? tr("{n} 秒", { n: sec }) : tr("{n} 分钟", { n: Math.round(sec / 60) }));

function icons(ids, phone, names = {}) {
  const list = ids.filter((b) => b && !NOT_APPS.has(b)).slice(0, 10);
  if (!list.length) return "";
  return `<div class="apps">${list.map((b) => {
    const name = names[b] || b.split(".").pop();
    return `<span class="ico" data-b="${esc(b)}" data-p="${phone ? 1 : 0}" title="${esc(name)}">${esc((name[0] || "?").toUpperCase())}</span>`;
  }).join("")}</div>`;
}

/** 本人在转写里的名字（录音服务器按声纹认出的那个） */
let SELF = null;

/** 一段对话：标题行写时长和谁在说，第一句做预览，点开看原文 */
function convRow(c, h, t) {
  const who = speakers(c.talk || [], SELF);
  const sec = (c.talk || []).reduce((a, x) => a + x.sec, 0);
  const first = (c.lines || []).find((l) => l.text.length >= 4) || (c.lines || [])[0];
  const lines = (c.lines || []).map((l) => `<li><span class="ut">${hm(l.start)}${new Date(l.start).getSeconds() ? ":" + pad2(new Date(l.start).getSeconds()) : ":00"}</span><span class="who${l.speaker === SELF ? " self" : ""}">${esc(l.speaker)}</span><span>${esc(l.text)}</span></li>`).join("");
  return `<details class="tl-row watch" ${h}><summary>${t}<span><span class="name">${tr("对话 · {d}", { d: talkText(sec) })}</span><div class="desc">${esc(who.slice(0, 4).join(tr("、")))}${who.length > 4 ? ` ${tr("等 {n} 人", { n: who.length })}` : ""}</div>${first ? `<div class="talk-line">${tr("{who}：{text}", { who: esc(first.speaker), text: esc(first.text.length > 60 ? first.text.slice(0, 60) + "…" : first.text) })}</div>` : ""}</span><span class="chev">${ICON.chevR}</span></summary>
    <div class="more"><ul class="utts">${lines}</ul><p class="note" style="margin:0">${tr("手表录音转写，可能有错字。「{self}」按声纹认出，「他人N」只在当天内是同一个人，「未辨」是太短认不出。", { self: esc(SELF || tr("本人")) })}</p></div></details>`;
}

function tlRow(x) {
  const t = `<span class="t">${hm(x.start)}–${x.end ? hm(x.end) : x.visit && !ongoing(x.visit) ? "24:00" : tr("现在")}</span>`;
  const h = `data-h="${localHour(x.start)}"`;
  if (x.conv) return convRow(x.conv, h, t);
  if (x.visit) return `<div class="tl-row phone place" ${h}>${t}<span><span class="name">${tr("在 {p}", { p: esc(x.visit.name || tr("没有地名的地方")) })}</span></span></div>`;
  if (x.dev === "phone") {
    const s = x.ses;
    const names = Object.fromEntries(s.apps.map((a) => [a.bundle_id, a.name]));
    if (s.kind === "brief") return `<div class="tl-row phone brief" ${h}>${t}<span><span class="name">${tr("手机 · 拿起 {n} 次，共 {d}", { n: s.count, d: minText(s.sec) })}</span>${icons(s.apps.map((a) => a.bundle_id), true, names)}</span></div>`;
    const top = s.apps.filter((a) => a.sec >= 5).slice(0, 4).map((a) => `${esc(a.name)} ${minText(a.sec)}`).join(tr("、"));
    return `<div class="tl-row phone" ${h}>${t}<span><span class="name">${tr("手机 · {d}", { d: minText(s.sec) })}</span>${top ? `<div class="desc">${top}</div>` : ""}${icons(s.apps.map((a) => a.bundle_id), true, names)}</span></div>`;
  }
  const s = x.seg;
  // 连续的「不在电脑前」已由 Rust 合成一段：一行灰字，不能展开
  if (s.kind === "idle") return `<div class="tl-row computer idle" ${h}>${t}<span>${esc(tr(s.title))}</span></div>`;
  const ctx = (s.context || []).filter((y) => !y.includes("/Users/"));
  const more = [
    ctx.length ? `<div><p class="more-k">${tr("涉及")}</p>${md(ctx.map((y) => "- " + y).join("\n"))}</div>` : "",
    s.recording ? `<div><p class="more-k">${tr("经过")}</p>${md(s.recording)}</div>` : s.memory ? `<div><p class="more-k">${tr("经过")}</p>${md(s.memory)}</div>` : "",
  ].join("");
  const tag = (s.kind === "6h" ? `<span class="mx-tag">${tr("6 小时")}</span>` : s.source === "codex" ? `<span class="mx-tag">Codex</span>` : "")
    + (s.device ? `<span class="mx-tag">${esc(devLabel(s.device))}</span>` : "");
  return `<details class="tl-row computer" ${h}><summary>${t}<span><span class="name">${esc(s.title || tr("（没有标题）"))}${tag}</span>${s.description ? `<div class="desc">${esc(s.description)}</div>` : ""}${icons(s.apps || [], false)}</span>${more ? `<span class="chev">${ICON.chevR}</span>` : "<span></span>"}</summary>${more ? `<div class="more">${more}</div>` : ""}</details>`;
}

/** 先画首字母，图标取到后换上（手机 app 第一次要联网查，之后走缓存） */
const ICONS = { 0: {}, 1: {} };
async function loadIcons() {
  for (const p of [0, 1]) {
    const els = [...$app.querySelectorAll(`.ico[data-p="${p}"]`)];
    const need = [...new Set(els.map((e) => e.dataset.b))].filter((b) => !(b in ICONS[p]));
    if (need.length) Object.assign(ICONS[p], await call("get_icons", { ids: need, phone: !!p }).catch(() => ({})));
    for (const e of $app.querySelectorAll(`.ico[data-p="${p}"]`)) {
      const got = ICONS[p][e.dataset.b];
      // 名字表里没有的 app，用 App Store 上的名字
      if (got && got.name && e.title === e.dataset.b.split(".").pop()) e.title = got.name;
      if (got && got.icon && !e.querySelector("img")) { e.textContent = ""; e.classList.add("has-img"); const img = new Image(); img.src = got.icon; img.alt = ""; e.appendChild(img); }
    }
  }
}

// ---------- 日总结 ----------

const SUM = { err: null };

/** 日／周／月 页签。总结都在电脑上写，手机上看同步过来的 */
const summaryTabs = (on) => `<div class="tabs" role="tablist">${[["day", tr("日")], ["week", tr("周")], ["month", tr("月")]].map(([k, v]) =>
  `<button role="tab" aria-selected="${on === k}" class="${on === k ? "is-on" : ""}" data-tab="${k}">${v}</button>`).join("")}</div>`;
function bindTabs(date) {
  for (const b of document.querySelectorAll("[data-tab]")) b.onclick = () => { location.hash = b.dataset.tab === "day" ? `#/summary/${date}` : `#/summary/${b.dataset.tab}/${date}`; };
}
/** 手机不写总结（电脑端为准），只看电脑同步来的：不放「现在生成」「重新生成」 */
const canWrite = () => !(S.status && S.status.mobile);
/** 总结末尾一行：口径说明（由 Beayy 用……生成，数据截至……）＋「重新生成」 */
const sumFoot = (note, id) => `<p class="note sum-foot">${esc(trMsg(note || ""))}${canWrite() ? `<button class="mx-btn mx-btn--text mx-btn--sm" id="${id}">${tr("重新生成")}</button>` : ""}</p>`;
/** 还没有总结：一句说明＋「现在生成」（手机上只有说明）。说明写清会自动写，按钮只是不想等时用，免得读成两件事 */
const sumEmpty = (title, text, id, err) => section(title, emptyBlock("notebook", canWrite() ? tr("{text}，也可以现在生成", { text }) : tr("总结在电脑上写，写好会同步过来"),
  canWrite() ? `<button class="mx-btn mx-btn--secondary" id="${id}">${tr("现在生成")}</button>` : "") + (err ? `<div class="mx-field-error">${esc(err)}</div>` : ""));
/** 一句话（蜡笔横线）＋点评 */
const sumLead = (sum, lineTitle) => {
  const one = sum.parts.find((p) => p.title === lineTitle), rm = sum.parts.find((p) => p.title === "点评");
  return (one ? `<p class="lead"><span class="crayon">“${esc(one.body.replace(/^[“"]|[”"]$/g, ""))}”</span></p>` : "") + (rm && rm.body ? remarkHtml(rm.body) : "");
};
/** 生成按钮：点了先变「生成中…」，写完重画这一页 */
function bindGen(id, cmd, date, redraw) {
  const b = document.getElementById(id);
  if (b) b.onclick = async () => {
    b.disabled = true; b.textContent = tr("生成中…"); SUM.err = null;
    try { await call(cmd, { date }); } catch (x) { SUM.err = String(x); }
    redraw();
  };
}

/** 周、月总结的读数：每天平均（有记录的日子才算进分母），悬停写合计 */
function rangeReadouts(days) {
  const avg = (k) => { const xs = days.map((d) => d[k]).filter((v) => v != null); return xs.length ? [xs.reduce((a, b) => a + b, 0) / xs.length, xs.reduce((a, b) => a + b, 0), xs.length] : null; };
  const c = avg("active_sec"), p = avg("phone_sec"), k = avg("pickups"), n = avg("night_sec"), sl = avg("sleep_sec"), st = avg("steps");
  return `<div class="readouts">
    ${readout(tr("日均电脑"), c ? hh(c[0]) : "—", "h", c ? tr("共 {h} 小时 · {n} 天有记录", { h: hh(c[1]), n: c[2] }) : tr("取不到"))}
    ${readout(tr("日均手机"), p ? hh(p[0]) : "—", "h", p ? tr("共 {h} 小时 · {n} 天有记录", { h: hh(p[1]), n: p[2] }) : tr("取不到"))}
    ${readout(tr("日均拿起"), k ? Math.round(k[0]) : "—", tr("次"), k ? tr("共 {n} 次", { n: numText(k[1]) }) : "", false, tr("按前台空档推算"))}
    ${readout(tr("日均熬夜"), n ? Math.round(n[0] / 60) : "—", tr("分钟"), n ? tr("每晚 21:00–5:00 · 共 {n} 分钟", { n: numText(Math.round(n[1] / 60)) }) : "21:00–5:00", n && n[0] / 60 > 10)}
    ${sl ? readout(tr("日均睡眠"), hh(sl[0]), "h", tr("{n} 晚有记录", { n: sl[2] })) : ""}
    ${st ? readout(tr("日均步数"), numText(Math.round(st[0])), tr("步"), tr("共 {n} 步", { n: numText(st[1]) })) : ""}
  </div>`;
}

async function renderSummary(arg, arg2) {
  const tk = RT.seq;
  if (arg === "week") return renderWeek(arg2);
  if (arg === "month") return renderMonth(arg2);
  const dates = S.days.map((x) => x.date);
  const date = dates.includes(arg) ? arg : dates[0];
  if (!date) { if (stale(tk)) return; $app.innerHTML = header(tr("总结")) + emptyInline(tr("还没有日数据。")); return; }
  markNav("summary");
  const i = dates.indexOf(date);
  const d = await call("get_day", { date });
  const idx = S.days[i];
  const pack = (d && d.pack) || {}, ph = (d && d.phone) || null;
  // 还没有数据包（通常是今天）：电脑读数用自采事件实时算的
  const c = pack.computer && pack.computer.active_sec != null && !pack.partial ? pack.computer : (d && d.live) || pack.computer || {};
  const pp = pack.phone || {};
  // 熬夜＝这一晚：当天 21:00 以后＋次日 0–5 点（规格/日总结.md：凌晨 0–5 点属于前一晚），电脑和手机分别算再相加
  const nd = addDays(date, 1);
  const n = dates.includes(nd) ? await call("get_day", { date: nd }) : null;
  const npack = (n && n.pack) || {};
  const lateC = c.late_sec ?? secsAfter((d && d.live && d.live.spans) || [], date, 21);
  const lateP = ph ? ph.late_sec || 0 : pp.late_sec || 0;
  const dawnP = n ? (n.phone ? n.phone.night_sec || 0 : (npack.phone || {}).dawn_sec || 0) : 0;
  const dawnC = n ? hourSecs((n.live && n.live.active_hours) ? n.live : npack.computer).slice(0, 5).reduce((a, b) => a + b, 0) : 0;
  const night = Math.round((lateC + lateP + dawnP + dawnC) / 60);
  const hasNight = c.late_sec != null || (d && d.live) || ph || pp.late_sec != null;
  const sum = d && d.summary_md ? splitSummary(d.summary_md) : null;
  // 没写成的原因（模型出错、这一天没记录）：自动重试前先让人看到
  const failed = sum || !TAURI || !canWrite() ? null : await call("daily_status", { date }).catch(() => null);
  const isToday = date === localDay();

  // 翻日期和今天页同一个组件（‹ 日期 ▾ ›，以前的日子左边多一个「今天」）
  const right = dayPicker(date, !isToday);
  // 正文：模型写的一句话作导语，下面是 做成了什么、偏离、明天；日总结里写死的「时间都去哪了」不显示，
  // 换成和今天页一样的「地点」「时间」（实时算，类型名跟着现在的分类）
  const visits = (d && d.places && d.places.visits) || [];
  const parts = sum ? sum.parts.filter((p) => p.title !== "时间都去哪了") : [];
  const one = parts.find((p) => p.title === "今天的一句话");
  const remark = parts.find((p) => p.title === "点评");
  const storyParts = parts.filter((p) => p !== one && p !== remark && p.body);

  // 布局照 Claude「自定义」页：标题单独一行；下面一行左边 日／周／月 页签（月后续做），右边翻日期
  const kinds = summaryTabs("day");
  if (stale(tk)) return; $app.innerHTML = header(tr("总结")) + `<div class="toolbar">${kinds}<div class="toolbar-right">${right}</div></div>` + `<div class="reading">
    <div class="readouts">
      ${readout(tr("在电脑前"), hh(c.active_sec ?? idx.active_sec), "h", c.first ? `${c.first}–${c.last}` : tr("取不到"))}
      ${readout(tr("手机"), hh(ph ? ph.total_sec : pp.total_sec ?? idx.phone_sec), "h", ph ? tr("同步至 {t}", { t: ymdhm(ph.synced_until) }) : pp.synced_at ? tr("同步至 {t}", { t: pp.synced_at }) : tr("取不到"))}
      ${readout(tr("拿起"), ph ? ph.pickups : pp.pickups ?? "—", tr("次"), "", false, tr("按前台空档推算"))}
      ${readout(tr("熬夜"), hasNight ? night : "—", tr("分钟"), "21:00–5:00", night > 10)}
    </div>
    ${one ? `<p class="lead"><span class="crayon">“${esc(one.body.replace(/^[“"]|[”"]$/g, ""))}”</span></p>` : ""}
    ${remark && remark.body ? remarkHtml(remark.body) : ""}
    <div class="secs-plain" style="margin-top:var(--space-6)">
      ${sum ? story(storyParts) + sumFoot(sum.note, "gen-daily")
            : sumEmpty(tr("这一天"), isToday ? tr("还没有日总结，21:30 自动生成") : failed ? tr("日总结没写成，稍后自动重试") : tr("还没有日总结，稍后自动补上"), "gen-daily", SUM.err || (failed && failed.error))}
      ${placeTotals(visits, date)}
      ${workSection(d && d.work)}
    </div></div>`;

  SUM.err = null;
  bindGen("gen-daily", "generate_daily", date, () => renderSummary(date));
  bindTabs(date);
  bindPick(document.getElementById("day-pick"));
  $app.querySelectorAll("[data-day]").forEach((b) => (b.onclick = () => { location.hash = `#/summary/${b.dataset.day}`; }));
}

async function renderWeek(arg) {
  const tk = RT.seq;
  markNav("summary");
  const date = /^\d{4}-\d{2}-\d{2}$/.test(arg || "") ? arg : localDay();
  const w = await call("get_week", { date });
  const r = await call("get_range", { from: w.monday, to: w.sunday }).catch(() => null);
  const prev = addDays(w.monday, -7), next = addDays(w.monday, 7);
  const done = w.sunday < localDay();
  const sum = w.summary_md ? splitSummary(w.summary_md) : null;
  // 翻周和今天页的翻日期同一个样子：‹ 起止 ›，不是这一周时左边多一个「本周」
  const thisWeek = w.monday <= localDay() && localDay() <= w.sunday;
  const right = `${thisWeek ? "" : `<button class="mx-btn mx-btn--secondary dn-today" data-week="${localDay()}">${tr("本周")}</button>`}
    <div class="daynav"><button class="dn-step" aria-label="${tr("前一周")}" title="${tr("前一周")}" data-week="${prev}">${ICON.chevL}</button>
      <span class="dn-date">${w.monday.slice(5)}–${w.sunday.slice(5)}</span>
      <button class="dn-step" aria-label="${tr("后一周")}" title="${tr("后一周")}" ${next > localDay() ? "disabled" : `data-week="${next}"`}>${ICON.chevR}</button></div>`;
  const days = `<div class="week-days">${w.days.map((x) => `<a href="#/summary/${x.date}"><span class="mono">${short(x.date)}</span><span class="${x.line ? "" : "muted"}">${esc(x.line || tr("没有日总结"))}</span></a>`).join("")}</div>`;
  const err = SUM.err || w.error;
  // 布局同日总结：读数（这周每天平均）、一句话、点评、叙述段，再是这周的地点、时间
  if (stale(tk)) return; $app.innerHTML = header(tr("总结")) + `<div class="toolbar">${summaryTabs("week")}<div class="toolbar-right">${right}</div></div><div class="reading">
    ${r ? rangeReadouts(r.days) : ""}
    ${sum ? sumLead(sum, "这周的一句话") : ""}
    <div class="secs-plain" style="margin-top:var(--space-6)">
      ${sum ? story(sum.parts.filter((p) => p.title !== "这周的一句话" && p.title !== "点评" && p.body)) + sumFoot(sum.note, "gen-weekly")
            : sumEmpty(tr("这一周"), done ? tr("还没有周总结，稍后自动补上") : tr("这周还没过完，过完自动生成"), "gen-weekly", err)}
      ${r ? placeTotalsDays(r.days) : ""}
      ${r ? workSection(r.work) : ""}
      ${section(tr("每天"), days)}
    </div></div>`;
  SUM.err = null;
  bindTabs(date);
  bindGen("gen-weekly", "generate_weekly", date, () => renderWeek(date));
  $app.querySelectorAll("[data-week]").forEach((b) => (b.onclick = () => { location.hash = `#/summary/week/${b.dataset.week}`; }));
}

/** 月总结：和周总结同一个样子，底下列这个月的几周（点进周总结） */
async function renderMonth(arg) {
  const tk = RT.seq;
  markNav("summary");
  const date = /^\d{4}-\d{2}-\d{2}$/.test(arg || "") ? arg : localDay();
  const m = await call("get_month", { date });
  const r = await call("get_range", { from: m.first, to: m.last }).catch(() => null);
  const prev = addDays(m.first, -1).slice(0, 8) + "01", next = addDays(m.last, 1);
  const done = m.last < localDay();
  const sum = m.summary_md ? splitSummary(m.summary_md) : null;
  const thisMonth = localDay().slice(0, 7) === m.first.slice(0, 7);
  const label = LANG === "zh" ? `${+m.first.slice(0, 4)} 年 ${+m.first.slice(5, 7)} 月` : new Date(+m.first.slice(0, 4), +m.first.slice(5, 7) - 1, 1).toLocaleString("en-US", { month: "short", year: "numeric" });
  const right = `${thisMonth ? "" : `<button class="mx-btn mx-btn--secondary dn-today" data-month="${localDay()}">${tr("本月")}</button>`}
    <div class="daynav"><button class="dn-step" aria-label="${tr("前一个月")}" title="${tr("前一个月")}" data-month="${prev}">${ICON.chevL}</button>
      <span class="dn-date">${label}</span>
      <button class="dn-step" aria-label="${tr("后一个月")}" title="${tr("后一个月")}" ${next > localDay() ? "disabled" : `data-month="${next}"`}>${ICON.chevR}</button></div>`;
  const weeks = `<div class="week-days">${m.weeks.map((x) => `<a href="#/summary/week/${x.monday}"><span class="mono">${x.monday.slice(5)}–${x.sunday.slice(5)}</span><span class="${x.line ? "" : "muted"}">${esc(x.line || tr("没有周总结"))}</span></a>`).join("")}</div>`;
  const err = SUM.err || m.error;
  if (stale(tk)) return; $app.innerHTML = header(tr("总结")) + `<div class="toolbar">${summaryTabs("month")}<div class="toolbar-right">${right}</div></div><div class="reading">
    ${r ? rangeReadouts(r.days) : ""}
    ${sum ? sumLead(sum, "这个月的一句话") : ""}
    <div class="secs-plain" style="margin-top:var(--space-6)">
      ${sum ? story(sum.parts.filter((p) => p.title !== "这个月的一句话" && p.title !== "点评" && p.body)) + sumFoot(sum.note, "gen-monthly")
            : sumEmpty(tr("这个月"), done ? tr("还没有月总结，稍后自动补上") : tr("这个月还没过完，过完自动生成"), "gen-monthly", err)}
      ${r ? placeTotalsDays(r.days) : ""}
      ${r ? workSection(r.work) : ""}
      ${section(tr("每周"), weeks)}
    </div></div>`;
  SUM.err = null;
  bindTabs(date);
  bindGen("gen-monthly", "generate_monthly", date, () => renderMonth(date));
  $app.querySelectorAll("[data-month]").forEach((b) => (b.onclick = () => { location.hash = `#/summary/month/${b.dataset.month}`; }));
}

// ---------- 收支：导入的微信、支付宝账单，在上面看钱（app/src-tauri/src/bills.rs、money.rs；方案 报告/方案_20261104_金钱管理.md） ----------

// 页签、方向、搜索、筛选、排序都只在这一页的内存里，换月不丢
const MN = { tab: "overview", view: "rank", dir: "out", side: "merchant", q: "", qOpen: false, f: { src: "", ttype: "", ctype: "", account: "", channel: "" }, day: "", sort: "time-desc", all: false, open: null, month: null, data: null, ov: null, busy: null, err: null };
const yuan = (f) => (f / 100).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
/** 读数的大数字取整元（精确的放悬停） */
const yuanRound = (f) => Math.round(f / 100).toLocaleString("en-US");
const sharePct = (x) => (x == null ? "—" : `${Math.round(x * 100)}%`);
const BILL_SRC = { alipay: tr("支付宝"), wechat: tr("微信") };
const BILL_BY = { manual: tr("你改的"), bill: tr("账单上的"), rule: tr("你定的"), keyword: tr("按名字认的"), ai: tr("AI 分的") };
const BILL_FILTERS = [["src", tr("来源")], ["ttype", tr("交易类型")], ["ctype", tr("消费类型")], ["account", tr("付款账户")], ["channel", tr("渠道")]];
const BILL_SORTS = [["time-desc", tr("时间：新的在前")], ["time-asc", tr("时间：旧的在前")], ["amt-desc", tr("金额：大的在前")], ["amt-asc", tr("金额：小的在前")]];
const MI_SORT = lu('<path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/>');
const MI_FILTER = '<svg class="mx-ico" viewBox="0 0 16 16"><path d="M2 4h12M2 8h12M2 12h12"/><circle cx="5" cy="4" r="1.5" style="fill:var(--page)"/><circle cx="11" cy="8" r="1.5" style="fill:var(--page)"/><circle cx="7" cy="12" r="1.5" style="fill:var(--page)"/></svg>';
const MI_SEARCH = '<svg class="mx-ico" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5 14 14"/></svg>';
const MI_PLUS = '<svg class="mx-ico" viewBox="0 0 16 16"><path d="M8 3v10M3 8h10"/></svg>';
const monthLabel = (m) => (m.length === 4 ? (LANG === "zh" ? `${m} 全年` : m) : LANG === "zh" ? `${+m.slice(0, 4)} 年 ${+m.slice(5, 7)} 月` : new Date(+m.slice(0, 4), +m.slice(5, 7) - 1, 1).toLocaleString("en-US", { month: "short", year: "numeric" }));
const monthShort = (m) => (m.length === 4 ? monthLabel(m) : m.slice(0, 4) === localDay().slice(0, 4) ? (LANG === "zh" ? `${+m.slice(5)} 月` : new Date(2000, +m.slice(5) - 1, 1).toLocaleString("en-US", { month: "short" })) : monthLabel(m));
const billVal = (b, k) => (k === "ctype" ? (b.ttype === "消费" ? b.ctype || "未分类" : "") : b[k]) || "";
const billValLabel = (k, v) => (k === "src" ? BILL_SRC[v] || v : ["ttype", "ctype"].includes(k) ? tr(v) : v || tr("未写"));
const billType = (b) => (b.ttype === "消费" ? (b.ctype ? tr(b.ctype) : tr("消费 · 未分类")) : tr(b.ttype));

/** 怎么从微信、支付宝导出账单 */
function billHowtoDialog() {
  openDialog({ title: tr("导出账单"), cancel: tr("关闭"), body: `<ol class="steps">
      <li>${tr("微信：我 → 服务 → 钱包 → 账单 → 下载账单 → 用于个人对账")}</li>
      <li>${tr("支付宝：我的 → 账单 → 右上角「…」→ 开具交易流水证明 → 用于个人对账")}</li>
      <li>${tr("账单发到邮箱，是加密的压缩包，密码在微信支付、支付宝发给你的消息里")}</li>
      <li>${tr("解压后把 csv、xlsx 拖进这个窗口，或点「导入账单」")}</li></ol>` });
}

async function importBills(paths) {
  if (!paths || !paths.length) return;
  const r = await call("import_bills", { paths });
  const line = (f) => f.error ? `<li><b>${esc(f.name)}</b><br><span class="bill-err">${esc(f.error)}</span></li>`
    : `<li><b>${esc(f.name)}</b><br>${esc(tr("{src} · {n} 笔，新增 {a}，更新 {u}", { src: tr(f.format), n: f.rows, a: f.added, u: f.updated }))}</li>`;
  openDialog({ title: tr("导入账单"), cancel: tr("关闭"), body: `<ul class="steps">${r.files.map(line).join("")}</ul>${r.error ? `<p class="bill-err">${esc(r.error)}</p>` : ""}` });
  if (S.page === "money") renderMoney(MN.month);
}

async function pickBills() {
  importBills(await call("pick_bill_files"));
}

// 账单文件拖进窗口就导入（Tauri 的拖放事件给的是本机路径）
if (TAURI && window.__TAURI__.event) window.__TAURI__.event.listen("tauri://drag-drop", (e) => {
  const paths = ((e.payload && e.payload.paths) || []).filter((p) => /\.(csv|xlsx|zip)$/i.test(p));
  if (paths.length && S.page === "money") importBills(paths);
});

/** 选一项的小下拉（弹层里用）：按钮写当前值，点开是选项 */
const pickMenu = (id, cur, opts) => `<div class="pick"><button type="button" class="mx-btn mx-btn--secondary bill-pickbtn" id="${id}" aria-haspopup="menu" aria-expanded="false"><span>${esc(tr(cur))}</span>${ICON.chevD}</button>
  <div class="mx-menu" role="menu" hidden>${opts.map((o) => `<button type="button" role="menuitemradio" aria-checked="${o === cur}" data-pv="${esc(o)}">${esc(tr(o))}${o === cur ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div>`;

/** 一笔的详情：要紧的几项、改交易类型和消费类型、原表格的全部列 */
async function billDialog(id) {
  const b = await call("get_bill", { id });
  if (!b) return;
  const d = MN.data || {};
  const sign = b.dir === "out" ? "−" : b.dir === "in" ? "+" : "";
  const by = (k) => (BILL_BY[k] ? `<span class="bill-by">${esc(BILL_BY[k])}</span>` : "");
  const facts = [[tr("说明"), b.item], [tr("备注"), b.note], [tr("时间"), b.time], [tr("金额"), `${sign}${yuan(b.fen)}`], [tr("收/支"), b.dir === "out" ? tr("支出") : b.dir === "in" ? tr("收入") : tr("不计收支")], [tr("状态"), b.status],
    [tr("来源"), BILL_SRC[b.src] || b.src], [tr("付款账户"), [b.account, b.discount].filter(Boolean).join(" · ")], [tr("渠道"), b.channel], [tr("已退"), b.refund_fen ? yuan(b.refund_fen) : ""], [tr("服务费"), b.fee ? yuan(b.fee) : ""]]
    .filter(([, v]) => v);
  const raw = Object.entries(b.raw || {});
  const st = { ttype: b.ttype, ctype: b.ctype || "", remember: false };
  const manual = b.ttype_by === "manual" || b.ctype_by === "manual";
  const editor = `<div class="bill-edit">
      <div class="mx-field"><label>${tr("交易类型")} ${by(b.ttype_by)}</label>${pickMenu("bd-t", st.ttype, d.ttypes || [st.ttype])}</div>
      <div class="mx-field" id="bd-cf"${st.ttype === "消费" ? "" : " hidden"}><label>${tr("消费类型")} ${by(b.ctype_by)}</label>${pickMenu("bd-c", st.ctype || "未分类", ["未分类", ...(d.ctypes || [])])}</div>
      ${b.counterparty ? `<div class="mx-row bill-remember"><span class="mx-row-main"><span class="mx-row-name">${esc(tr("以后「{who}」都这样", { who: b.counterparty }))}</span></span><button type="button" class="mx-switch" role="switch" aria-checked="${b.remembered}" id="bd-r" aria-label="${tr("以后同一个对方都这样")}"></button></div>` : ""}
      ${manual ? `<button type="button" class="mx-btn mx-btn--text mx-btn--sm" id="bd-reset">${tr("恢复成自动认的")}</button>` : ""}
    </div>`;
  st.remember = !!b.remembered;
  openDialog({ title: b.counterparty || b.item || tr("这一笔"), ok: tr("保存"), busy: tr("保存中…"), body:
    `<dl class="mx-facts bill-facts">${facts.map(([k, v]) => `<div${k === tr("说明") || k === tr("备注") ? ' class="is-wide"' : ""}><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join("")}</dl>
     ${b.is_refund ? "" : editor}
     ${raw.length ? `<div class="mx-menu-group bill-raw-h">${tr("原表格")}</div><dl class="bill-raw">${raw.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}</dl>` : ""}`,
    onOpen: (el) => {
      for (const [bid, key] of [["bd-t", "ttype"], ["bd-c", "ctype"]]) {
        const btn = el.querySelector("#" + bid); if (!btn) continue;
        bindPick(btn);
        btn.parentElement.querySelector(".mx-menu").onclick = (e) => {
          const o = e.target.closest("[data-pv]"); if (!o) return;
          st[key] = o.dataset.pv === "未分类" ? "" : o.dataset.pv;
          btn.querySelector("span").textContent = tr(o.dataset.pv);
          o.parentElement.hidden = true;
          o.parentElement.querySelectorAll("[data-pv]").forEach((x) => { x.setAttribute("aria-checked", String(x === o)); const c = x.querySelector(".mx-check"); if (c) c.remove(); });
          o.insertAdjacentHTML("beforeend", `<span class="mx-check">${ICON.check}</span>`);
          if (key === "ttype") el.querySelector("#bd-cf").hidden = st.ttype !== "消费";
        };
      }
      const sw = el.querySelector("#bd-r");
      if (sw) sw.onclick = () => { st.remember = !st.remember; sw.setAttribute("aria-checked", String(st.remember)); };
      const reset = el.querySelector("#bd-reset");
      if (reset) reset.onclick = async () => { await call("set_bill_class", { id, ttype: null, ctype: null, remember: st.remember }); closeDialog(); renderMoney(MN.month); };
    },
    submit: async () => {
      if (b.is_refund) return;
      const ttype = st.ttype !== b.ttype || b.ttype_by === "manual" ? st.ttype : null;
      const ctype = st.ttype === "消费" && (st.ctype !== (b.ctype || "") || b.ctype_by === "manual") ? st.ctype || null : null;
      await call("set_bill_class", { id, ttype, ctype, remember: st.remember });
      renderMoney(MN.month);
    } });
}

/** 一份导入的账单：表头、表尾写的（账户、起止、共几笔、合计）、对账、和别的账单重合几笔、这次导入的结果；可以删 */
function importDialog(x) {
  const c = x.check || null;
  const chk = c ? `<dl class="mx-facts bill-facts">${[[tr("账单写的收入"), `${c.bill_n_in} · ${yuan(c.bill_in)}`], [tr("算出的收入"), `${c.n_in} · ${yuan(c.ours_in)}`], [tr("账单写的支出"), `${c.bill_n_out} · ${yuan(c.bill_out)}`], [tr("算出的支出"), `${c.n_out} · ${yuan(c.ours_out)}`]]
    .map(([k, v]) => `<div><dt>${esc(k)}</dt><dd class="mx-num">${esc(v)}</dd></div>`).join("")}</dl>` : "";
  openDialog({ title: x.file, cancel: tr("关闭"), body:
    `<dl class="mx-facts bill-facts">${[[tr("账单"), tr(x.format)], [tr("笔数"), numText(x.rows)], [tr("新增"), numText(x.added)], [tr("更新"), numText(x.updated)],
      [tr("第一笔"), x.first], [tr("最后一笔"), x.last], [tr("导出时间"), x.export], [tr("导入时间"), x.imported_at]].map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v || "—")}</dd></div>`).join("")}</dl>
     ${c ? `<div class="mx-menu-group bill-raw-h">${c.ok ? tr("对账：对上了") : tr("对账：没对上")}</div>${chk}` : ""}
     ${x.overlap ? `<div class="mx-menu-group bill-raw-h">${tr("和别的账单重合")}</div><dl class="mx-facts bill-facts">${[...x.overlap.map((o) => [`${tr(o.format)} · ${o.export.slice(0, 16)}`, tr("{n} 笔", { n: numText(o.n) })]), [tr("只在这份里"), tr("{n} 笔", { n: numText(x.only) })]]
        .map(([k, v]) => `<div><dt>${esc(k)}</dt><dd class="mx-num">${esc(v)}</dd></div>`).join("")}</dl>` : ""}
     ${x.meta && x.meta.length ? `<div class="mx-menu-group bill-raw-h">${tr("账单上写的")}</div><ul class="bill-meta">${x.meta.map((m) => `<li>${esc(m)}</li>`).join("")}</ul>` : ""}
     <div class="bill-del" id="imp-del">${impDelBody(x, false)}</div>`,
    onOpen: (el, dlg) => {
      const box = el.querySelector("#imp-del");
      box.onclick = async (e) => {
        const b = e.target.closest("button"); if (!b) return;
        if (b.dataset.del === "ask") { box.innerHTML = impDelBody(x, true); return; }
        if (b.dataset.del === "keep") { box.innerHTML = impDelBody(x, false); return; }
        if (b.dataset.del === "go") {
          b.disabled = true; dlg.setErr(null);
          const r = await call("delete_bill_import", { format: x.format, export: x.export }).catch((err) => ({ error: String(err && err.message ? err.message : err) }));
          if (r && r.error) { b.disabled = false; return dlg.setErr(r.error); }
          closeDialog(); renderMoney(MN.month);
        }
      };
    } });
}

/** 删一份账单：先问一句；只在这份里的那些笔会一起没了 */
const impDelBody = (x, asking) => asking
  ? `<div class="mx-confirm"><div class="mx-confirm-text"><b>${tr("删除这份账单？")}</b><span>${x.only === 0 ? tr("这份里的每一笔别的账单都有，删了不少记录。") : x.only != null ? esc(tr("只在这份里的 {n} 笔会一起删掉。", { n: numText(x.only) })) : tr("只在这份里的那些笔会一起删掉。")}</span></div><button class="mx-btn mx-btn--secondary" data-del="keep">${tr("取消")}</button><button class="mx-btn mx-btn--danger" data-del="go">${tr("删除")}</button></div>`
  : `<button class="mx-btn mx-btn--text mx-btn--sm bill-del-btn" data-del="ask">${tr("删除这份账单")}</button>`;

/** 这段时间里筛过的：搜索、筛选、某一天；dir 是收支方向（null＝不看方向） */
function billRows(d, dir = MN.dir) {
  const q = MN.q.trim().toLowerCase();
  return d.items.filter((b) => {
    if (dir && dir !== "all" && !(b.counted && b.dir === dir)) return false;
    for (const [k] of BILL_FILTERS) if (MN.f[k] && billVal(b, k) !== MN.f[k]) return false;
    if (MN.day && !b.time.startsWith(MN.day)) return false;
    if (q && ![b.counterparty, b.item, b.note, b.ctype, b.ttype, b.account, b.status].some((x) => (x || "").toLowerCase().includes(q))) return false;
    return true;
  });
}

function billAmount(b) {
  const sign = b.dir === "out" ? "−" : b.dir === "in" ? "+" : "";
  const full = b.refund_fen && b.refund_fen >= b.fen, part = b.refund_fen && !full;
  const tip = full ? tr("已全额退款") : part ? tr("已退 {v} 元", { v: yuan(b.refund_fen) }) : "";
  return `<td class="is-num${full ? " bill-void" : ""}"${tip ? ` title="${esc(tip)}"` : ""}>${sign}${yuan(b.fen)}${part ? `<small>${esc(tr("退 {v}", { v: yuan(b.refund_fen) }))}</small>` : ""}</td>`;
}

const billMore = (n, shown, unit) => (n > shown ? `<button class="mx-btn mx-btn--secondary mx-btn--sm bills-more" data-more="1">${tr(unit, { n })}</button>` : "");

// ---------- 概览 ----------

// 消费类型的颜色：后端按全部账单挑的消费最多的 5 类（ov.palette）用地点那 6 色里的 5 个，别的是灰色的「其余」；换月、换图颜色不变
const ctTone = (ov, name) => (ov.palette || []).indexOf(name) + 1;
const ctColor = (ov, name) => `var(--mn-${ctTone(ov, name) || "rest"})`;
/** 大数字缩写（柱顶）：一万以上写「8.6 万」 */
const yuanShort = (f) => (f >= 1e6 ? (LANG === "zh" ? `${(f / 1e6).toFixed(1)} 万` : `${Math.round(f / 1e5)}k`) : yuanRound(f));
/** 悬停里的一段：合计＋每类（多的在前） */
const ctTip = (head, total, by, extra = "") => [head, `${tr("消费")} ${yuan(total)}`, ...Object.entries(by || {}).sort((a, b) => b[1] - a[1]).map(([k, v]) => `  ${tr(k || "未分类")} ${yuan(v)}`), extra].filter(Boolean).join("\n");

/** 一排竖柱，每根按消费类型堆起来（颜色排前的类在下，灰色的「其余」在上）。
 *  一两根特别高的会把别的压扁：纵轴最高取有数的柱的中位数的 3 倍（最多截两根），超出的整根按比例压到顶（各类的比例不变）、画断口、顶上写金额 */
function stackCols(ov, cols, cls, dense = false) {
  const vals = cols.map((c) => c.total).filter((v) => v > 0).sort((a, b) => a - b);
  const real = Math.max(1, ...cols.map((c) => c.total)), mid = vals.length ? vals[Math.floor(vals.length / 2)] : 1;
  const max = vals.length >= 4 && real > mid * 3 ? Math.max(mid * 3, vals[vals.length - 3]) : real;
  const pal = ov.palette || [];
  return `<div class="mn-cols ${cls}">${cols.map((c, i) => {
    const parts = [...pal.map((n) => [ctColor(ov, n), (c.by || {})[n] || 0]), ["var(--mn-rest)", Object.entries(c.by || {}).filter(([k]) => !pal.includes(k)).reduce((a, [, v]) => a + v, 0)]];
    const k = Math.min(c.total, max) / Math.max(c.total, 1) / max;
    const segs = parts.map(([color, v]) => (v > 0 ? `<i style="height:${(v * k * 100).toFixed(2)}%;background:${color}"></i>` : "")).join("");
    // 窄柱挨着的两根都截断，金额字会叠在一起：后一根不写（悬停里有）
    const cut = c.total > max, label = cut && !(dense && i > 0 && cols[i - 1].total > max);
    return `<button class="mn-col${c.cls ? " " + c.cls : ""}" ${c.attrs || ""} title="${esc(c.tip)}">${label ? `<span class="mn-cut mx-num">${esc(yuanShort(c.total))}</span>` : ""}<span class="mn-stack${cut ? " is-cut" : ""}">${segs}</span><span class="mn-l">${c.label}</span></button>`;
  }).join("")}</div>`;
}

const ctLegend = (ov) => `<div class="legend mn-legend">${(ov.palette || []).map((n) => `<span><i style="background:${ctColor(ov, n)}"></i>${esc(tr(n))}</span>`).join("")}<span><i style="background:var(--mn-rest)"></i>${tr("其余")}</span></div>`;

/** 近 12 个月的消费（转账这类大额会把消费压扁，只画消费；支出、收入在悬停里），有账单以前的月份不画；点一根换到那个月 */
function trendChart(ov) {
  const first = ov.months[ov.months.length - 1] || "";
  const cols = ov.trend.filter((x) => x.month >= first).map((x) => ({
    total: x.consume, by: x.by, label: +x.month.slice(5), cls: x.month === ov.period ? "is-on" : "", attrs: x.has ? `data-month="${x.month}"` : "disabled",
    tip: ctTip(monthLabel(x.month), x.consume, x.by, `${tr("支出")} ${yuan(x.out)}\n${tr("收入")} ${yuan(x.income)}`) }));
  return stackCols(ov, cols, "mn-trend") + ctLegend(ov);
}

/** 这个月每天的消费：一天一根；点一根看那天的明细；账单还没到的日子空着 */
function dayChart(ov) {
  const m = ov.period, n = new Date(+m.slice(0, 4), +m.slice(5, 7), 0).getDate();
  const by = Object.fromEntries(ov.days.map((x) => [x.date, x]));
  const cols = Array.from({ length: n }, (_, i) => {
    const date = `${m}-${pad2(i + 1)}`, x = by[date], later = ov.upto && date > ov.upto;
    const cls = [later ? "is-later" : "", i === 0 || (i + 1) % 5 === 0 ? "is-tick" : ""].filter(Boolean).join(" ");
    if (later) return { total: 0, label: i + 1, cls, attrs: "disabled", tip: "" };
    return { total: x ? x.consume : 0, by: x && x.by, label: i + 1, cls, attrs: `data-day="${date}"`,
      tip: ctTip(`${date.slice(5)} ${weekday(date)}`, x ? x.consume : 0, x && x.by, x && x.out !== x.consume ? `${tr("支出")} ${yuan(x.out)}` : "") };
  });
  return stackCols(ov, cols, "mn-days", true) + ctLegend(ov);
}

/** 消费的占比：一个圆环（颜色同上，「其余」并成一块），中间写合计；右边一行一块 */
function ctDonut(ov) {
  const pal = ov.palette || [], tot = Math.max(1, ov.by_ctype.reduce((a, x) => a + x.fen, 0));
  const rest = ov.by_ctype.filter((x) => !pal.includes(x.name));
  const parts = [...ov.by_ctype.filter((x) => pal.includes(x.name)).map((x) => ({ name: x.name, fen: x.fen, color: ctColor(ov, x.name) })),
    ...(rest.length ? [{ name: null, fen: rest.reduce((a, x) => a + x.fen, 0), color: "var(--mn-rest)", members: rest }] : [])];
  const R = 80, r = 52, C = 84, pt = (rad, a) => `${(C + rad * Math.cos(a)).toFixed(2)} ${(C + rad * Math.sin(a)).toFixed(2)}`;
  let a0 = -Math.PI / 2;
  const label = (x) => (x.name == null ? tr("其余") : tr(x.name));
  const tipOf = (x) => `${label(x)} ${sharePct(x.fen / tot)} · ${yuan(x.fen)}${x.members ? "\n" + x.members.map((m) => `  ${tr(m.name || "未分类")} ${yuan(m.fen)}`).join("\n") : ""}`;
  const arcs = parts.map((x) => {
    const sweep = Math.min((x.fen / tot) * 2 * Math.PI, 2 * Math.PI - 1e-4), a1 = a0 + sweep, big = sweep > Math.PI ? 1 : 0;
    const d = `M${pt(R, a0)}A${R} ${R} 0 ${big} 1 ${pt(R, a1)}L${pt(r, a1)}A${r} ${r} 0 ${big} 0 ${pt(r, a0)}Z`;
    a0 = a1;
    return `<path d="${d}" style="fill:${x.color}"${x.name != null ? ` data-ctype="${esc(x.name || "未分类")}"` : ""}><title>${esc(tipOf(x))}</title></path>`;
  }).join("");
  const rows = parts.map((x) => `<${x.name != null ? `button data-ctype="${esc(x.name || "未分类")}"` : "div"} class="mn-dl-row" title="${esc(tipOf(x))}"><i style="background:${x.color}"></i><span class="mn-dl-name">${esc(label(x))}${x.members ? `<small>${esc(tr("{n} 类", { n: x.members.length }))}</small>` : ""}</span><span class="mx-num">${sharePct(x.fen / tot)}</span><span class="mx-num">${yuan(x.fen)}</span></${x.name != null ? "button" : "div"}>`).join("");
  return `<div class="mn-donut"><div class="mn-ring"><svg viewBox="0 0 ${C * 2} ${C * 2}" role="img" aria-label="${tr("消费的占比")}">${arcs}</svg><div class="mn-ring-c"><span>${tr("消费")}</span><b class="mx-num">${yuanRound(tot)}</b></div></div><div class="mn-dl">${rows}</div></div>`;
}

const billBar = (share, color) => `<span class="bill-bar"><i style="width:${Math.max(0, Math.min(100, share * 100)).toFixed(1)}%${color ? `;background:${color}` : ""}"></i></span>`;
/** 和前三个月平均比：翻倍以上写倍数（×3.6），否则 +62%／−22% */
const billVs = (x) => {
  if (x.change == null) return x.avg3 === 0 ? `<span class="bill-new">${tr("新")}</span>` : "—";
  const r = x.change + 1, text = r >= 2 ? `×${r >= 10 ? Math.round(r) : r.toFixed(1)}` : `${x.change >= 0 ? "+" : "−"}${Math.round(Math.abs(x.change) * 100)}%`;
  return `<span class="${x.change > 0.5 ? "bill-up" : ""}">${text}</span>`;
};

function overviewBody(ov) {
  const t = ov.totals, p = ov.prev;
  const ro = `<div class="readouts">
      ${readout(tr("支出"), yuanRound(t.out), tr("元"), yuan(t.out), false, [...ov.by_ttype.filter((x) => x.out).map((x) => `${tr(x.name)} ${yuan(x.out)}`), t.refund ? tr("已扣退款 {v} 元", { v: yuan(t.refund) }) : ""].filter(Boolean).join("\n"))}
      ${readout(tr("消费"), yuanRound(t.consume), tr("元"), yuan(t.consume), false, p ? (ov.upto ? tr("前三个月同期平均 {v} 元", { v: yuan(p.consume) }) : tr("前三个月平均 {v} 元", { v: yuan(p.consume) })) : "")}
      ${readout(tr("收入"), yuanRound(t.in), tr("元"), yuan(t.in))}
      ${readout(tr("结余"), (t.net < 0 ? "−" : "") + yuanRound(Math.abs(t.net)), tr("元"), yuan(t.net))}
      ${readout(tr("储蓄率"), t.rate == null ? "—" : Math.round(t.rate * 100), "%", "", false, tr("结余 ÷ 收入"))}
    </div>`;
  const ins = ov.is_month ? section(tr("这个月的钱"), ov.insight
      ? `<div class="story bill-insight">${md(ov.insight)}</div><p class="sum-foot"><button class="mx-btn mx-btn--text mx-btn--sm" id="ins-gen">${MN.busy === "insight" ? tr("在写…") : tr("重新写")}</button></p>`
      : `<div class="bill-insight-empty"><span>${tr("还没有这个月的总结")}</span><button class="mx-btn mx-btn--secondary mx-btn--sm" id="ins-gen"${MN.busy === "insight" ? " disabled" : ""}>${MN.busy === "insight" ? tr("在写…") : tr("现在写")}</button></div>`)
      + (MN.err ? `<p class="bill-err">${esc(MN.err)}</p>` : "") : "";
  const alerts = ov.alerts.length ? section(tr("值得注意的"), `<div class="bill-alerts">${ov.alerts.map((a, i) =>
    `<button class="bill-alert" data-alert="${i}"><span class="mx-status${a.level === "warn" ? " mx-status--warn" : ""}"></span><span>${esc(a.text)}</span>${a.action === "classify" ? `<span class="bill-alert-act">${MN.busy === "classify" ? tr("在分…") : tr("让 AI 分类")}</span>` : ""}</button>`).join("")}</div>`) : "";
  const trend = section(tr("近 12 个月的消费"), trendChart(ov));
  const days = ov.is_month && ov.days.length ? section(tr("每天的消费"), dayChart(ov)) : "";
  const tt = ov.by_ttype.length ? section(tr("钱怎么动的"), `<div class="mx-table-wrap"><table class="mx-table bills-to"><thead><tr><th>${tr("交易类型")}</th><th class="is-num">${tr("笔数")}</th><th class="is-num">${tr("支出")}</th><th class="is-num">${tr("收入")}</th></tr></thead><tbody>
      ${ov.by_ttype.map((x) => `<tr data-ttype="${esc(x.name)}"><td>${esc(tr(x.name))}</td><td class="is-num">${x.n}</td><td class="is-num">${x.out ? yuan(x.out) : "—"}</td><td class="is-num">${x.in ? yuan(x.in) : "—"}</td></tr>`).join("")}</tbody></table></div>`) : "";
  const ct = ov.by_ctype.length ? ctypeSection(ov, false) : "";
  const ac = ov.accounts.length ? section(tr("从哪个账户付的"), `<div class="mx-table-wrap"><table class="mx-table bills-to"><thead><tr><th>${tr("付款账户")}</th><th class="is-num">${tr("笔数")}</th><th class="is-num">${tr("金额")}</th></tr></thead><tbody>
      ${ov.accounts.map((x) => `<tr data-account="${esc(x.name)}"><td>${esc(x.name)}${x.credit ? `<span class="mx-tag">${tr("信用")}</span>` : ""}</td><td class="is-num">${x.n}</td><td class="is-num">${yuan(x.fen)}</td></tr>`).join("")}</tbody></table></div>`,
      { aside: ov.credit != null ? esc(tr("消费里信用付的占 {v}", { v: sharePct(ov.credit) })) : "" }) : "";
  const rc = ov.recurring.length ? section(tr("每月固定扣费"), `<div class="mx-table-wrap"><table class="mx-table bills-to"><thead><tr><th>${tr("对方")}</th><th class="is-num">${tr("月数")}</th><th class="is-num">${tr("每月约")}</th><th class="is-num">${tr("最近一个月")}</th></tr></thead><tbody>
      ${ov.recurring.map((x) => `<tr data-who="${esc(x.name)}"><td title="${esc(x.name)}">${esc(x.name)}${x.new ? `<span class="mx-tag">${tr("新")}</span>` : ""}${x.rise ? `<span class="mx-tag bill-tag-up">${tr("涨了")}</span>` : ""}</td><td class="is-num">${x.months}</td><td class="is-num">${yuan(x.avg)}</td><td class="is-num">${yuan(x.last)}</td></tr>`).join("")}</tbody></table></div>`) : "";
  const bg = ov.budgets.length ? section(tr("预算"), `<div class="bill-budgets">${ov.budgets.map((x) => `<div class="bill-budget"><span>${esc(x.text)}</span>${billBar(x.spent / x.limit)}<span class="mx-num${x.ok ? "" : " bill-up"}">${yuan(x.spent)} / ${yuan(x.limit)}</span></div>`).join("")}</div>`) : "";
  return ro + `<div class="secs-plain" style="margin-top:var(--space-6)">${ins}${trend}${ct}${days}${alerts}${tt}${ac}${rc}${bg}</div>`;
}

/** 「消费花在哪」（概览、分类页共用）：排行（表）和占比（圆环）两种看法，切换只在这一页的内存里 */
const ctypeSection = (ov, expand) => section(tr("消费花在哪"), MN.view === "share" ? ctDonut(ov) : ctypeTable(ov, expand),
  { aside: `<span class="mx-seg mn-view" role="tablist">${[["rank", tr("排行")], ["share", tr("占比")]].map(([k, v]) => `<button role="tab" aria-selected="${k === MN.view}" class="${k === MN.view ? "is-on" : ""}" data-view="${k}">${v}</button>`).join("")}</span>` });

/** 消费类型那张表；分类页点一行展开近 12 个月和主要商户 */
function ctypeTable(ov, expand) {
  const rows = ov.by_ctype.map((x) => {
    const name = x.name || "未分类", open = expand && MN.open === name;
    let detail = "";
    if (open) {
      const mx = Math.max(1, ...x.series);
      const spark = `<div class="bill-spark" style="--c:${ctColor(ov, x.name)}">${x.series.map((v, i) => `<i style="height:${Math.max(2, (v / mx) * 44)}px" title="${esc(`${monthLabel(ov.trend[i].month)} ${yuan(v)}`)}"></i>`).join("")}</div>`;
      const m = new Map();
      for (const b of (MN.data ? MN.data.items : []).filter((b) => b.counted && b.ttype === "消费" && (b.ctype || "未分类") === name)) {
        const w = b.counterparty || b.item; const e = m.get(w) || [0, 0]; e[0] += b.fen - (b.refund_fen || 0); e[1] += 1; m.set(w, e);
      }
      const top = [...m.entries()].sort((a, b) => b[1][0] - a[1][0]).slice(0, 6);
      detail = `<tr class="bill-open"><td colspan="5"><div class="bill-open-in">${spark}<div class="bill-open-top">${top.map(([w, [f, n]]) => `<button data-who="${esc(w)}"><span>${esc(w)}</span><span class="mx-num">${n} · ${yuan(f)}</span></button>`).join("")}
        <button class="mx-btn mx-btn--text mx-btn--sm" data-ctype-list="${esc(name)}">${tr("看这类的明细")}</button></div></div></td></tr>`;
    }
    return `<tr data-ctype="${esc(name)}"${open ? ' class="is-open"' : ""}><td>${esc(tr(name))}</td><td class="bill-bar-td">${billBar(x.share, ctColor(ov, x.name))}</td><td class="is-num">${sharePct(x.share)}</td><td class="is-num">${ov.is_month ? billVs(x) : x.n}</td><td class="is-num">${yuan(x.fen)}</td></tr>${detail}`;
  }).join("");
  return `<div class="mx-table-wrap"><table class="mx-table bills-ct"><thead><tr><th>${tr("消费类型")}</th><th class="b-bar"></th><th class="is-num">${tr("占比")}</th><th class="is-num"${ov.upto ? ` title="${esc(tr("每月 1 日到 {d} 日", { d: +ov.upto.slice(8) }))}"` : ""}>${ov.is_month ? (ov.upto ? tr("比前三个月同期") : tr("比前三个月")) : tr("笔数")}</th><th class="is-num">${tr("金额")}</th></tr></thead><tbody>${rows}</tbody></table></div>`;
}

// ---------- 明细、分类、对方、导入 ----------

function billListBody(d) {
  const rows = billRows(d);
  const [k, way] = MN.sort.split("-"), w = way === "asc" ? 1 : -1;
  rows.sort((a, b) => (k === "amt" ? (a.fen - b.fen) * w || b.time.localeCompare(a.time) : a.time.localeCompare(b.time) * w));
  const shown = MN.all ? rows : rows.slice(0, 100);
  const xs = billRows(d, null).filter((b) => b.counted);
  const out = xs.filter((b) => b.dir === "out").reduce((a, b) => a + b.fen - (b.refund_fen || 0), 0), inc = xs.filter((b) => b.dir === "in").reduce((a, b) => a + b.fen, 0);
  const sum = `${tr("支出 {a} · 收入 {b} · {n} 笔", { a: yuan(out), b: yuan(inc), n: rows.length })}`;
  const table = rows.length ? `<div class="mx-table-wrap"><table class="mx-table bills"><thead><tr><th class="b-time">${tr("时间")}</th><th>${tr("对方")}</th><th>${tr("说明")}</th><th class="b-cat">${tr("类型")}</th><th class="b-method">${tr("付款账户")}</th><th class="b-src">${tr("来源")}</th><th class="is-num b-amt">${tr("金额")}</th></tr></thead><tbody>
      ${shown.map((b) => `<tr data-bill="${esc(b.id)}"${b.counted ? "" : ' class="bill-muted"'}><td class="mx-num">${esc(b.time.slice(5, 16))}</td><td title="${esc(b.counterparty)}">${esc(b.counterparty)}</td><td title="${esc(b.item)}">${esc(b.item)}</td><td class="${b.ttype === "消费" && !b.ctype ? "bill-dim" : ""}" title="${esc(BILL_BY[b.ctype_by] || BILL_BY[b.ttype_by] || "")}">${esc(billType(b))}</td><td title="${esc(b.account)}">${esc(b.account)}</td><td>${esc(BILL_SRC[b.src] || b.src)}</td>${billAmount(b)}</tr>`).join("")}</tbody></table></div>
      ${billMore(rows.length, shown.length, "显示全部 {n} 笔")}`
    : emptyInline(MN.q || MN.day || Object.values(MN.f).some(Boolean) ? tr("没有符合条件的") : tr("这段时间没有记录"));
  const cover = Object.entries(d.coverage).map(([s, c]) => tr("{src}至 {d}", { src: BILL_SRC[s] || s, d: c.last.slice(0, 10) })).join(" · ");
  return `<p class="bill-sum mx-num">${esc(sum)}</p>` + section(tr("明细"), table, { aside: esc(cover) });
}

function billCtypeBody(ov, d) {
  const n = d.unclassified;
  const notice = n ? `<div class="mx-notice"><span>${esc(tr("还有 {n} 笔消费没分好类", { n }))}</span><button class="mx-btn mx-btn--secondary mx-btn--sm" id="ai-cls" title="${tr("只发对方名和说明")}"${MN.busy === "classify" ? " disabled" : ""}>${MN.busy === "classify" ? tr("在分…") : tr("让 AI 分类")}</button></div>` : "";
  return notice + (MN.err ? `<p class="bill-err">${esc(MN.err)}</p>` : "") + (ov.by_ctype.length ? ctypeSection(ov, true) : emptyInline(tr("这段时间没有消费")));
}

function billWhoBody(d) {
  const people = MN.side === "person";
  const m = new Map();
  for (const b of billRows(d, null).filter((b) => b.counted && (people ? ["转账", "红包", "亲友代付"].includes(b.ttype) : b.ttype === "消费"))) {
    const w = b.who || b.counterparty || b.item || tr("未写对方");
    const e = m.get(w) || { name: w, out: 0, in: 0, n: 0, ct: new Map(), last: "" };
    e.out += b.dir === "out" ? b.fen - (b.refund_fen || 0) : 0; e.in += b.dir === "in" ? b.fen : 0; e.n += 1; e.last = e.last > b.time ? e.last : b.time;
    if (b.ctype) e.ct.set(b.ctype, (e.ct.get(b.ctype) || 0) + 1);
    m.set(w, e);
  }
  const rows = [...m.values()].sort((a, b) => b.out + b.in - (a.out + a.in));
  const shown = MN.all ? rows : rows.slice(0, 50);
  if (!rows.length) return emptyInline(tr("没有符合条件的"));
  const head = people ? `<th>${tr("对方")}</th><th class="is-num">${tr("笔数")}</th><th class="is-num">${tr("给出")}</th><th class="is-num">${tr("收到")}</th><th class="is-num">${tr("净额")}</th>`
    : `<th>${tr("对方")}</th><th class="b-cat">${tr("消费类型")}</th><th class="is-num">${tr("笔数")}</th><th class="b-time">${tr("最近")}</th><th class="is-num">${tr("金额")}</th>`;
  const row = (x) => people
    ? `<tr data-who="${esc(x.name)}"><td title="${esc(x.name)}">${esc(x.name)}</td><td class="is-num">${x.n}</td><td class="is-num">${x.out ? yuan(x.out) : "—"}</td><td class="is-num">${x.in ? yuan(x.in) : "—"}</td><td class="is-num">${(x.in - x.out < 0 ? "−" : "+") + yuan(Math.abs(x.in - x.out))}</td></tr>`
    : `<tr data-who="${esc(x.name)}"><td title="${esc(x.name)}">${esc(x.name)}</td><td>${esc(tr([...x.ct.entries()].sort((a, b) => b[1] - a[1]).map((e) => e[0])[0] || "未分类"))}</td><td class="is-num">${x.n}</td><td class="mx-num">${esc(x.last.slice(5, 10))}</td><td class="is-num">${yuan(x.out)}</td></tr>`;
  return section(people ? tr("和人的来往") : tr("商户"), `<div class="mx-table-wrap"><table class="mx-table bills-who"><thead><tr>${head}</tr></thead><tbody>${shown.map(row).join("")}</tbody></table></div>${billMore(rows.length, shown.length, "显示全部 {n} 个")}`);
}

function billImportsBody(d) {
  const xs = d.imports || [];
  const cover = Object.entries(d.coverage).map(([s, c]) => `<div><dt>${esc(BILL_SRC[s] || s)}</dt><dd>${esc(tr("{a}–{b} · {n} 笔", { a: c.first.slice(0, 10), b: c.last.slice(0, 10), n: numText(c.count) }))}</dd></div>`).join("");
  const ok = (x) => !x.check || x.check.ok == null ? "—" : x.check.ok ? `<span class="mx-status mx-status--ok">${tr("对上了")}</span>` : `<span class="mx-status mx-status--warn">${esc(tr("差 {v} 元", { v: yuan(Math.abs(x.check.bill_out - x.check.ours_out) + Math.abs(x.check.bill_in - x.check.ours_in)) }))}</span>`;
  // 重合：这份里有几笔别的账单也有（同一笔按交易号认，只算一次）
  const dup = (x) => {
    if (!x.overlap) return "—";
    const n = x.rows - x.only;
    return n ? `<span title="${esc(x.overlap.map((o) => `${tr(o.format)} · ${o.export.slice(0, 16)}  ${tr("{n} 笔", { n: numText(o.n) })}`).join("\n"))}">${numText(n)}</span>` : "0";
  };
  const table = xs.length ? `<div class="mx-table-wrap"><table class="mx-table bill-imports"><thead><tr><th>${tr("账单")}</th><th class="b-range">${tr("账单里的时间")}</th><th class="is-num b-n">${tr("笔数")}</th><th class="is-num b-n">${tr("重合")}</th><th class="b-chk">${tr("对账")}</th><th class="b-at">${tr("导入时间")}</th></tr></thead><tbody>
      ${xs.map((x, i) => `<tr data-imp="${i}"><td title="${esc(x.file)}">${esc(tr(x.format))}</td><td class="mx-num">${esc(x.first.slice(0, 10))}–${esc(x.last.slice(0, 10))}</td><td class="is-num">${numText(x.rows)}</td><td class="is-num">${dup(x)}</td><td>${ok(x)}</td><td class="mx-num">${esc(x.imported_at.slice(0, 16))}</td></tr>`).join("")}</tbody></table></div>`
    : emptyInline(tr("之前导入的没有记下文件，再导一次就有"));
  const rows = xs.reduce((a, x) => a + x.rows, 0), uniq = Object.values(d.coverage).reduce((a, c) => a + c.count, 0);
  const aside = xs.length > 1 && rows > uniq ? esc(tr("{r} 行，去掉重合的是 {n} 笔", { r: numText(rows), n: numText(uniq) })) : "";
  const rebuild = xs.some((x) => x.stored) ? `<button class="mx-btn mx-btn--text mx-btn--sm" id="bill-rebuild"${MN.busy === "rebuild" ? " disabled" : ""}>${MN.busy === "rebuild" ? tr("在重算…") : tr("从原文件重算")}</button>` : "";
  return `${cover ? `<dl class="mx-facts bill-cover">${cover}</dl>` : ""}<div class="secs-plain" style="margin-top:var(--space-6)">${section(tr("导入过的账单"), table + `<p class="sum-foot">${rebuild}${howto("bills", tr("怎么导出账单"))}</p>`, { aside })}</div>`;
}

/** 筛选菜单：第一层一行一个维度（右边灰字是当前值），点进去是这个维度的选项 */
function billFilterMenu(d, dim) {
  if (!dim) {
    const on = BILL_FILTERS.some(([k]) => MN.f[k]);
    return BILL_FILTERS.map(([k, n]) => `<button role="menuitem" data-fdim="${k}">${n}<span class="mx-menu-val${MN.f[k] ? " is-changed" : ""}">${MN.f[k] ? esc(billValLabel(k, MN.f[k])) : tr("全部")}</span></button>`).join("")
      + (on ? `<hr><button role="menuitem" data-freset="1">${tr("重置")}</button>` : "");
  }
  const counts = new Map();
  for (const b of d.items) if (b.counted || MN.dir === "all") { const v = billVal(b, dim); if (v) counts.set(v, (counts.get(v) || 0) + 1); }
  const vals = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  const opt = (v, label) => `<button role="menuitemradio" aria-checked="${MN.f[dim] === v}" data-fval="${esc(v)}" data-fk="${dim}">${esc(label)}${MN.f[dim] === v ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`;
  return `<button role="menuitem" data-fback="1">${ICON.chevL}${esc(BILL_FILTERS.find(([k]) => k === dim)[1])}</button><hr>`
    + opt("", tr("全部")) + vals.map(([v, n]) => opt(v, `${billValLabel(dim, v)}  ${n}`)).join("");
}

/** 只重画正文（搜索、筛选、排序、换方向时），工具栏和搜索框的焦点不动 */
function drawBillBody() {
  const d = MN.data, ov = MN.ov; if (!d) return;
  const body = document.getElementById("bill-body"); if (!body) return;
  body.innerHTML = MN.tab === "overview" ? overviewBody(ov) : MN.tab === "ctype" ? billCtypeBody(ov, d) : MN.tab === "who" ? billWhoBody(d) : MN.tab === "imports" ? billImportsBody(d) : billListBody(d);
  const n = BILL_FILTERS.filter(([k]) => MN.f[k]).length;
  const badge = document.querySelector("#bill-filter .mx-badge");
  if (badge) { badge.textContent = n; badge.hidden = !n; }
  const chips = document.getElementById("bill-chips");
  if (chips) {
    const on = ["list", "who"].includes(MN.tab) ? BILL_FILTERS.filter(([k]) => MN.f[k]).map(([k, name]) => [k, `${name}：${billValLabel(k, MN.f[k])}`]) : [];
    if (MN.day && ["list", "who"].includes(MN.tab)) on.unshift(["day", `${MN.day.slice(5)} ${weekday(MN.day)}`]);
    chips.hidden = !on.length;
    chips.innerHTML = on.map(([k, text]) => `<button class="mx-chip" data-chip="${k}" title="${tr("去掉")}">${esc(text)}${ICON.close}</button>`).join("");
  }
}

/** 跳到明细，带上条件 */
function billGo(cond) {
  MN.tab = "list"; MN.all = false; MN.day = cond.day || ""; MN.q = cond.q || ""; MN.qOpen = !!MN.q;
  MN.f = { src: "", ttype: cond.ttype || "", ctype: cond.ctype || "", account: cond.account || "", channel: "" };
  if (cond.ttype || cond.ctype || cond.account) MN.dir = "all";
  renderMoney(MN.month);
}

async function billRun(kind, cmd, args = {}) {
  MN.busy = kind; MN.err = null; drawBillBody();
  try {
    const r = await call(cmd, args);
    if (r && r.error) MN.err = r.error;
    if (r && r.errors && r.errors.length) MN.err = r.errors.join("；");
  } catch (e) { MN.err = String(e && e.message ? e.message : e); }
  MN.busy = null;
  renderMoney(MN.month);
}

async function renderMoney(arg) {
  const tk = RT.seq;
  const want = /^\d{4}(-\d{2})?$/.test(arg || "") ? arg : MN.month;
  const [d, ov] = await Promise.all([call("get_bills", { month: want || null }), call("money_overview", { month: want || null })]);
  if (stale(tk)) return;
  MN.month = d.month; MN.data = d; MN.ov = ov;
  const has = Object.keys(d.coverage || {}).length > 0;
  if (!has) {
    $app.innerHTML = header(tr("收支")) + `<div class="reading">${emptyBlock("notebook", tr("导入微信、支付宝的账单，看每个月花在哪"), `<div class="empty-acts"><button class="mx-btn mx-btn--primary" id="bill-import">${tr("导入账单")}</button>${howto("bills", tr("怎么导出账单"))}</div>`)}</div>`;
    document.getElementById("bill-import").onclick = pickBills;
    return;
  }
  const tab = MN.tab;
  const periods = [];
  for (const y of d.years) { periods.push(y); for (const m of d.months.filter((m) => m.startsWith(y))) periods.push(m); }
  const tabs = `<div class="tabs" role="tablist" id="bill-tabs">${[["overview", tr("概览")], ["list", tr("明细")], ["ctype", tr("分类")], ["who", tr("对方")], ["imports", tr("导入")]].map(([k, v]) =>
    `<button role="tab" aria-selected="${k === tab}" class="${k === tab ? "is-on" : ""}" data-btab="${k}">${v}</button>`).join("")}</div>`;
  const segOf = (id, items, cur) => `<div class="mx-seg" role="tablist" id="${id}">${items.map(([k, v]) => `<button role="tab" aria-selected="${k === cur}" class="${k === cur ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div>`;
  const seg = tab === "list" ? segOf("bill-dir", [["out", tr("支出")], ["in", tr("收入")], ["all", tr("全部")]], MN.dir) : tab === "who" ? segOf("bill-side", [["merchant", tr("商户")], ["person", tr("个人")]], MN.side) : "";
  const period = `<div class="pick"><button class="mx-btn mx-btn--secondary bill-month" id="bill-month" aria-haspopup="menu" aria-expanded="false" title="${esc(monthLabel(d.month))}">${esc(monthShort(d.month))}${ICON.chevD}</button>
      <div class="mx-menu" role="menu" hidden>${periods.map((m) => `<button role="menuitemradio" aria-checked="${m === d.month}" data-month="${m}"${m.length === 4 ? ' class="bill-year"' : ""}>${esc(monthLabel(m))}${m === d.month ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div>`;
  const search = MN.qOpen || MN.q ? `<label class="mx-search bill-search">${MI_SEARCH}<input id="bill-q" placeholder="${tr("搜索对方、说明、备注")}" value="${esc(MN.q)}"></label>`
    : `<button class="mx-iconbtn" id="bill-qbtn" aria-label="${tr("搜索")}" title="${tr("搜索")}">${MI_SEARCH}</button>`;
  const filter = `<div class="pick"><button class="mx-iconbtn" id="bill-filter" aria-label="${tr("筛选")}" title="${tr("筛选")}" aria-haspopup="menu" aria-expanded="false">${MI_FILTER}<span class="mx-badge" hidden></span></button><div class="mx-menu" role="menu" id="bill-fmenu" hidden></div></div>`;
  const sort = `<div class="pick"><button class="mx-iconbtn" id="bill-sort" aria-label="${tr("排序")}" title="${tr("排序")}" aria-haspopup="menu" aria-expanded="false">${MI_SORT}</button>
      <div class="mx-menu" role="menu" hidden>${BILL_SORTS.map(([k, v]) => `<button role="menuitemradio" aria-checked="${k === MN.sort}" data-sort="${k}">${v}${k === MN.sort ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div>`;
  const addMenu = `<div class="pick bill-add"><button class="mx-btn mx-btn--primary" id="bill-add" aria-haspopup="menu" aria-expanded="false">${MI_PLUS}${tr("导入账单")}${ICON.chevD}</button>
      <div class="mx-menu" role="menu" hidden><button role="menuitem" data-add="pick">${tr("选择文件…")}</button><button role="menuitem" data-add="howto">${tr("怎么导出账单")}</button></div></div>`;
  const right = (tab === "imports" ? "" : period) + (["list", "who"].includes(tab) ? search : "") + (tab === "list" ? filter + sort : "") + addMenu;
  $app.innerHTML = header(tr("收支")) + `<div id="money"><div class="toolbar bill-toolbar"><div class="toolbar-left">${tabs}${seg ? `<span class="tb-sep"></span>${seg}` : ""}</div><div class="toolbar-right">${right}</div></div>
    <div class="mx-chips bill-chips" id="bill-chips" hidden></div><div class="reading" id="bill-body"></div></div>`;
  drawBillBody();

  for (const id of ["bill-add", "bill-sort", "bill-month"]) bindPick(document.getElementById(id));
  const fbtn = document.getElementById("bill-filter"), fmenu = document.getElementById("bill-fmenu");
  if (fbtn) fbtn.onclick = (e) => { e.stopPropagation(); fmenu.innerHTML = billFilterMenu(d); fmenu.hidden = !fmenu.hidden; fbtn.setAttribute("aria-expanded", String(!fmenu.hidden)); };
  const qbtn = document.getElementById("bill-qbtn");
  if (qbtn) qbtn.onclick = () => { MN.qOpen = true; renderMoney(d.month).then(() => { const q = document.getElementById("bill-q"); if (q) q.focus(); }); };
  const q = document.getElementById("bill-q");
  if (q) {
    q.oninput = () => { MN.q = q.value; MN.all = false; drawBillBody(); };
    q.onblur = () => { if (!MN.q) { MN.qOpen = false; setTimeout(() => { if (!MN.q && document.activeElement !== q && S.page === "money") renderMoney(d.month); }, 150); } };
  }
  document.getElementById("money").onclick = (e) => {
    const t = e.target;
    const at = (sel) => t.closest(sel);
    let x;
    if ((x = at("[data-add]"))) { x.parentElement.hidden = true; return x.dataset.add === "pick" ? pickBills() : billHowtoDialog(); }
    if ((x = at("[data-view]"))) { MN.view = x.dataset.view; return drawBillBody(); }
    if ((x = at(".mn-donut [data-ctype]"))) return billGo({ ctype: x.dataset.ctype });
    if ((x = at("[data-btab]"))) { MN.tab = x.dataset.btab; MN.all = false; MN.open = null; return renderMoney(d.month); }
    if ((x = at("#bill-dir [data-v]"))) { MN.dir = x.dataset.v; MN.all = false; return renderMoney(d.month); }
    if ((x = at("#bill-side [data-v]"))) { MN.side = x.dataset.v; MN.all = false; return renderMoney(d.month); }
    if ((x = at("[data-month]"))) { MN.all = false; MN.day = ""; location.hash = `#/money/${x.dataset.month}`; return; }
    if ((x = at("[data-sort]"))) { MN.sort = x.dataset.sort; return renderMoney(d.month); }
    if ((x = at("[data-fdim]"))) { e.stopPropagation(); fmenu.innerHTML = billFilterMenu(d, x.dataset.fdim); return; }
    if (at("[data-fback]")) { e.stopPropagation(); fmenu.innerHTML = billFilterMenu(d); return; }
    if ((x = at("[data-fval]"))) { MN.f[x.dataset.fk] = x.dataset.fval; MN.all = false; fmenu.hidden = true; return drawBillBody(); }
    if (at("[data-freset]")) { MN.f = { src: "", ttype: "", ctype: "", account: "", channel: "" }; fmenu.hidden = true; return drawBillBody(); }
    if ((x = at("[data-chip]"))) { if (x.dataset.chip === "day") MN.day = ""; else MN.f[x.dataset.chip] = ""; return drawBillBody(); }
    if (at("[data-more]")) { MN.all = true; return drawBillBody(); }
    if ((x = at("[data-bill]"))) return billDialog(x.dataset.bill);
    if ((x = at("[data-imp]"))) return importDialog(d.imports[+x.dataset.imp]);
    if ((x = at("[data-day]"))) return billGo({ day: x.dataset.day, ctype: "" });
    if ((x = at("[data-ctype-list]"))) return billGo({ ctype: x.dataset.ctypeList });
    if ((x = at("[data-who]"))) return billGo({ q: x.dataset.who });
    if ((x = at("[data-ttype]"))) return billGo({ ttype: x.dataset.ttype });
    if ((x = at("[data-account]"))) return billGo({ account: x.dataset.account });
    if ((x = at("tr[data-ctype]"))) {
      if (MN.tab === "ctype") { MN.open = MN.open === x.dataset.ctype ? null : x.dataset.ctype; return drawBillBody(); }
      return billGo({ ctype: x.dataset.ctype });
    }
    if ((x = at("[data-alert]"))) {
      const a = ov.alerts[+x.dataset.alert];
      if (a.action === "classify") return MN.busy ? null : billRun("classify", "classify_bills_ai");
      if (a.id) return billDialog(a.id);
      if (a.ctype) return billGo({ ctype: a.ctype });
      if (a.q) return billGo({ q: a.q });
      return;
    }
    if (at("#ins-gen")) return MN.busy ? null : billRun("insight", "generate_money_insight", { month: d.month });
    if (at("#ai-cls")) return MN.busy ? null : billRun("classify", "classify_bills_ai");
    if (at("#bill-rebuild")) return MN.busy ? null : billRun("rebuild", "rebuild_bills");
  };
}

// ---------- 游戏：英雄联盟、Steam（Windows 上 crates/beayy-games 记的，src-tauri/src/games.rs 汇总；方案 报告/方案_20261106_英雄联盟接入.md） ----------

/** game 空着＝让后台选最近玩过的那款 */
const GM = { game: null, tab: "overview", range: "30", data: null };
/** 支持的游戏（id → 名字）；再加一款在这里和 games.rs 各加一项 */
const GM_GAMES = { lol: "英雄联盟", steam: "Steam" };
/** 设置 → 功能 → 游戏下面每款的说明 */
const GM_DESC = { lol: "Windows 上读客户端，记每一局的战绩", steam: "Windows 上记在玩哪款、每次玩了多久" };
/** 开着的游戏（总开关开了以后，每款自己的开关默认开） */
const gmOn = () => Object.keys(GM_GAMES).filter((k) => S.settings && S.settings["games_" + k] !== false);
const GM_TIER = { IRON: "黑铁", BRONZE: "黄铜", SILVER: "白银", GOLD: "黄金", PLATINUM: "铂金", EMERALD: "翡翠", DIAMOND: "钻石", MASTER: "大师", GRANDMASTER: "宗师", CHALLENGER: "王者" };
const GM_QUEUE = { RANKED_SOLO_5x5: "单双排", RANKED_FLEX_SR: "灵活排位", RANKED_TFT: "云顶之弈", RANKED_TFT_DOUBLE_UP: "云顶双人" };
const GM_REGION = { HN1: "艾欧尼亚", HN10: "黑色玫瑰", BGP2: "峡谷之巅" };
const gmNum = (n) => Math.round(n || 0).toLocaleString("en-US");
const gmKda = (m) => `${m.k}/${m.d}/${m.a}`;
const gmRatio = (k, d, a) => ((k + a) / Math.max(d, 1)).toFixed(1);
const gmTier = (q) => (!q || !q.tier ? "—" : tr(GM_TIER[q.tier] || q.tier) + (["MASTER", "GRANDMASTER", "CHALLENGER"].includes(q.tier) || !q.division || q.division === "NA" ? "" : " " + q.division));
const gmResult = (m) => (m.remake ? tr("重开") : m.win === true ? tr("胜") : m.win === false ? tr("负") : "—");
const gmDay = (date) => (LANG === "zh" ? `${+date.slice(5, 7)} 月 ${+date.slice(8)} 日 ${weekday(date)}` : `${toDate(date).toLocaleDateString("en-US", { month: "short", day: "numeric" })} ${weekday(date)}`);
const gmDayOf = (iso) => localDay(new Date(iso));
const gmLp = (r) => (!r ? "" : r.lp != null ? (r.lp > 0 ? `+${r.lp}` : r.lp < 0 ? `−${-r.lp}` : "0") : `→ ${gmTier(r.to)}`);
const gmCounted = (ms) => ms.filter((m) => !m.remake && typeof m.win === "boolean");
const gmWinCls = (m) => (m.remake ? "is-remake" : m.win === true ? "is-win" : m.win === false ? "is-loss" : "");
const gmWho = (p) => `${esc(p.name || "—")}${p.tag ? `<span class="gm-tag">#${esc(p.tag)}</span>` : ""}`;

/** 英雄、装备、召唤师技能的图标（src-tauri/src/games.rs 的 game_icons，CommunityDragon 的公开图，取过的存在本机）：
 *  先画一个写着首字的方块，gmLoadIcons 取到了再换成图 */
const GMI = { champ: {}, item: {}, spell: {} };
function gmIc(kind, id, alt = "", cls = "") {
  if (!id) return `<span class="gm-ic gm-ic--${kind} is-empty ${cls}"></span>`;
  const u = GMI[kind][id];
  if (u) return `<img class="gm-ic gm-ic--${kind} ${cls}" src="${u}" alt="${esc(alt)}"${alt ? ` title="${esc(alt)}"` : ""}>`;
  return `<span class="gm-ic gm-ic--${kind} ${cls}"${u === undefined ? ` data-gic="${kind}:${id}"` : ""}${alt ? ` title="${esc(alt)}"` : ""}>${esc([...(alt || "")][0] || "")}</span>`;
}
async function gmLoadIcons(root) {
  if (!root) return;
  const want = { champ: new Set(), item: new Set(), spell: new Set() };
  root.querySelectorAll("[data-gic]").forEach((el) => { const [k, id] = el.dataset.gic.split(":"); if (GMI[k][id] === undefined) want[k].add(+id); });
  await Promise.all(Object.entries(want).filter(([, ids]) => ids.size).map(async ([k, ids]) => {
    const r = await call("game_icons", { kind: k, ids: [...ids] }).catch(() => ({}));
    for (const id of ids) GMI[k][id] = r[id] || null;
  }));
  root.querySelectorAll("[data-gic]").forEach((el) => {
    const [k, id] = el.dataset.gic.split(":"), u = GMI[k][id];
    el.removeAttribute("data-gic");
    if (!u) return;
    const img = document.createElement("img");
    img.className = el.className; img.src = u; img.alt = el.title || ""; if (el.title) img.title = el.title;
    el.replaceWith(img);
  });
}

/** 线条图标（lucide） */
const GM_SVG = {
  crown: '<path d="M11.562 3.266a.5.5 0 0 1 .876 0L15.39 8.87a1 1 0 0 0 1.516.294L21.183 5.5a.5.5 0 0 1 .798.519l-2.834 10.246a1 1 0 0 1-.956.734H5.81a1 1 0 0 1-.957-.734L2.02 6.02a.5.5 0 0 1 .798-.519l4.276 3.664a1 1 0 0 0 1.516-.294z"/><path d="M5 21h14"/>',
  flame: '<path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>',
  drop: '<path d="M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-5.5s-3.5-4-4-6.5c-.5 2.5-2 4.9-4 6.5C6 11.1 5 13 5 15a7 7 0 0 0 7 7z"/>',
  swords: '<polyline points="14.5 17.5 3 6 3 3 6 3 17.5 14.5"/><line x1="13" x2="19" y1="19" y2="13"/><line x1="16" x2="20" y1="16" y2="20"/><line x1="19" x2="21" y1="21" y2="19"/><polyline points="14.5 6.5 18 3 21 3 21 6 17.5 9.5"/><line x1="5" x2="9" y1="14" y2="18"/><line x1="7" x2="4" y1="17" y2="20"/><line x1="3" x2="5" y1="19" y2="21"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  coins: '<circle cx="8" cy="8" r="6"/><path d="M18.09 10.37A6 6 0 1 1 10.34 18"/><path d="M7 6h1v4"/><path d="m16.71 13.88.7.71-2.82 2.82"/>',
  eye: '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>',
  target: '<circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/>',
  users: '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
};
const gmI = (k) => lu(GM_SVG[k]);
/** 徽章：代码 → [文字, 图标]（games.rs 的 badges） */
const GM_BADGE = { mvp: ["MVP", "crown"], svp: ["SVP", "crown"], penta: ["五杀", "flame"], quadra: ["四杀", "flame"], triple: ["三杀", "flame"], fb: ["一血", "drop"], top_dmg: ["伤害最高", "swords"], top_tank: ["承伤最高", "shield"] };
const gmBadges = (bs) => (bs || []).filter((b) => GM_BADGE[b]).map((b) => `<span class="gm-badge gm-badge--${b}">${gmI(GM_BADGE[b][1])}${esc(tr(GM_BADGE[b][0]))}</span>`).join("");
/** 表头：图标＋字 */
const gmTh = (icon, text) => `${gmI(icon)}<span>${esc(text)}</span>`;

/** 选的这段时间里的局和天（近 7 天、近 30 天按本地日算，含今天） */
function gmView() {
  const d = GM.data;
  const cut = GM.range === "all" ? "" : localDay(new Date(Date.now() - (+GM.range - 1) * 86400000));
  return { cut, matches: d.matches.filter((m) => !cut || gmDayOf(m.start) >= cut), days: d.days.filter((x) => !cut || x.date >= cut) };
}

function gmChamps(ms) {
  const by = new Map();
  for (const m of gmCounted(ms)) {
    const c = by.get(m.champ_id) || { id: m.champ_id, name: m.champ, n: 0, w: 0, k: 0, d: 0, a: 0, dmg: 0, dur: 0, last: m.start };
    c.n++; c.w += m.win ? 1 : 0; c.k += m.k; c.d += m.d; c.a += m.a; c.dmg += m.dmg; c.dur += m.dur;
    if (m.start > c.last) c.last = m.start;
    by.set(m.champ_id, c);
  }
  return [...by.values()].sort((a, b) => b.n - a.n || b.w - a.w);
}

/** 一局里落在 23:00–5:00 的秒数（按开局那天的本地时间算） */
function gmNightSec(g) {
  const t = new Date(g.start), s = t.getHours() * 60 + t.getMinutes() + t.getSeconds() / 60, e = s + g.dur / 60;
  const ov = (a, b) => Math.max(0, Math.min(e, b) - Math.max(s, a));
  return (ov(0, 300) + ov(1380, 1740)) * 60;
}

/** 每天打了多久：一天一根柱，深夜那段用越界色。o.tip(d)＝那天提示里的局数、胜负几行，o.label＝图例 */
function gmDayChart(days, cut, o = {}) {
  const by = Object.fromEntries(days.map((x) => [x.date, x]));
  const today = localDay(), first = cut || (days.length ? days[days.length - 1].date : today);
  const dates = [];
  for (let t = toDate(first); localDay(t) <= today; t.setDate(t.getDate() + 1)) dates.push(localDay(t));
  const max = Math.max(3600, ...dates.map((x) => (by[x] || {}).play || 0));
  const every = dates.length > 14 ? 5 : 1;
  const cols = dates.map((x, i) => {
    const d = by[x], play = d ? d.play : 0, night = d ? Math.min(play, d.games.reduce((s, g) => s + gmNightSec(g), 0)) : 0;
    const head = o.tip ? o.tip(d) : d ? [tr("{n} 局", { n: d.n + d.remakes }), d.n ? tr("{w} 胜 {l} 负", { w: d.w, l: d.n - d.w }) : "", tr("打了 {t}", { t: durText(play) })] : [];
    const tip = d ? [gmDay(x), ...head, night ? tr("深夜 {t}", { t: durText(night) }) : ""].filter(Boolean).join("\n") : gmDay(x);
    // 柱子从下往上画：先是 23:00 以前的，深夜那段在上面
    const segs = (play - night > 0 ? `<i style="height:${((play - night) / max * 100).toFixed(2)}%;background:var(--rec-computer)"></i>` : "") + (night ? `<i style="height:${(night / max * 100).toFixed(2)}%;background:var(--rec-night)"></i>` : "");
    const tick = i % every === 0 || x === today;
    return `<button class="mn-col${tick ? " is-tick" : ""}" data-gday="${x}"${d ? "" : " disabled"} title="${esc(tip)}"><span class="mn-stack">${segs}</span><span class="mn-l">${tick ? +x.slice(8) : ""}</span></button>`;
  }).join("");
  return `<div class="mn-cols mn-days">${cols}</div><div class="legend mn-legend"><span><i style="background:var(--rec-computer)"></i>${o.label || tr("对局")}</span><span><i style="background:var(--rec-night)"></i>${tr("23:00 以后")}</span></div>`;
}

/** 什么时候玩：一天一行、横轴 24 小时，每局按开局和时长画一段；what(g)＝提示里最后那段（胜负、游戏名） */
function gmTimeline(days, what = gmResult) {
  const rows = days.filter((x) => x.games.length).slice(0, 14).map((x) => {
    const blocks = x.games.map((g) => {
      const t = new Date(g.start), s = (t.getHours() * 60 + t.getMinutes()) / 1440, w = Math.min(1 - s, g.dur / 86400);
      return `<i class="${gmNightSec(g) > g.dur / 2 ? "is-night" : ""}" style="left:${(s * 100).toFixed(2)}%;width:${(w * 100).toFixed(2)}%" title="${esc(`${hm(g.start)} · ${durText(g.dur)} · ${what(g)}`)}"></i>`;
    }).join("");
    return `<div class="gm-tl-row"><span class="gm-tl-d">${esc(gmDay(x.date))}</span><span class="gm-tl-track">${blocks}</span></div>`;
  }).join("");
  const axis = [0, 6, 12, 18, 24].map((h) => `<span style="left:${(h / 24 * 100).toFixed(2)}%">${pad2(h)}:00</span>`).join("");
  return `<div class="gm-tl">${rows}<div class="gm-tl-row gm-tl-axis"><span class="gm-tl-d"></span><span class="gm-tl-ticks">${axis}</span></div></div>`;
}

function gmRankTable(rank) {
  const qs = (rank && rank.queues) || [];
  if (!qs.length) return "";
  return section(tr("段位"), `<div class="mx-table-wrap"><table class="mx-table gm-t"><thead><tr><th>${tr("队列")}</th><th>${tr("段位")}</th><th class="is-num">${tr("胜点")}</th><th class="is-num">${tr("胜")}</th><th class="is-num">${tr("负")}</th><th class="is-num">${tr("胜率")}</th></tr></thead><tbody>
    ${qs.map((q) => `<tr><td>${esc(tr(GM_QUEUE[q.queue] || q.queue))}</td><td>${esc(gmTier(q))}</td><td class="is-num">${q.lp}</td><td class="is-num">${q.wins}</td><td class="is-num">${q.losses}</td><td class="is-num">${q.wins + q.losses ? sharePct(q.wins / (q.wins + q.losses)) : "—"}</td></tr>`).join("")}</tbody></table></div>`,
    { aside: esc(tr("{t} 更新", { t: `${gmDayOf(rank.at).slice(5)} ${hm(rank.at)}` })) });
}

function gmChampTable(cs, cls = "") {
  return `<div class="mx-table-wrap"><table class="mx-table gm-t gm-champs ${cls}"><thead><tr><th>${tr("英雄")}</th><th class="is-num g-n">${tr("局数")}</th><th class="is-num g-n">${tr("胜率")}</th><th class="is-num g-kda">${tr("场均 K/D/A")}</th><th class="is-num g-n">KDA</th><th class="is-num g-dmg">${tr("场均伤害")}</th><th class="is-num g-dur">${tr("场均时长")}</th><th class="is-num g-n">${tr("最近")}</th></tr></thead><tbody>
    ${cs.map((c) => `<tr data-gchamp="${c.id}"><td><span class="gm-cell">${gmIc("champ", c.id, c.name)}<span>${esc(c.name || `#${c.id}`)}</span></span></td><td class="is-num">${c.n}</td><td class="is-num">${sharePct(c.w / c.n)}</td><td class="is-num">${[c.k, c.d, c.a].map((v) => (v / c.n).toFixed(1)).join("/")}</td><td class="is-num">${gmRatio(c.k, c.d, c.a)}</td><td class="is-num">${gmNum(c.dmg / c.n)}</td><td class="is-num">${durText(c.dur / c.n)}</td><td class="is-num">${gmDayOf(c.last).slice(5)}</td></tr>`).join("")}</tbody></table></div>`;
}

function gmOverview(v) {
  const ms = gmCounted(v.matches), w = ms.filter((m) => m.win).length, remakes = v.matches.filter((m) => m.remake).length;
  const play = v.days.reduce((s, x) => s + x.play, 0), wait = v.days.reduce((s, x) => s + x.wait, 0);
  const night = v.matches.filter((m) => m.night).length;
  const ro = `<div class="readouts">
      ${readout(tr("局数"), ms.length, "", "", false, [tr("{w} 胜 {l} 负", { w, l: ms.length - w }), remakes ? tr("另有 {n} 局重开", { n: remakes }) : ""].filter(Boolean).join("\n"))}
      ${readout(tr("胜率"), ms.length ? Math.round(w / ms.length * 100) : "—", "%")}
      ${readout(tr("在打"), hh(play), tr("小时"), "", false, durText(play))}
      ${readout(tr("等待"), wait ? hh(wait) : "—", tr("小时"), "", false, tr("排队、选人、读条"))}
      ${readout(tr("深夜局"), night, tr("局"), "", night > 0, tr("23:00 到 5:00 开的局"))}
    </div>`;
  const champs = gmChamps(v.matches).slice(0, 5);
  return ro + `<div class="secs-plain" style="margin-top:var(--space-6)">
    ${section(tr("每天"), gmDayChart(v.days, v.cut))}
    ${v.days.some((x) => x.games.length) ? section(tr("什么时候玩"), gmTimeline(v.days)) : ""}
    ${champs.length ? section(tr("常玩英雄"), gmChampTable(champs)) : ""}
    ${gmFriends(v)}
    ${gmRankTable(GM.data.rank)}</div>`;
}

/** 常一起玩的：这段时间里同队打过 5 局以上的人（games.rs 的 friends） */
function gmFriends(v) {
  const by = new Map(v.matches.map((m) => [m.id, m]));
  const fs = (GM.data.friends || []).map((f) => {
    const ms = f.games.map((id) => by.get(id)).filter(Boolean);
    return { ...f, n: ms.length, w: ms.filter((m) => m.win).length, last: ms.map((m) => m.start).sort().pop() };
  }).filter((f) => f.n >= 5).sort((a, b) => b.n - a.n);
  if (!fs.length) return "";
  return section(tr("常一起玩的"), `<div class="mx-table-wrap"><table class="mx-table gm-t gm-friends"><thead><tr><th>${tr("玩家")}</th><th class="is-num g-n">${tr("一起打")}</th><th class="is-num g-n">${tr("胜率")}</th><th class="is-num g-n">${tr("最近")}</th></tr></thead><tbody>
    ${fs.map((f) => `<tr><td><span class="gm-cell">${gmI("users")}<span>${gmWho(f)}</span></span></td><td class="is-num">${tr("{n} 局", { n: f.n })}</td><td class="is-num">${sharePct(f.w / f.n)}</td><td class="is-num">${gmDayOf(f.last).slice(5)}</td></tr>`).join("")}</tbody></table></div>`,
    { aside: esc(tr("同队 5 局以上")) });
}

function gmList(v) {
  if (!v.matches.length) return emptyInline(tr("这段时间没打"));
  const ranked = v.matches.some((m) => m.rank);
  const groups = [];
  for (const m of v.matches) { const d = gmDayOf(m.start); if (!groups.length || groups[groups.length - 1][0] !== d) groups.push([d, []]); groups[groups.length - 1][1].push(m); }
  return `<div class="secs-plain">${groups.map(([d, ms]) => {
    const c = gmCounted(ms), w = c.filter((m) => m.win).length;
    return `<div id="gm-d-${d}">${section(gmDay(d), `<div class="gm-games">${ms.map((m) => `<button class="gm-row ${gmWinCls(m)}" data-game="${m.id}">
        <span class="gm-res"><b>${gmResult(m)}</b><small>${durText(m.dur)}</small></span>
        <span class="gm-pick">${gmIc("champ", m.champ_id, m.champ, "gm-ic--lg")}<span class="gm-sp">${(m.spells || []).map((id) => gmIc("spell", id)).join("")}</span></span>
        <span class="gm-what"><b>${esc(m.champ || `#${m.champ_id}`)}</b><small>${hm(m.start)} · ${esc(m.queue || "")}</small></span>
        <span class="gm-kda"><b>${gmKda(m)}</b><small>KDA ${gmRatio(m.k, m.d, m.a)}</small></span>
        <span class="gm-dmg"><b>${gmNum(m.dmg)}</b><small>${[m.dmg_share != null ? tr("伤害占比 {v}", { v: sharePct(m.dmg_share) }) : "", m.kp != null ? tr("参团 {v}", { v: sharePct(m.kp) }) : ""].filter(Boolean).join(" · ")}</small></span>
        <span class="gm-items">${(m.items || []).map((id, i) => gmIc("item", id, "", i === 6 ? "is-trinket" : "")).join("")}</span>
        <span class="gm-badges">${gmBadges(m.badges)}${ranked && m.rank ? `<span class="gm-lp">${esc(gmLp(m.rank))}</span>` : ""}</span>
      </button>`).join("")}</div>`,
      { aside: esc([tr("{n} 局", { n: ms.length }), c.length ? tr("{w} 胜 {l} 负", { w, l: c.length - w }) : ""].filter(Boolean).join(" · ")) })}</div>`;
  }).join("")}</div>`;
}

function gmChampsBody(v) {
  const cs = gmChamps(v.matches);
  return cs.length ? gmChampTable(cs) : emptyInline(tr("这段时间没打"));
}

// Steam：每次从开到关（steam.session）、游戏库里每款的总时长（steam.library）

const gmApp = (x) => x.name || `#${x.appid}`;

function gmSteamView() {
  const d = GM.data;
  const cut = GM.range === "all" ? "" : localDay(new Date(Date.now() - (+GM.range - 1) * 86400000));
  return { cut, sessions: d.sessions.filter((x) => !cut || gmDayOf(x.start) >= cut), days: d.days.filter((x) => !cut || x.date >= cut) };
}

/** 每款：这段时间玩了几次、多久、最近一次；累计是 Steam 自己记的总时长。选「全部」时没在 Beayy 里记过的也列上 */
function gmSteamApps(ss) {
  const lib = new Map((GM.data.library || []).map((a) => [a.appid, a]));
  const by = new Map();
  for (const x of ss) {
    const a = by.get(x.appid) || { appid: x.appid, name: x.name, n: 0, play: 0, last: x.start };
    a.n++; a.play += x.dur;
    if (x.start > a.last) a.last = x.start;
    by.set(x.appid, a);
  }
  if (GM.range === "all") for (const l of lib.values()) if (!by.has(l.appid)) by.set(l.appid, { appid: l.appid, name: l.name, n: 0, play: 0, last: l.last });
  return [...by.values()].map((a) => ({ ...a, total: lib.has(a.appid) ? lib.get(a.appid).total_min * 60 : null })).sort((a, b) => b.play - a.play || (b.total || 0) - (a.total || 0));
}

function gmSteamTable(apps) {
  return `<div class="mx-table-wrap"><table class="mx-table gm-t gm-apps"><thead><tr><th>${tr("游戏")}</th><th class="is-num g-n">${tr("启动")}</th><th class="is-num g-dur">${tr("时长")}</th><th class="is-num g-n">${tr("最近")}</th><th class="is-num g-dur">${tr("累计")}</th></tr></thead><tbody>
    ${apps.map((a) => `<tr><td>${esc(gmApp(a))}</td><td class="is-num">${a.n || "—"}</td><td class="is-num">${a.play ? durText(a.play) : "—"}</td><td class="is-num">${a.last ? gmDayOf(a.last).slice(5) : "—"}</td><td class="is-num">${a.total != null ? durText(a.total) : "—"}</td></tr>`).join("")}</tbody></table></div>`;
}

function gmSteamOverview(v) {
  const play = v.sessions.reduce((s, x) => s + x.dur, 0), night = v.sessions.reduce((s, x) => s + x.night_s, 0);
  const apps = gmSteamApps(v.sessions);
  const ro = `<div class="readouts gm-ro4">
      ${readout(tr("玩了"), hh(play), tr("小时"), "", false, durText(play))}
      ${readout(tr("启动"), v.sessions.length, tr("次"))}
      ${readout(tr("游戏"), apps.filter((a) => a.n).length, tr("款"))}
      ${readout(tr("深夜"), night ? hh(night) : "—", tr("小时"), "", night > 0, tr("23:00 到 5:00"))}
    </div>`;
  const tip = (d) => (d ? [tr("{n} 次", { n: d.n }), tr("玩了 {t}", { t: durText(d.play) })] : []);
  return ro + `<div class="secs-plain" style="margin-top:var(--space-6)">
    ${section(tr("每天"), gmDayChart(v.days, v.cut, { tip, label: tr("游戏") }))}
    ${v.days.some((x) => x.games.length) ? section(tr("什么时候玩"), gmTimeline(v.days, gmApp)) : ""}
    ${apps.length ? section(tr("每款游戏"), gmSteamTable(apps)) : ""}</div>`;
}

function drawGames() {
  const body = document.getElementById("gm-body"); if (!body || !GM.data) return;
  if (GM.data.game === "steam") { body.innerHTML = gmSteamOverview(gmSteamView()); return; }
  const v = gmView();
  body.innerHTML = GM.tab === "list" ? gmList(v) : GM.tab === "champs" ? gmChampsBody(v) : gmOverview(v);
  gmLoadIcons(body);
}

async function renderGames(arg) {
  const tk = RT.seq;
  if (!S.settings.games) {
    $app.innerHTML = header(tr("游戏")) + `<div class="reading">${emptyBlock("day", tr("游戏记录没开"), `<div class="empty-acts"><button class="mx-btn mx-btn--primary" id="gm-enable">${tr("去设置里打开")}</button></div>`)}</div>`;
    document.getElementById("gm-enable").onclick = () => { SS.tab = "features"; location.hash = "#/settings"; };
    return;
  }
  const on = gmOn();
  if (!on.length) {
    $app.innerHTML = header(tr("游戏")) + `<div class="reading">${emptyBlock("day", tr("没开要记的游戏"), `<div class="empty-acts"><button class="mx-btn mx-btn--primary" id="gm-enable">${tr("去设置里打开")}</button></div>`)}</div>`;
    document.getElementById("gm-enable").onclick = () => { SS.tab = "features"; location.hash = "#/settings"; };
    return;
  }
  if (arg && on.includes(arg)) GM.game = arg;
  if (GM.game && !on.includes(GM.game)) GM.game = null;
  let d = await call("games_overview", GM.game ? { game: GM.game } : {});
  // 没指定时后台挑最近玩的那款；那款关着就换成开着的第一款
  if (!on.includes(d.game)) d = await call("games_overview", { game: on[0] });
  if (stale(tk)) return;
  GM.data = d;
  GM.game = d.game;
  const steam = d.game === "steam";
  const pick = `<div class="pick"><button class="mx-btn mx-btn--secondary bill-month" id="gm-game" aria-haspopup="menu" aria-expanded="false">${esc(tr(GM_GAMES[GM.game]))}${ICON.chevD}</button>
      <div class="mx-menu" role="menu" hidden>${on.map((k) => `<button role="menuitemradio" aria-checked="${k === GM.game}" data-ggame="${k}">${esc(tr(GM_GAMES[k]))}${k === GM.game ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div>`;
  if (!(steam ? d.sessions : d.matches).length) {
    $app.innerHTML = header(tr("游戏"), pick) + `<div class="reading">${emptyBlock("day", steam ? tr("在 Windows 电脑的 Beayy 里也打开「游戏」，玩 Steam 游戏的时长会记在这里") : tr("在 Windows 电脑的 Beayy 里也打开「游戏」，打的每一局英雄联盟会记在这里"))}</div>`;
    bindPick(document.getElementById("gm-game"));
    return;
  }
  const a = d.account;
  const who = !a ? "" : steam ? a.name || "" : [`${a.name}${a.tag ? "#" + a.tag : ""}`, a.platform ? tr(GM_REGION[a.platform] || a.platform) : ""].filter(Boolean).join(" · ");
  // Steam 只有概览，不出页签
  const tabs = steam ? "" : `<div class="tabs" role="tablist">${[["overview", tr("概览")], ["list", tr("对局")], ["champs", tr("英雄")]].map(([k, v]) => `<button role="tab" aria-selected="${k === GM.tab}" class="${k === GM.tab ? "is-on" : ""}" data-gtab="${k}">${v}</button>`).join("")}</div>`;
  const seg = `<div class="mx-seg" role="tablist">${[["7", tr("近 7 天")], ["30", tr("近 30 天")], ["all", tr("全部")]].map(([k, v]) => `<button role="tab" aria-selected="${k === GM.range}" class="${k === GM.range ? "is-on" : ""}" data-grange="${k}">${v}</button>`).join("")}</div>`;
  $app.innerHTML = header(tr("游戏"), (who ? `<span class="gm-who" title="${esc(a.level ? tr("{n} 级", { n: a.level }) : "")}">${esc(who)}</span>` : "") + pick) + `<div id="games"><div class="toolbar"><div class="toolbar-left">${tabs}${tabs ? '<span class="tb-sep"></span>' : ""}${seg}</div></div><div class="reading" id="gm-body"></div></div>`;
  drawGames();
  bindPick(document.getElementById("gm-game"));
  document.getElementById("gm-game").parentElement.onclick = (e) => { const x = e.target.closest("[data-ggame]"); if (x) location.hash = `#/games/${x.dataset.ggame}`; };
  document.getElementById("games").onclick = (e) => {
    const at = (sel) => e.target.closest(sel);
    let x;
    if ((x = at("[data-gtab]"))) { GM.tab = x.dataset.gtab; return renderGames(); }
    if ((x = at("[data-grange]"))) { GM.range = x.dataset.grange; return renderGames(); }
    if ((x = at("[data-game]"))) return gmGameDialog(+x.dataset.game);
    if (at("[data-gchamp]")) { if (GM.tab !== "champs") { GM.tab = "champs"; renderGames(); } return; }
    if ((x = at("[data-gday]"))) {
      if (steam) return;
      GM.tab = "list"; const day = x.dataset.gday;
      return renderGames().then(() => { const el = document.getElementById(`gm-d-${day}`); if (el) el.scrollIntoView({ block: "start" }); });
    }
  };
}

/** 一局：开局时刻、模式、时长、阶段，两队 10 个人 */
async function gmGameDialog(id) {
  const g = await call("get_game", { id });
  if (!g) return;
  const m = g.match;
  const facts = [[tr("开局"), `${gmDay(gmDayOf(m.start))} ${hm(m.start)}`], [tr("模式"), m.queue], [tr("时长"), durText(m.dur)], [tr("胜点"), gmLp(m.rank)],
    [tr("排队"), m.phases ? durText(m.phases.queue) : ""], [tr("选人"), m.phases ? durText(m.phases.select) : ""], [tr("读条"), m.phases ? durText(m.phases.load) : ""], [tr("版本"), (m.version || "").split(".").slice(0, 2).join(".")]].filter(([, v]) => v);
  // 伤害条按这一局伤害最高的那个人算满
  const top = Math.max(1, ...g.teams.flatMap((t) => t.players.map((p) => p.dmg)));
  const team = (t) => `<div class="gm-team ${t.win ? "is-win" : "is-loss"}">
    <h3><b>${esc(`${t.mine ? tr("我方") : tr("对方")} · ${t.win ? tr("胜") : tr("负")}`)}</b><span>${esc([tr("{n} 杀", { n: t.kills }), tr("伤害 {v}", { v: gmNum(t.dmg) }), tr("金币 {v}", { v: gmNum(t.gold) })].join(" · "))}</span></h3>
    <div class="gm-pl gm-pl-head"><span></span><span>${esc(tr("玩家"))}</span><span class="is-num">K/D/A</span><span>${gmTh("swords", tr("伤害"))}</span><span class="is-num">${gmTh("coins", tr("金币"))}</span><span class="is-num">${gmTh("target", tr("补刀"))}</span><span>${esc(tr("装备"))}</span></div>
    ${t.players.map((p) => `<div class="gm-pl${p.me ? " is-me" : ""}">
      <span class="gm-pick">${gmIc("champ", p.champ_id, p.champ, "gm-ic--lg")}<span class="gm-sp">${(p.spells || []).map((sid) => gmIc("spell", sid)).join("")}</span></span>
      <span class="gm-pwho"><span class="gm-nm" title="${esc(p.champ || "")}">${gmWho(p)}</span><span class="gm-badges">${gmBadges(p.badges)}</span></span>
      <span class="gm-kda is-num"><b>${gmKda(p)}</b><small>KDA ${gmRatio(p.k, p.d, p.a)}</small></span>
      <span class="gm-dbar" title="${esc(tr("承伤 {v}", { v: gmNum(p.taken) }))}"><b>${gmNum(p.dmg)}</b><i style="width:${(p.dmg / top * 100).toFixed(1)}%"></i></span>
      <span class="is-num">${gmNum(p.gold)}</span>
      <span class="is-num">${p.cs}</span>
      <span class="gm-items">${(p.items || []).map((it, i) => gmIc("item", it, "", i === 6 ? "is-trinket" : "")).join("")}</span>
    </div>`).join("")}
    ${t.bans.length ? `<p class="gm-bans"><span>${esc(tr("禁用"))}</span>${t.bans.map((b) => gmIc("champ", b.id, b.name || `#${b.id}`, "gm-ic--sm")).join("")}</p>` : ""}</div>`;
  openDialog({ title: `${m.champ || "—"} · ${gmResult(m)}`, cancel: tr("关闭"),
    body: `<dl class="mx-facts gm-facts">${facts.map(([k, v]) => `<div><dt>${esc(k)}</dt><dd>${esc(v)}</dd></div>`).join("")}</dl>${g.teams.map(team).join("")}`,
    onOpen: (el) => { el.classList.add("gm-dlg"); gmLoadIcons(el); } });
}

// ---------- 规矩 ----------

const RS = { deleting: null };

/** 把一条规矩识别成能算的三类；识别不了返回 null（交给日总结对照）
 *  after：几点以后不用（电脑／手机／都不用）；limit：某设备每天不超过多久；app：某个 app 每天不超过多久 */
function parseRule(text) {
  const t = String(text).replace(/\s+/g, "");
  const dev = /电脑|mac/i.test(t) ? "computer" : /手机|iphone/i.test(t) ? "phone" : "any";
  let m = t.match(/(\d{1,2})(?:[:：点](\d{2})?)?(?:分)?(?:以后|之后|后)/);
  if (m && /不|别|停|关|禁/.test(t)) return { kind: "after", dev, h: +m[1] + (+m[1] < 12 && /晚上|夜里|夜间|下午|傍晚/.test(t) ? 12 : 0), m: +(m[2] || 0) };
  m = t.match(/(不超过|少于|最多|以内|不到|≤|<=?)(\d+(?:\.\d+)?)(小时|h|分钟|min|分)/i) || t.match(/(\d+(?:\.\d+)?)(小时|h|分钟|min|分)(以内|以下)/i);
  if (m) {
    const [num, unit] = /\d/.test(m[1]) ? [m[1], m[2]] : [m[2], m[3]];
    const minutes = +num * (/小时|h/i.test(unit) ? 60 : 1);
    const subject = t.split(/每天|一天|每日|不超过|少于|最多|不到/)[0].replace(/^(用|玩|刷|看)/, "");
    if (dev !== "any" && /^(用|玩|刷|看)?(电脑|手机|mac|iphone)?$/i.test(subject)) return { kind: "limit", dev, minutes };
    if (subject) return { kind: "app", app: subject.replace(/^(用|玩|刷|看)/, ""), minutes };
  }
  return null;
}

/** 用今天的实时数据对照一条规矩：{label: 对照什么, value: 今天多少, ok: 是否守住（null＝还没到点）} */
function checkRule(r, d) {
  if (!d) return null;
  const live = d.live || {}, ph = d.phone || {};
  const dayStart = toDate(d.date).getTime();
  const overlap = (a, b, from) => Math.max(0, Math.min(b, dayStart + 864e5) - Math.max(a, from)) / 1000;
  if (r.kind === "after") {
    const from = dayStart + (r.h * 60 + r.m) * 6e4;
    const when = `${pad2(r.h)}:${pad2(r.m)}`;
    const what = r.dev === "phone" ? tr("用手机") : r.dev === "computer" ? tr("用电脑") : tr("用电脑和手机");
    if (Date.now() < from) return { label: tr("还没到 {when}", { when }), value: "", ok: null };
    let sec = 0;
    if (r.dev !== "phone") for (const [a, b] of live.spans || []) sec += overlap(Date.parse(a), Date.parse(b), from);
    if (r.dev !== "computer") for (const x of ph.sessions || []) { const a = Date.parse(x.start), b = Date.parse(x.end); sec += b > a ? (overlap(a, b, from) / ((b - a) / 1000)) * x.sec : 0; }
    return { label: tr("{when} 以后{what}", { when, what }), value: tr("{n} 分钟", { n: Math.round(sec / 60) }), ok: sec < 60 };
  }
  if (r.kind === "limit") {
    const sec = r.dev === "phone" ? ph.total_sec || 0 : live.active_sec || 0;
    return { label: r.dev === "phone" ? tr("今天手机") : tr("今天电脑"), value: tr("{a} / {b} 小时", { a: hh(sec), b: +(r.minutes / 60).toFixed(1) }), ok: sec <= r.minutes * 60 };
  }
  if (r.kind === "app") {
    const key = r.app.toLowerCase();
    const hit = (x) => (x.name || "").toLowerCase().includes(key) || (x.bundle_id || "").toLowerCase().includes(key);
    let sec = 0;
    for (const x of ph.sessions || []) for (const a of x.apps) if (hit(a)) sec += a.sec;
    for (const a of live.apps || []) if (hit(a)) sec += a.sec;
    return { label: tr("今天{app}（电脑和手机）", { app: r.app }), value: tr("{a} / {b} 分钟", { a: Math.round(sec / 60), b: r.minutes }), ok: sec <= r.minutes * 60 };
  }
  return null;
}

/** 加一条或改一条规矩（小弹层） */
function ruleDialog(r, save) {
  openDialog({
    title: r ? tr("改规矩") : tr("加一条规矩"), ok: r ? tr("保存改动") : tr("加上这条"),
    body: `<div class="mx-field"><label for="rule-text">${tr("规矩")}</label><textarea class="mx-input dlg-ta" id="rule-text" placeholder="${tr("用自己的话写，比如「21:00 以后不开电脑」")}">${esc(r ? r.text : "")}</textarea>
      <p class="note">${tr("「几点以后不用电脑／手机」「电脑／手机每天不超过几小时」「某个 app 每天不超过几分钟」会当场对照，别的写法交给日总结。⌘↩ 保存。")}</p></div>`,
    submit: async (el) => {
      const text = dlgVal(el, "rule-text");
      if (!text) throw tr("先写一条规矩。");
      if (r) S.rules.find((x) => x.id === r.id).text = text;
      else S.rules.unshift({ id: "r" + Date.now().toString(36), text, enabled: true });
      await save();
    },
  });
}

/** 「方向」页：你想要什么——规矩、项目（以后加待办、每天的意图）。和今天、总结、收支这些「发生了什么」分开 */
const dirTabs = (on) => `<nav class="mx-utabs dir-tabs">${[["rules", tr("规矩")], ["projects", tr("项目")]].map(([k, v]) =>
  `<a href="#/direction${k === "rules" ? "" : "/" + k}" class="${k === on ? "is-on" : ""}"${k === on ? ' aria-current="page"' : ""}>${v}</a>`).join("")}</nav>`;

async function renderProjects() {
  const tk = RT.seq;
  PJ.list = await call("list_projects").catch(() => []);
  if (stale(tk)) return;
  $app.innerHTML = header(tr("方向")) + dirTabs("projects") + `<div class="reading">${projectsSection() || emptyInline(tr("写过几段摘要后，这里列出认出的项目"))}</div>`;
  bindProjects();
}

async function renderRules() {
  const tk = RT.seq;
  if (!S.rules) S.rules = await call("get_rules");
  const rules = S.rules;
  const today = await call("get_day", { date: localDay() }).catch(() => null);
  // 预算（「外卖每月不超过 1500」）按导入的账单算这个月花了多少（手机上没有账单）
  const budgets = rules.some((r) => /每月/.test(r.text)) && !(S.status && S.status.mobile)
    ? ((await call("money_overview", { month: localDay().slice(0, 7) }).catch(() => null)) || {}).budgets || [] : [];
  const verdict = (r) => {
    if (r.enabled === false) return `<div class="verdict muted">${tr("暂不对照")}</div>`;
    const bg = budgets.find((b) => b.text === r.text);
    if (bg) return `<div class="verdict ${bg.ok ? "ok" : "over"}"><span>${tr("这个月已花")}</span><b>${yuan(bg.spent)} / ${yuan(bg.limit)}</b>${bg.ok ? ` · ${tr("守住了")}` : ` · ${tr("没守住")}`}</div>`;
    const c = checkRule(parseRule(r.text) || {}, today);
    if (!c) return `<div class="verdict muted">${tr("这条算不出来，交给日总结对照")}</div>`;
    return `<div class="verdict ${c.ok === null ? "muted" : c.ok ? "ok" : "over"}"><span>${esc(c.label)}</span>${c.value ? `<b>${esc(c.value)}</b>` : ""}${c.ok === null ? "" : c.ok ? ` · ${tr("守住了")}` : ` · ${tr("没守住")}`}</div>`;
  };
  const row = (r, i) => `<div class="rule${r.enabled === false ? " off" : ""}" data-id="${r.id}">
      <div class="mx-row"><span class="mx-row-mark mono">${i + 1}</span>
        <div class="mx-row-main"><div class="mx-row-name">${esc(r.text)}</div>${verdict(r)}</div>
        <div class="acts"><button class="mx-btn mx-btn--text mx-btn--sm" data-act="edit">${tr("改")}</button><button class="mx-btn mx-btn--text mx-btn--sm" data-act="delete">${tr("删除")}</button></div>
        <button class="mx-switch" role="switch" aria-checked="${r.enabled !== false}" aria-label="${tr("对照这条")}" title="${r.enabled !== false ? tr("日总结对照这条") : tr("暂不对照")}" data-act="toggle"></button>
      </div>
      ${RS.deleting === r.id ? `<div class="mx-confirm"><div class="mx-confirm-text"><b>${tr("删除这条规矩？")}</b><span>${tr("删了找不回来。")}</span></div><button class="mx-btn mx-btn--secondary" data-act="keep">${tr("取消")}</button><button class="mx-btn mx-btn--danger" data-act="remove">${tr("删除这条")}</button></div>` : ""}
    </div>`;
  if (stale(tk)) return; $app.innerHTML = header(tr("方向"), `<button class="mx-btn mx-btn--primary" id="add">${tr("加一条规矩")}</button>`) + dirTabs("rules")
    + `<div class="reading" id="rules">
      ${rules.length ? rules.map(row).join("") : emptyInline(tr("用自己的话写一条规矩，比如「21:00 以后不开电脑」"))}
    </div>`;
  const save = async () => { await call("save_rules", { rules: S.rules }); renderRules(); };
  document.getElementById("add").onclick = () => { RS.deleting = null; ruleDialog(null, save); };
  const keepBtn = $app.querySelector('[data-act="keep"]');
  if (keepBtn) keepBtn.focus();
  document.getElementById("rules").onclick = async (e) => {
    const b = e.target.closest("[data-act]"); if (!b) return;
    const box = b.closest("[data-id]"), id = box.dataset.id, act = b.dataset.act;
    if (act === "edit") { RS.deleting = null; return ruleDialog(S.rules.find((r) => r.id === id), save); }
    if (act === "delete") { RS.deleting = id; return renderRules(); }
    if (act === "keep") { RS.deleting = null; return renderRules(); }
    if (act === "remove") { S.rules = S.rules.filter((r) => r.id !== id); RS.deleting = null; return save(); }
    if (act === "toggle") { const r = S.rules.find((x) => x.id === id); r.enabled = r.enabled === false; return save(); }
  };
  $app.onkeydown = (e) => { if (e.key === "Escape" && RS.deleting) { RS.deleting = null; renderRules(); } };
}

// ---------- 设备：绑定的电脑和手机，各自的记录状态 ----------

const DS = { open: null, msg: null, done: null, err: null, busy: false, account: null, removing: null };
const DEV_ICON = {
  mac: '<svg class="mx-ico lu" viewBox="0 0 24 24"><rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8M12 17v4"/></svg>',
  win: '<svg class="mx-ico lu" viewBox="0 0 24 24"><rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8M12 17v4"/></svg>',
  iphone: '<svg class="mx-ico lu" viewBox="0 0 24 24"><rect width="14" height="20" x="5" y="2" rx="2"/><path d="M12 18h.01"/></svg>',
  watch: '<svg class="mx-ico lu" viewBox="0 0 24 24"><rect width="12" height="12" x="6" y="6" rx="3"/><path d="M9 6l.6-3.5h4.8L15 6M9 18l.6 3.5h4.8L15 18"/></svg>',
};
const bytesText = (b) => (b >= 1e9 ? (b / 1e9).toFixed(1) + " GB" : b >= 1e6 ? (b / 1e6).toFixed(1) + " MB" : Math.round(b / 1e3) + " KB");
const numText = (n) => Number(n || 0).toLocaleString("en-US");

/** 设备一行下面那句状态 */
function devStatus(d, st) {
  const perm = (st && st.permissions) || {};
  if (d.this) {
    const missing = !perm.accessibility || !perm.input_monitoring;
    const [cls, text] = st.paused ? ["", st.paused_until ? tr("已暂停，{t} 自动继续", { t: hm(st.paused_until) }) : tr("已暂停")] : missing ? ["warn", tr("缺权限")] : st.capturing ? ["ok", tr("记录中")] : ["", tr("没在记录")];
    return `<span class="dot ${cls}"></span>${tr("{text} · 今天已记 {n} 条", { text, n: numText(st.events_today) })}`;
  }
  const m = String((st && st.phone_sync) || "").match(/\d{4}-\d{2}-\d{2}T[\d:.]+Z/);
  // 超过一天没有新数据：变提醒样式
  const old = (at) => !at || Date.now() - Date.parse(at) > 86400e3;
  if (d.kind === "iphone" && !d.synced) {
    const at = m ? m[0] : d.last, stale = old(at);
    return `<span class="dot ${stale ? "warn" : "ok"}"></span>${tr("同步至 {t}", { t: whenAt(at) || "—" })}${stale ? " · " + howto("screentime", tr("怎么设置")) : ""}`;
  }
  const stale = old(d.last), t = whenAt(d.last) || "—";
  return `<span class="dot ${stale ? "warn" : "ok"}"></span>${stale ? tr("停在 {t}", { t }) : tr("最近一条 {t}", { t })}`;
}

/** 每种设备记什么：一行一项 */
const DEV_RECORDS = {
  mac: [tr("切换 app 和窗口"), tr("键盘输入和发送（密码框打码）"), tr("点击和滚动"), tr("界面上的文字"), tr("在不在电脑前、锁屏、睡眠"), tr("10 分钟摘要")],
  win: [tr("切换程序和窗口"), tr("键盘输入和发送（密码框打码）"), tr("点击和滚动"), tr("界面上的文字"), tr("在不在电脑前、锁屏、睡眠"), tr("10 分钟摘要")],
  iphone: [tr("每个 app 在前台的时长"), tr("停留点：到了哪、待了多久")],
  watch: [tr("录音转成的文字，分出说话人"), tr("录音时记下的停留点")],
  android: [tr("每个 app 在前台的时长")],
};

/** 设备详情页：DetailHeader（返回、名字、状态、操作菜单）＋ 下划线页签 概况／事件 */
const DEV = { id: null, tab: "overview", events: [], more: false, type: "all", live: true, open: new Set(), timer: null, err: null };
async function renderDevice(id, tab) {
  const tk = RT.seq;
  if (DEV.id !== id) { DEV.events = []; DEV.more = false; DEV.type = "all"; DEV.open = new Set(); DEV.err = null; }
  DEV.id = id; DEV.tab = tab === "events" ? "events" : "overview";
  const [devs, st] = await Promise.all([DS.devs ? Promise.resolve(DS.devs) : call("list_devices").catch(() => []), call("get_status")]);
  S.status = st; renderSide();
  const d = devs.find((x) => x.id === id);
  if (!d) { SS.tab = "data"; location.hash = "#/settings"; return; }
  const base = `#/devices/${encodeURIComponent(id)}`;
  const head = `<div class="dev-detail"><a class="mx-back" href="#/settings" data-set-tab="data">${ICON.chevL}${tr("设备")}</a>
    <div class="mx-detail"><span class="mx-detail-mark">${DEV_ICON[d.kind] || DEV_ICON.mac}</span><div class="mx-detail-main"><h1>${esc(d.name)}</h1><p>${devStatus(d, st)}${d.this ? ` · ${tr("本机")}` : ""}</p></div>
      <div class="pick"><button class="mx-btn mx-btn--secondary" id="dev-ops" aria-haspopup="menu" aria-expanded="false">${tr("操作")}${ICON.chevD}</button>
        <div class="mx-menu" role="menu" hidden><button role="menuitem" id="dev-rename">${tr("重命名")}</button></div></div></div>
    <nav class="mx-utabs"><a href="${base}" class="${DEV.tab === "overview" ? "is-on" : ""}">${tr("概况")}</a><a href="${base}/events" class="${DEV.tab === "events" ? "is-on" : ""}">${tr("事件")}</a></nav></div>`;
  if (stale(tk)) return; $app.innerHTML = `<div class="reading">${head}<div id="dev-body">${DEV.tab === "events" ? evShell(d) : devOverview(d, st)}</div></div>`;
  bindPick(document.getElementById("dev-ops"));
  document.getElementById("dev-rename").onclick = () => renameDialog(d, () => { DS.devs = null; renderDevice(id, DEV.tab); });
  $app.querySelectorAll("[data-perm]").forEach((b) => (b.onclick = () => call("open_permission", { kind: b.dataset.perm })));
  if (DEV.tab === "events") { DEV.drawn = false; bindEvents(d); await loadEvents(d, false); startEventsTimer(d); } else stopEventsTimer();
}

function devOverview(d, st) {
  const perm = st.permissions || {};
  const permRow = (kind, name, desc) => `<div class="mx-setting"><div class="mx-setting-main"><div class="mx-setting-name">${name}</div><div class="mx-setting-desc">${desc}</div></div>
    <span class="mx-status mx-status--${perm[kind] ? "ok" : "warn"}">${perm[kind] ? tr("已授权") : tr("未授权")}</span>${perm[kind] ? "" : `<button class="mx-btn mx-btn--secondary mx-btn--sm" data-perm="${kind}">${tr("去授权")}</button>`}</div>`;
  const stats = `<div class="mx-stats dev-stats">
      <div class="mx-stat"><div class="mx-stat-label">${tr("记录从")}</div><div class="mx-stat-value mx-num">${esc(mdhm(d.first) || "—")}</div></div>
      <div class="mx-stat"><div class="mx-stat-label">${tr("最近一条")}</div><div class="mx-stat-value mx-num">${esc(ymdhm(d.last) || "—")}</div></div>
      <div class="mx-stat"><div class="mx-stat-label">${tr("共")}</div><div class="mx-stat-value mx-num">${numText(d.events)}<small>${tr("条")}</small></div></div>
      <div class="mx-stat"><div class="mx-stat-label">${tr("占用")}</div><div class="mx-stat-value mx-num">${bytesText(d.bytes).replace(/ (\S+)$/, "<small>$1</small>")}</div></div></div>`;
  const kind = d.kind === "win" ? "win" : d.kind;
  const recs = DEV_RECORDS[kind] || DEV_RECORDS.mac;
  const what = section(tr("记什么"), `<ul class="dev-recs">${recs.map((x) => `<li>${esc(x)}</li>`).join("")}</ul>`, { aside: `<a href="#/devices/${encodeURIComponent(d.id)}/events">${tr("看最近的事件")}</a>` });
  const perms = d.this && d.kind === "mac" ? section(tr("权限"), `<div class="rows">${permRow("accessibility", tr("辅助功能"), tr("读取窗口标题和屏幕上的文字"))}${permRow("input_monitoring", tr("输入监控"), tr("判断你在打字还是程序在操作"))}${permRow("full_disk_access", tr("完全磁盘访问"), `${tr("读取 iPhone 的屏幕使用时间")} · ${howto("screentime", tr("怎么设置"))}`)}</div>`) : "";
  return stats + what + perms + `<p class="note dev-id">${tr("设备编号")} <span class="mono">${esc(d.id)}</span></p>`;
}

// ---------- 设备详情 · 事件：最近的原始事件，新的在上；页面开着时每 3 秒刷新 ----------

/** 筛选：显示名 → 事件种类（以「.」结尾按前缀）；每种设备只列它会有的 */
const EV_TYPES = {
  all: [tr("全部"), null],
  input: [tr("打字和发送"), ["keyboard."]],
  mouse: [tr("点击和滚动"), ["mouse."]],
  switch: [tr("切换"), ["app.activated", "window.changed", "app.focus"]],
  ui: [tr("界面文字"), ["ui.text"]],
  presence: [tr("在不在"), ["presence", "heartbeat", "screen.", "system."]],
  speech: [tr("对话"), ["speech."]],
  place: [tr("位置"), ["place.visit"]],
  bg: [tr("后台"), ["sync.run", "summary.run", "phone.sync", "relay.sync", "app.launch", "app.quit", "permission.", "tap.started"]],
};
const EV_KINDS = { mac: ["all", "input", "mouse", "switch", "ui", "presence", "bg"], win: ["all", "input", "mouse", "switch", "ui", "presence", "bg"], iphone: ["all", "switch", "place"], watch: ["all", "speech", "place"] };

/** 一条事件的种类标签 */
function evTag(t) {
  const m = { "keyboard.text_input": tr("打字"), "keyboard.submit": tr("发送"), "keyboard.return": tr("回车"), "keyboard.shortcut": tr("快捷键"), "mouse.click": tr("点击"), "mouse.scroll": tr("滚动"),
    "app.activated": tr("切换"), "window.changed": tr("窗口"), "app.focus": tr("前台"), "ui.text": tr("界面文字"), heartbeat: tr("心跳"), presence: tr("在不在"), "speech.utterance": tr("说话"), "speech.segment": tr("录音"),
    "place.visit": tr("停留"), "sync.run": tr("同步"), "summary.run": tr("摘要"), "phone.sync": tr("读 iPhone"), "relay.sync": tr("拉录音"), "app.launch": tr("启动"), "app.quit": tr("退出") };
  return m[t] || (/^(screen|system)\./.test(t) ? tr("屏幕") : /^permission\./.test(t) ? tr("权限") : t);
}

/** 打的字：去掉方向键、退格这些控制字符，回车写成 ⏎ */
const typedText = (s) => String(s || "").replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g, "").replace(/\n/g, " ⏎ ").trim();
/** 这次打字敲了几下键（人敲的＋代理敲的） */
const keyCount = (e) => Object.values(e.keys || {}).reduce((a, b) => a + (typeof b === "number" ? b : 0), 0);
const clip = (s, n = 80) => (s.length > n ? s.slice(0, n) + "…" : s);
const durText = (sec) => (sec < 60 ? tr("{n} 秒", { n: Math.round(sec) }) : sec < 3600 ? tr("{n} 分钟", { n: Math.round(sec / 60) }) : tr("{h} 小时", { h: hh(sec) }));

/** 一条事件的一句人话（打的字、界面文字原样写出来：只在本机显示） */
function evText(e) {
  const app = e.app_name || "";
  const at = app ? tr("在 {app} ", { app }) : "";
  const el = e.element || {};
  const who = e.actor && e.actor.kind === "agent" ? tr("代理（{p}）", { p: e.actor.source_proc || tr("别的程序") }) : e.actor_kind === "agent" ? tr("代理") : "";
  const elName = el.title || el.description || ({ AXButton: tr("按钮"), AXTextField: tr("输入框"), AXTextArea: tr("输入框"), AXLink: tr("链接"), AXMenuItem: tr("菜单项"), AXCheckBox: tr("勾选框"), AXTab: tr("页签"), AXImage: tr("图片"), AXStaticText: tr("文字") }[el.role] || "");
  switch (e.event_type) {
    case "keyboard.text_input": { const t = typedText(e.text || e.typed), n = keyCount(e); return t ? tr("{who}{at}打了「{t}」", { who, at, t: clip(t) }) : tr("{who}{at}敲了{k}键盘（没读到打的内容）", { who, at, k: n ? tr(" {n} 下", { n }) : "" }); }
    case "keyboard.submit": { const t = typedText(e.text), n = keyCount(e); return tr("{who}{at}发出{t}{k}", { who, at, t: t ? tr("「{t}」", { t: clip(t) }) : "", k: n ? tr("（敲了 {n} 下）", { n }) : "" }); }
    case "keyboard.return": return tr("{who}{at}按了回车", { who, at });
    case "keyboard.shortcut": return tr("{who}{at}按了 {keys}", { who, at, keys: e.keys || "" });
    case "mouse.click": return tr("{who}{at}点了{x}{r}", { who, at, x: elName ? tr("「{t}」", { t: clip(elName, 40) }) : tr("一下"), r: e.button === "right" ? tr("（右键）") : "" });
    case "mouse.scroll": return tr("{who}{at}滚了 {n} 格", { who, at, n: e.ticks || 1 });
    case "app.activated": return tr("切到 {app}{w}", { app, w: e.window_title ? ` · ${clip(e.window_title, 50)}` : "" });
    case "window.changed": return tr("{app} 换了窗口{w}", { app, w: e.window_title ? tr("：{t}", { t: clip(e.window_title, 50) }) : "" });
    case "ui.text": return tr("读到 {app} 的界面文字 {n} 行{s}", { app, n: e.lines || 0, s: e.has_secure_field ? tr("（有密码框，已打码）") : "" });
    case "heartbeat": return e.state === "idle" ? tr("没在操作，空闲 {d}", { d: durText(e.idle_seconds || 0) }) : tr("在用 {app}", { app });
    case "presence": return e.state === "active" ? tr("回到电脑前") : tr("离开电脑（空闲 {d}）", { d: durText(e.idle_seconds || 0) });
    case "screen.sleep": return tr("屏幕关了");
    case "screen.wake": return tr("屏幕亮了");
    case "screen.locked": return tr("锁屏");
    case "screen.unlocked": return tr("解锁");
    case "system.sleep": return tr("电脑睡眠");
    case "system.wake": return tr("电脑唤醒");
    case "app.focus": { const n = (ICONS[1][e.bundle_id] || {}).name || String(e.bundle_id || "").split(".").pop(); return e.state === "start" ? tr("打开 {app}", { app: n }) : tr("离开 {app}", { app: n }); }
    case "speech.utterance": return tr("{who}：{t}", { who: e.speaker || tr("未辨"), t: clip(String(e.text || ""), 120) });
    case "speech.segment": return tr("录了一段 {a}，有人说话 {b}{c}", { a: durText(e.audio_sec || 0), b: durText(e.speech_sec || 0), c: e.battery != null ? tr("，电量 {p}%", { p: Math.round(e.battery * 100) }) : "" });
    case "place.visit": { const p = e.label || e.name || tr("没有地名的地方"); return e.depart ? tr("离开 {place}", { place: p }) : tr("到了 {place}", { place: p }); }
    case "sync.run": return tr("同步一轮：传 {up} 段、收 {down} 段{err}", { up: e.uploaded || 0, down: e.downloaded || 0, err: e.errors ? tr("，{n} 个错", { n: e.errors }) : "" });
    case "summary.run": return tr("写了 {n} 段摘要（{s} 秒）", { n: e.written || 0, s: e.seconds || 0 });
    case "phone.sync": return tr("从 iCloud 读到 iPhone 新记录 {n} 条", { n: numText(e.added) });
    case "relay.sync": return tr("同步来 {n} 条", { n: numText(e.added) });
    case "app.launch": return tr("Beayy {v} 启动{s}", { v: e.version || "", s: e.shadow ? tr("（测试版）") : "" });
    case "app.quit": return tr("Beayy 退出");
    case "tap.started": return tr("开始监听键盘和鼠标");
    case "permission.requested": return tr("申请{p}权限", { p: { accessibility: tr("辅助功能"), input_monitoring: tr("输入监控"), full_disk_access: tr("完全磁盘访问") }[e.service] || e.service || "" });
    case "permission.changed": return tr("权限变了");
    default: return e.event_type || tr("（没有种类）");
  }
}

const hms = (iso) => { const t = new Date(iso); return `${pad2(t.getHours())}:${pad2(t.getMinutes())}:${pad2(t.getSeconds())}`; };
/** 一条事件的键：时刻＋种类＋第几条（同一秒可能有好几条） */
const evKey = (e, i) => `${e.timestamp}|${e.event_type}|${i}`;

function evShell(d) {
  const kinds = EV_KINDS[d.kind] || EV_KINDS.mac;
  if (!kinds.includes(DEV.type)) DEV.type = "all";
  return `<div class="toolbar ev-bar"><div class="toolbar-left"><div class="pick"><button class="mx-btn mx-btn--secondary" id="ev-type" aria-haspopup="menu" aria-expanded="false">${EV_TYPES[DEV.type][0]}${ICON.chevD}</button>
      <div class="mx-menu" role="menu" hidden>${kinds.map((k) => `<button role="menuitemradio" aria-checked="${k === DEV.type}" data-ev-type="${k}">${EV_TYPES[k][0]}${k === DEV.type ? `<span class="mx-check">${ICON.check}</span>` : ""}</button>`).join("")}</div></div></div>
    <div class="toolbar-right"><span class="mx-status ${DEV.live ? "mx-status--ok" : ""}" id="ev-live-state">${DEV.live ? tr("实时 · 每 3 秒刷新") : tr("已暂停刷新")}</span><button class="mx-btn mx-btn--text mx-btn--sm" id="ev-live">${DEV.live ? tr("暂停") : tr("继续")}</button></div></div>
    <div id="ev-list"><p class="note">${tr("读取中…")}</p></div>
    <p class="note">${tr("打的字和界面文字只在这台电脑上显示，不进网站；密码框在记录时就打了码。")}</p>`;
}

function bindEvents(d) {
  bindPick(document.getElementById("ev-type"));
  $app.querySelectorAll("[data-ev-type]").forEach((b) => (b.onclick = () => { DEV.type = b.dataset.evType; DEV.events = []; DEV.open = new Set(); renderDevice(d.id, "events"); }));
  document.getElementById("ev-live").onclick = () => { DEV.live = !DEV.live; renderDevice(d.id, "events"); };
}

/** 读事件：older＝接着往前读一页；不然读最新一页，和已经读到的更早的那些接上 */
async function loadEvents(d, older) {
  const types = EV_TYPES[DEV.type][1];
  const before = older && DEV.events.length ? DEV.events[DEV.events.length - 1].timestamp : null;
  let r;
  try { r = await call("recent_events", { device: d.id, before, limit: 200, types }); DEV.err = null; }
  catch (x) { DEV.err = String(x && x.message ? x.message : x); return drawEvents(d); }
  if (older) { DEV.events = DEV.events.concat(r.events); DEV.more = r.more; }
  else {
    const oldest = r.events.length ? r.events[r.events.length - 1].timestamp : null;
    const keep = oldest ? DEV.events.filter((e) => e.timestamp < oldest) : [];
    const same = DEV.events.length && r.events.length && DEV.events[0].timestamp === r.events[0].timestamp && DEV.events.length >= r.events.length;
    if (same && DEV.drawn) return;
    DEV.events = r.events.concat(keep);
    if (!keep.length) DEV.more = r.more;
  }
  drawEvents(d);
}

function drawEvents(d) {
  const box = document.getElementById("ev-list"); if (!box) return;
  DEV.drawn = true;
  if (DEV.err) { box.innerHTML = `<p class="mx-error">${tr("读不到事件：{e}", { e: esc(DEV.err) })}</p>`; return; }
  if (!DEV.events.length) { box.innerHTML = emptyInline(tr("最近 8 小时没有这类事件")); return; }
  const phone = d.kind === "iphone";
  box.innerHTML = `<div class="ev-list">${DEV.events.map((e, i) => {
    const k = evKey(e, i), app = e.bundle_id && (e.app_name || phone) ? `<span class="ico" data-b="${esc(e.bundle_id)}" data-p="${phone ? 1 : 0}" title="${esc(e.app_name || e.bundle_id)}">${esc(((e.app_name || e.bundle_id.split(".").pop())[0] || "?").toUpperCase())}</span>` : `<span class="ico ico-none"></span>`;
    const raw = JSON.stringify(e, null, 2);
    const extra = e.event_type === "ui.text" && e.text ? `<pre class="ev-text">${esc(clip(String(e.text), 2000))}</pre>` : "";
    return `<details class="ev-row" data-k="${esc(k)}"${DEV.open.has(k) ? " open" : ""}><summary><span class="t mono">${hms(e.timestamp)}</span><span class="mx-tag">${esc(evTag(e.event_type))}</span>${app}<span class="ev-say">${esc(evText(e))}</span></summary>
      <div class="ev-more">${extra}<pre class="ev-raw mono">${esc(raw)}</pre></div></details>`;
  }).join("")}</div>${DEV.more ? `<div class="btns ev-older"><button class="mx-btn mx-btn--secondary mx-btn--sm" id="ev-older">${tr("加载更早")}</button></div>` : ""}`;
  box.querySelectorAll(".ev-row").forEach((x) => x.addEventListener("toggle", () => { x.open ? DEV.open.add(x.dataset.k) : DEV.open.delete(x.dataset.k); }));
  const older = document.getElementById("ev-older"); if (older) older.onclick = () => loadEvents(d, true);
  loadIcons().then(() => { if (phone) box.querySelectorAll(".ev-row").forEach((x, i) => { const e = DEV.events[i]; if (e && e.event_type === "app.focus") x.querySelector(".ev-say").textContent = evText(e); }); });
}

function startEventsTimer(d) {
  stopEventsTimer();
  if (!DEV.live) return;
  DEV.timer = setInterval(() => {
    if (S.page !== "devices" || DEV.id !== d.id || DEV.tab !== "events" || !document.getElementById("ev-list")) return stopEventsTimer();
    loadEvents(d, false);
  }, 3000);
}
function stopEventsTimer() { if (DEV.timer) { clearInterval(DEV.timer); DEV.timer = null; } }

/** 下拉菜单（.pick 里的按钮＋.mx-menu）：点按钮开关，点外面或 Esc 关（见文末） */
function bindPick(btn) {
  if (!btn) return;
  const menu = btn.parentElement.querySelector(".mx-menu");
  btn.onclick = (e) => { e.stopPropagation(); menu.hidden = !menu.hidden; btn.setAttribute("aria-expanded", String(!menu.hidden)); };
}

/** 给设备改名（小弹层）。改的名字会同步到别的设备，时间线上的标签也用它；清空＝改回默认名 */
function renameDialog(d, done) {
  if (!d) return;
  openDialog({
    title: tr("重命名设备"), ok: tr("保存"),
    body: dlgField("dev-name", tr("名称"), { value: d.custom ? d.name : "", ph: d.default_name, help: tr("清空就改回默认名字") }),
    onOpen: (el) => { el.querySelector("#dev-name").maxLength = 40; },
    submit: async (el) => { await call("rename_device", { id: d.id, name: dlgVal(el, "dev-name") }); DS.devs = null; done ? done() : renderSettings(); },
  });
}

/** 同步进行中：每 2 秒只刷新同步卡片（进度条），同步完了整页重画一次（设备列表、最近一条会变） */
let SYNC_POLL = null;
/** 同步进行中：设置「数据」页开着就每 2 秒重画一次（进度条、上次时间） */
function pollSync() {
  if (SYNC_POLL) return;
  SYNC_POLL = setTimeout(() => {
    SYNC_POLL = null;
    if (S.page === "settings" && SS.tab === "data" && !document.querySelector(".dlg-root")) renderSettings();
  }, 2000);
}

/** 同步方式：一种一张卡片，选中的下面只放这种方式自己的东西。不同步＝没开 */
const SYNC_WAYS = [
  ["none", tr("不同步"), tr("只用这一台")],
  ["icloud", "iCloud", tr("设备都是苹果的，不用注册")],
  ["webdav", tr("网盘（WebDAV）"), tr("坚果云、NAS，安卓和 Windows 也能用")],
  ["server", tr("Beayy 服务"), tr("邮箱登录，官方或自己部署的")],
];
const syncWay = (sy) => (!sy || !sy.configured ? "none" : sy.mode || (sy.folder ? "folder" : "server"));
const wayName = (w) => (w === "folder" ? tr("同步文件夹") : (SYNC_WAYS.find(([k]) => k === w) || [])[1] || w);
const IS_ANDROID = /Android/i.test(navigator.userAgent);
/** 中文里夹着 iCloud、WebDAV 这类词：前后补一个空格 */
const gap = (t) => (LANG === "zh" ? String(t).replace(/([\u4e00-\u9fff])([A-Za-z])/g, "$1 $2").replace(/([A-Za-z])([\u4e00-\u9fff])/g, "$1 $2") : t);

/** 设置「数据」页的同步一组：方式卡片＋这种方式的状态、账号或主密码、恢复码 */
function syncPanel(sy) {
  if (sy.shadow) return section(tr("同步"), emptyInline(tr("测试版不同步")));
  const mobile = !!(S.status && S.status.mobile), cur = syncWay(sy);
  const ways = SYNC_WAYS.filter(([k]) => !(IS_ANDROID && k === "icloud"));
  const cards = `<div class="sync-ways" role="radiogroup" aria-label="${tr("同步方式")}">${ways.map(([k, name, desc]) =>
    `<button class="sync-way${k === cur ? " is-on" : ""}" role="radio" aria-checked="${k === cur}" data-way="${k}"><b>${name}</b><span>${desc}</span></button>`).join("")}</div>`
    + (mobile ? "" : `<button class="mx-btn mx-btn--text mx-btn--sm sync-adv" data-way="folder">${cur === "folder" ? tr("在用：同步文件夹") : tr("高级：同步文件夹（OneDrive、Syncthing）")}</button>`);
  const msg = (DS.err ? `<div class="mx-field-error">${esc(DS.err)}</div>` : "") + (DS.msg || AC.msg ? `<p class="note">${esc(DS.msg || AC.msg)}</p>` : "");
  const btn = (id, text) => smallBtn(id, text);
  const confirm = (go) => `<div class="btns"><button class="mx-btn mx-btn--text mx-btn--sm" id="logout-keep">${tr("取消")}</button><button class="mx-btn mx-btn--danger mx-btn--sm" id="logout-go">${go}</button></div>`;
  // 同步不走 Beayy 服务时，单为官方服务（模型、转写、地图）登的账号
  const svc = sy.service_email ? setRow(tr("Beayy 账号"), `${esc(sy.service_email)} · ${tr("官方模型、转写、地图用")}`, btn("svc-logout", tr("退出登录"))) : "";
  if (cur === "none") return section(tr("同步"), cards + (svc ? `<div class="rows">${svc}</div>` : "") + msg);
  // 旧版配对码账号、登录失效：先把这件事办了
  if (cur === "server" && !sy.email) return section(tr("同步"), cards + `<div class="rows">${setRow(tr("邮箱和主密码"), tr("旧版配对码账号，设好邮箱和主密码就能在别的设备登录"), btn("ac-claim", tr("设好账号")))}</div>` + msg);
  if (sy.need_login) return section(tr("同步"), cards + `<div class="rows">${setRow(tr("登录已失效"), tr("重新登录后接着同步"), btn("ac-login", tr("重新登录")))}</div>` + msg);
  const pg = sy.progress || {}, busy = sy.running || DS.busy, going = busy && pg.phase && pg.total > 0;
  const bar = going ? `<div class="sync-bar-row"><div class="sync-prog"><div class="sync-prog-text"><span>${pg.phase === "upload" ? tr("上传") : tr("下载")}</span><span class="mono">${tr("{a} / {b} 段", { a: numText(pg.done), b: numText(pg.total) })}</span></div><div class="bar"><i style="width:${Math.min(100, Math.round((pg.done / pg.total) * 100))}%"></i></div></div></div>` : "";
  const state = busy ? `<span class="mx-status">${tr("同步中…")}</span>` : sy.last_error ? `<span class="mx-status mx-status--warn">${tr("上次有错")}</span>` : `<span class="mx-status mx-status--ok">${tr("已开{last}", { last: sy.last ? tr(" · 上次 {t}", { t: whenAt(sy.last) }) : "" })}</span>`;
  const target = { server: hostOf(sy.server || ""), folder: tildePath(sy.folder), webdav: hostOf(sy.webdav || "") }[cur] || "";
  const sessions = (DS.account && DS.account.sessions) || [];
  const leave = cur === "server"
    ? setRow(tr("账号"), `${esc(sy.email)} · ${tr("别的设备用这个邮箱登录")}`, AC.confirmLogout ? confirm(tr("退出登录")) : btn("ac-logout", tr("退出登录")))
      + (sessions.length ? setRow(tr("已登录设备"), "", `<button class="mx-btn mx-btn--text mx-btn--sm" id="sess-open">${tr("{n} 台", { n: sessions.length })}${ICON.chevR}</button>`) : "")
    : "";
  return section(tr("同步"), cards + `<div class="rows">`
    + setRow(wayName(cur), `${target ? `<span class="mono">${esc(target)}</span>` : ""}${target && DS.done ? "<br>" : ""}${DS.done ? esc(DS.done) : ""}`, `<span class="btns">${state}<button class="mx-btn mx-btn--secondary mx-btn--sm" id="sync-now" ${busy ? "disabled" : ""}>${tr("立即同步")}</button></span>`)
    + bar + (sy.last_error ? `<div class="mx-error">${esc(sy.last_error)}</div>` : "")
    + leave + svc
    + setRow(tr("主密码"), cur === "server" ? tr("只在设备上用，不会发给服务器") : gap(tr("加入同步时要输，{where}读不到内容", { where: wayName(cur) })), btn("ac-password", tr("更改")))
    + setRow(tr("恢复码"), tr("忘了主密码、设备全丢了，凭它解开记录"), btn("ac-show-code", tr("查看")))
    + `</div>` + msg);
}

/** 设置「数据」页的设备一组：一台一行，点进去是设备详情页；本机缺权限时顶上一条提示 */
function devicesPanel(devs, st) {
  const perm = st.permissions || {};
  const me = devs.find((d) => d.this);
  const missing = [["accessibility", tr("辅助功能")], ["input_monitoring", tr("输入监控")], ["full_disk_access", tr("完全磁盘访问")]].filter(([k]) => perm[k] === false).map(([, n]) => n);
  const notice = me && missing.length ? `<div class="mx-notice"><span>${tr("这台电脑缺{perms}权限，记不全。", { perms: missing.join(tr("、")), n: missing.length })}</span><a class="mx-btn mx-btn--secondary mx-btn--sm" href="#/devices/${encodeURIComponent(me.id)}">${tr("去处理")}</a></div>` : "";
  const rows = devs.map((d) => `<a class="mx-row" href="#/devices/${encodeURIComponent(d.id)}"><span class="mx-row-mark">${DEV_ICON[d.kind] || DEV_ICON.mac}</span>
      <span class="mx-row-main"><div class="mx-row-name">${esc(d.name)}${d.this ? `<span class="mx-tag">${tr("本机")}</span>` : ""}${d.synced ? `<span class="mx-tag">${tr("经同步")}</span>` : ""}</div><div class="mx-row-desc">${devStatus(d, st)}</div></span>
      <span class="mx-row-side">${ICON.chevR}</span></a>`).join("");
  return section(tr("设备"), notice + (rows ? `<div class="mx-list">${rows}</div>` : emptyInline(tr("还没有设备"))),
    { aside: `<button class="mx-btn mx-btn--text mx-btn--sm" id="add-dev">${tr("添加设备")}</button>` });
}

/** 添加设备：怎么加入跟着现在的同步方式说 */
function addDeviceDialog() {
  const way = syncWay(S.sync);
  const join = { none: tr("先在这里选一种同步方式，那台设备上选同一种"), icloud: tr("登同一个 Apple ID，在设置 → 数据里选 iCloud，输主密码"),
    webdav: tr("在设置 → 数据里选网盘，填同一个地址和主密码"), server: tr("用同一个账号登录"), folder: tr("选同一个同步文件夹，输主密码") }[way];
  const phoneOk = way !== "folder";
  const opt = (icon, name, text) => `<div class="add-opt">${icon}<div><b>${name}</b><span>${text}</span></div></div>`;
  openDialog({
    title: tr("添加设备"), cancel: tr("关闭"), ok: null,
    body: `<div class="add-dev">`
      + opt(DEV_ICON.iphone, tr("iPhone 和手表"), phoneOk ? tr("从 TestFlight 装 Beayy，{join}。停留点、苹果健康、手表录音从这里来", { join }) : tr("同步文件夹到不了手机，要换成 iCloud、网盘或 Beayy 服务"))
      + (IS_WIN ? "" : opt(DEV_ICON.iphone, tr("只看 iPhone 用了哪些 app"), tr("不用装 app：iPhone 和这台 Mac 都打开 设置 → 屏幕使用时间 →「跨设备共享」，用同一个 Apple 账号")))
      + opt(DEV_ICON.mac, tr("另一台电脑"), tr("装 Mac 版或 Windows 版，{join}", { join }))
      + opt(DEV_ICON.iphone, tr("安卓手机"), way === "icloud" || way === "folder" ? gap(tr("安卓用不了{way}，要换成网盘或 Beayy 服务", { way: wayName(way) })) : tr("装安卓版（预览），{join}", { join }))
      + `</div>`,
  });
}

/** 点一张同步方式卡片：开着别的先确认停掉（不同步＝停掉），再接着设新的 */
function pickWay(way) {
  const cur = syncWay(S.sync);
  if (way === cur) return;
  if (cur === "none") return setupWay(way);
  openDialog({
    title: way === "none" ? tr("停止同步？") : gap(tr("换成{way}？", { way: wayName(way) })),
    body: `<p class="note">${gap(tr("先停掉现在的{cur}。这台的记录不受影响，{cur}里已有的也还在。", { cur: wayName(cur) }))}</p>`,
    ok: way === "none" ? tr("停止同步") : tr("停掉并换"), okClass: way === "none" ? "danger" : "primary", busy: tr("处理中…"),
    submit: async () => {
      await call(cur === "server" ? "account_stop_sync" : "account_logout");
      DS.account = null; SS.llm = null; DS.done = DS.err = DS.msg = AC.msg = null; AC.confirmLogout = false;
      await renderSettings();
      if (way !== "none") setTimeout(() => setupWay(way));
    },
  });
}

async function setupWay(way) {
  if (way === "server") {
    if (S.settings.sync_mode === "folder") await saveSome({ sync_mode: "server" });
    return loginDialog("login");
  }
  if (way === "folder") return folderPickDialog();
  if (way === "webdav") return webdavDialog();
  let s;
  try { s = await call("icloud_state"); } catch (x) { s = { available: false, error: String(x) }; }
  if (!s.available) return openDialog({ title: "iCloud", cancel: tr("关闭"), ok: null, body: `<p class="note">${esc(s.error || tr("这台设备没登录 iCloud，或者没打开 iCloud 云盘。打开后再选一次。"))}</p>` });
  vaultDialog(s.vault ? "join" : "create", "icloud");
}

/** 网盘（WebDAV）：地址、账号、密码，连得上再新开或加入 */
function webdavDialog() {
  openDialog({
    title: tr("网盘（WebDAV）"), ok: tr("下一步"), busy: tr("连接中…"),
    body: dlgField("dv-url", tr("地址"), { mono: true, ph: "https://dav.jianguoyun.com/dav/Beayy", help: tr("每台设备填同一个地址，最好是单独的文件夹") })
      + dlgField("dv-user", tr("账号"), { ph: tr("网盘的登录邮箱或用户名") })
      + dlgField("dv-pass", tr("密码"), { type: "password", help: tr("坚果云要用「第三方应用管理」里生成的应用密码") }),
    submit: async (el) => {
      const cred = { url: dlgVal(el, "dv-url").replace(/\/+$/, ""), user: dlgVal(el, "dv-user"), pass: el.querySelector("#dv-pass").value };
      if (!isUrl(cred.url)) throw tr("地址要以 https:// 开头。");
      if (!cred.user || !cred.pass) throw tr("账号和密码都要填。");
      const s = await call("webdav_state", cred);
      if (!s.ok) throw s.error || tr("连不上这个地址");
      setTimeout(() => vaultDialog(s.vault ? "join" : "create", "webdav", cred));
    },
  });
}

/** 同步文件夹（高级，只有电脑）：选一个网盘会同步的文件夹，每台电脑选同一个 */
function folderPickDialog() {
  const cf = S.settings;
  openDialog({
    title: tr("同步文件夹"), ok: tr("下一步"),
    body: `<div class="mx-field"><label for="sf">${tr("文件夹")}</label><div class="inline"><input class="mx-input mono" id="sf" value="${esc(cf.sync_folder || "")}" placeholder="${esc(cf.icloud_folder || "")}"><button class="mx-btn mx-btn--secondary" id="sf-pick" type="button">${tr("选择…")}</button></div></div>
      <p class="note">${tr("选 OneDrive、Dropbox、Syncthing 或 NAS 会同步的文件夹，每台电脑选同一个。手机用不了")}</p>`,
    onOpen: (el, d) => { el.querySelector("#sf-pick").onclick = async () => { try { const p = await call("pick_folder"); if (p) el.querySelector("#sf").value = p; } catch (x) { d.setErr(String(x)); } }; },
    submit: async (el) => {
      const f = dlgVal(el, "sf") || cf.icloud_folder;
      if (!f) throw tr("选一个文件夹。");
      await saveSome({ sync_mode: "folder", sync_folder: f });
      const s = await call("folder_state").catch(() => ({}));
      setTimeout(() => vaultDialog(s.vault ? "join" : "create", "folder"));
    },
  });
}

/** 设置「数据」页的交互：方式卡片、立即同步、账号和主密码、设备、存储 */
function bindData(sy) {
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  document.querySelectorAll("[data-way]").forEach((b) => (b.onclick = () => pickWay(b.dataset.way)));
  on("ac-login", () => loginDialog("login"));
  on("ac-claim", claimDialog);
  on("sess-open", sessionsDialog);
  on("ac-password", passwordDialog);
  on("ac-show-code", async () => { try { codeDialog(await call("account_recovery_code"), false); } catch (x) { AC.msg = tr("取不到恢复码：{e}", { e: x }); renderSettings(); } });
  on("ac-logout", () => { AC.confirmLogout = true; renderSettings(); });
  on("logout-keep", () => { AC.confirmLogout = false; renderSettings(); });
  on("svc-logout", async () => { await call("account_service_logout"); SS.llm = null; renderSettings(); });
  on("logout-go", async () => { await call("account_logout"); AC.confirmLogout = false; AC.msg = null; DS.account = null; DS.done = null; renderSettings(); });
  on("sync-now", async () => {
    DS.busy = true; DS.err = DS.msg = DS.done = null; renderSettings();
    try { const r = await call("sync_now"); DS.done = tr("{t} 上传 {up} 段，下载 {down} 段{err}", { t: hm(new Date().toISOString()), up: r.uploaded, down: r.downloaded, err: r.errors.length ? tr("，有错：{e}", { e: r.errors.join(tr("；")) }) : "" }); } catch (x) { DS.err = String(x); }
    DS.busy = false; renderSettings();
  });
  on("add-dev", addDeviceDialog);
  // 登录着的设备：进这一页取一次，失败（没网、令牌失效）就不显示
  if (syncWay(sy) === "server" && sy.email && !sy.need_login && !DS.account) { DS.account = {}; call("account_info").then((a) => { DS.account = a; if (S.page === "settings" && SS.tab === "data") renderSettings(); }).catch(() => {}); }
  if (sy.running || DS.busy) pollSync();
}

/** 登录着这个账号的设备（小弹层）：这台标出来，别的可以移除（先点「移除」，再点一次确认） */
function sessionsDialog() {
  let removing = null;
  const body = () => {
    const sessions = (DS.account && DS.account.sessions) || [], me = DS.account && DS.account.session;
    return `<div class="sess">${sessions.map((x) => `<div class="sess-row"><span>${esc((DS.names || {})[x.device] || x.name || x.device)}${x.id === me ? `<span class="mx-tag">${tr("这台")}</span>` : ""}</span><span class="note">${tr("最近 {t}", { t: mdhm(new Date(x.last_seen).toISOString()) })}</span>${x.id === me ? "<span></span>" : removing === x.id
      ? `<span class="btns"><button class="mx-btn mx-btn--text mx-btn--sm" data-sess-keep>${tr("取消")}</button><button class="mx-btn mx-btn--danger mx-btn--sm" data-sess-go="${esc(x.id)}">${tr("移除")}</button></span>`
      : `<button class="mx-btn mx-btn--text mx-btn--sm" data-sess="${esc(x.id)}">${tr("移除")}</button>`}</div>`).join("")}</div>
      <p class="note">${tr("移除后，那台设备要重新登录才能再同步，它已经传上来的记录还在。")}</p>`;
  };
  const d = openDialog({ title: tr("登录着这个账号的设备"), body: `<div id="sess-body">${body()}</div>`, cancel: tr("关闭"), ok: null,
    onOpen: (el) => {
      const draw = () => { el.querySelector("#sess-body").innerHTML = body(); bind(); };
      const bind = () => {
        el.querySelectorAll("[data-sess]").forEach((b) => (b.onclick = () => { removing = b.dataset.sess; draw(); }));
        el.querySelectorAll("[data-sess-keep]").forEach((b) => (b.onclick = () => { removing = null; draw(); }));
        el.querySelectorAll("[data-sess-go]").forEach((b) => (b.onclick = async () => {
          try { await call("account_remove_device", { id: b.dataset.sessGo }); DS.account = await call("account_info").catch(() => DS.account); DS.msg = tr("已移除，那台设备要重新登录"); }
          catch (x) { DS.err = String(x); }
          removing = null; draw(); if (S.page === "settings") renderSettings();
        }));
      };
      bind();
    } });
  return d;
}

// ---------- 设置 ----------

const THEMES = [["", tr("跟随系统")], ["light", tr("浅色")], ["dark", tr("深色")]];
/** 界面语言（i18n.js 读 localStorage "lang"）：语言名照各自的写法，不翻 */
const LANGS = [["", tr("跟随系统")], ["zh", "简体中文"], ["en", "English"]];
/** AI 总结的语言（settings.json 的 summary_language）：空＝跟随界面语言（界面语言只在本机 localStorage，启动时报给后台存成 ui_lang） */
const SUM_LANGS = [["", tr("跟随界面")], ["zh", "简体中文"], ["en", "English"]];
const ROASTS = ["off", "low", "medium", "high", "max", "ultra"].map((k) => [k, k]);

// ---------- 字体（设置 → 通用）：默认用系统的，也可以换成这台电脑上装的任意字体；只存本机（index.html 的 FONTS） ----------

const IS_APPLE = /Mac|iPhone|iPad/.test(navigator.userAgent);
/** 两处字体：界面（几乎所有字）、数字和钟点（等宽）。默认是什么、样例写什么 */
const FONT_KINDS = {
  sans: { label: tr("界面"), def: IS_APPLE ? tr("苹方") : /Windows/.test(navigator.userAgent) ? tr("微软雅黑") : tr("系统字体"),
    how: IS_APPLE ? tr("默认中文用苹方，英文和数字用系统字体 SF Pro。") : tr("默认中文用微软雅黑，英文和数字用 Segoe UI。"),
    sample: tr("今天在电脑前 6.2 小时，切换 372 次。Be Authentic.") },
  mono: { label: tr("数字和钟点"), def: IS_APPLE ? "SF Mono" : /Windows/.test(navigator.userAgent) ? "Consolas" : tr("等宽字体"),
    how: IS_APPLE ? tr("默认用 SF Mono，夹在里面的中文用苹方。") : tr("默认用 Consolas，夹在里面的中文用微软雅黑。"),
    sample: tr("09:04–18:00 · 4.3h · 62% · 昨天 22:10") },
};
const fontName = (k) => { const f = FONTS.get(k); return f ? f.name : FONT_KINDS[k].def; };
const fontDesc = () => tr("界面 {a} · 数字和钟点 {b}", { a: esc(fontName("sans")), b: esc(fontName("mono")) });
/** 一个字体在 CSS 里的写法（自己的名字＋默认的退路） */
const fontCss = (k, f) => esc(FONTS.stack(k, f));

/** 换字体：上面切「界面／数字和钟点」，预览用选中的字体，下面搜、选；保存才生效 */
function fontDialog(done) {
  const pick = { sans: FONTS.get("sans"), mono: FONTS.get("mono") };
  let kind = "sans", all = null, q = "";
  const body = `<div class="mx-seg fp-kind" role="tablist">${Object.entries(FONT_KINDS).map(([k, v]) => `<button role="tab" data-k="${k}">${v.label}</button>`).join("")}</div>
    <div class="fp-preview" id="fp-preview"></div>
    <p class="note fp-how" id="fp-how"></p>
    <input class="mx-input" id="fp-q" type="search" placeholder="${tr("搜索这台电脑上的字体")}" autocomplete="off" aria-label="${tr("搜索字体")}">
    <div class="fp-list" id="fp-list" role="radiogroup" aria-label="${tr("字体")}"><p class="note fp-wait">${tr("在读这台电脑上的字体…")}</p></div>`;
  openDialog({
    title: tr("字体"), body, ok: tr("保存"),
    submit: () => { FONTS.set("sans", pick.sans); FONTS.set("mono", pick.mono); done(); },
    onOpen: (el) => {
      const $ = (id) => el.querySelector("#" + id), list = $("fp-list");
      const draw = () => {
        el.querySelectorAll(".fp-kind button").forEach((b) => { const on = b.dataset.k === kind; b.classList.toggle("is-on", on); b.setAttribute("aria-selected", on); });
        const k = FONT_KINDS[kind], cur = pick[kind];
        $("fp-preview").style.fontFamily = FONTS.stack(kind, cur);
        $("fp-preview").textContent = k.sample;
        $("fp-how").textContent = k.how + tr(" 换成别的字体后，缺的字照样用默认的。");
        if (!all) return;
        const hit = all.filter((f) => !q || (f.name + " " + f.family).toLowerCase().includes(q));
        const rows = [{ family: "", name: tr("跟随系统（{f}）", { f: k.def }) }, ...hit];
        list.innerHTML = rows.map((f, i) => {
          const on = cur ? cur.family === f.family : !f.family;
          return `<button class="fp-item" role="radio" aria-checked="${on}" tabindex="-1" data-i="${i}"><span class="fp-n" style="font-family:${fontCss(kind, f.family ? f : null)}">${esc(f.name)}</span>${f.family && f.family !== f.name ? `<span class="fp-f">${esc(f.family)}</span>` : ""}${on ? ICON.check : ""}</button>`;
        }).join("") + (hit.length ? "" : `<p class="note fp-wait">${tr("这台电脑上没有叫「{q}」的字体", { q: esc(q) })}</p>`);
        const items = [...list.querySelectorAll(".fp-item")];
        const sel = items.find((x) => x.getAttribute("aria-checked") === "true") || items[0];
        sel.tabIndex = 0; sel.scrollIntoView({ block: "nearest" });
        items.forEach((b) => (b.onclick = () => { const f = rows[+b.dataset.i]; pick[kind] = f.family ? f : null; draw(); list.querySelector(`[data-i="${b.dataset.i}"]`).focus(); }));
        list.onkeydown = (e) => {
          const i = items.indexOf(document.activeElement); if (i < 0) return;
          const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: items.length - 1 }[e.key];
          if (to == null || !items[to]) return;
          e.preventDefault(); items[to].click();
        };
      };
      el.querySelector(".fp-kind").onclick = (e) => { const b = e.target.closest("[data-k]"); if (b) { kind = b.dataset.k; draw(); } };
      $("fp-q").oninput = (e) => { q = e.target.value.trim().toLowerCase(); draw(); };
      // 搜索框里回车＝选第一个搜到的，不是保存
      $("fp-q").onkeydown = (e) => { if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); e.stopPropagation(); const b = list.querySelector('.fp-item[data-i="1"]'); if (b) b.click(); } };
      draw();
      call("list_fonts").then((fs) => { all = fs || []; draw(); })
        .catch(() => { all = []; draw(); list.insertAdjacentHTML("beforeend", `<p class="note fp-wait">${tr("读不到这台电脑上的字体，只能跟随系统")}</p>`); });
    },
  });
}
const SETTING_KEYS = ["gateway", "model_segment", "repo_path", "blocklist", "relay_server"];
const same = (a, b) => JSON.stringify(SETTING_KEYS.map((k) => a[k])) === JSON.stringify(SETTING_KEYS.map((k) => b[k]));
/** Windows 版：没有钥匙串和系统授权（密钥存数据目录，采集不用授权） */
const IS_WIN = /Windows/.test(navigator.userAgent);
const KEY_SOURCE = { keychain: tr("已存（钥匙串）"), file: tr("已存（数据目录）"), env: tr("在用环境变量里的") };
const KEY_STORE = IS_WIN ? tr("数据目录") : tr("钥匙串");
/** 暂停多久（设置 → 记录 → 暂停） */
const PAUSES = [["60", tr("1 小时")], ["morning", tr("到明早 8 点")], ["on", tr("一直")]];
const DELETES = [["15", tr("最近 15 分钟")], ["60", tr("最近 1 小时")], ["today", tr("今天全部")]];
const SS = { deleting: null, deleted: null, advanced: false };
/** 项目列表（设置页进来时取一次，改过后重取） */
const PJ = { err: null, list: null };

function profileRow() {
  const pf = S.profile || { name: tr("我") };
  return `<div class="mx-setting"><div class="profile"><span class="avatar big">${avatarHtml(pf)}</span><div><div class="mx-setting-name">${esc(pf.name)}</div><div class="mx-setting-desc">${pf.email ? esc(pf.email) : tr("显示在左下角")}</div></div></div>
      <button class="mx-btn mx-btn--secondary mx-btn--sm" id="pf-edit">${tr("编辑")}</button></div>`;
}

/** 选的图片居中裁成方的、缩到 128 像素，转成 JPEG data URL */
function shrinkImage(file) {
  return new Promise((ok, fail) => {
    const img = new Image();
    img.onload = () => {
      const s = Math.min(img.width, img.height), c = document.createElement("canvas");
      c.width = c.height = 128;
      c.getContext("2d").drawImage(img, (img.width - s) / 2, (img.height - s) / 2, s, s, 0, 0, 128, 128);
      ok(c.toDataURL("image/jpeg", 0.85));
    };
    img.onerror = () => fail(tr("读不了这张图片"));
    img.src = URL.createObjectURL(file);
  });
}

/** 编辑头像和名称（小弹层）。avatar：undefined＝没动，null＝改用名字首字 */
function profileDialog() {
  const pf = S.profile || { name: tr("我") };
  let avatar;
  openDialog({
    title: tr("编辑个人资料"), ok: tr("保存"),
    body: `<div class="profile"><span class="avatar big" id="pf-av">${avatarHtml(pf)}</span>
        <div class="btns sync-btns"><button class="mx-btn mx-btn--secondary mx-btn--sm" id="pf-pick">${tr("换头像")}</button><button class="mx-btn mx-btn--text mx-btn--sm" id="pf-clear">${tr("用名字首字")}</button><input type="file" id="pf-file" accept="image/*" hidden></div></div>
      ${dlgField("pf-name", tr("名称"), { value: pf.name || "", help: "" })}`,
    onOpen: (el, d) => {
      const av = el.querySelector("#pf-av"), file = el.querySelector("#pf-file"), name = el.querySelector("#pf-name");
      name.maxLength = 40;
      const show = () => (av.innerHTML = avatarHtml({ name: name.value.trim() || pf.name, avatar: avatar === undefined ? pf.avatar : avatar }));
      name.oninput = show;
      el.querySelector("#pf-pick").onclick = () => file.click();
      el.querySelector("#pf-clear").onclick = () => { avatar = null; show(); };
      file.onchange = async () => { if (!file.files[0]) return; try { avatar = await shrinkImage(file.files[0]); d.setErr(null); show(); } catch (x) { d.setErr(String(x)); } };
    },
    submit: async (el) => {
      const name = dlgVal(el, "pf-name");
      if (!name) throw tr("名称不能空。");
      await call("save_profile", { name, avatar: avatar === undefined ? pf.avatar || null : avatar });
      S.profile = await call("get_profile"); renderSide(); renderSettings();
    },
  });
}

function bindProfile() {
  const b = document.getElementById("pf-edit");
  if (b) b.onclick = profileDialog;
}

const DOMAINS = ["工作", "成长", "生活", "健康", "关系", "休闲"];

function projectsSection() {
  const list = PJ.list || [];
  if (!list.length) return "";
  const dur = (sec) => (sec < 3600 ? tr("{n} 分钟", { n: Math.round(sec / 60) }) : tr("{h} 小时", { h: hh(sec) }));
  const byDom = DOMAINS.map((d) => [d, list.filter((p) => p.domain === d)]).filter(([, xs]) => xs.length);
  const rows = byDom.map(([d, xs]) => `<div class="pj-group">${tr(d)}</div>` + xs.map((p) => `<div class="pj-row" data-p="${esc(p.name)}">
      <div class="pj-main"><div class="mx-setting-name">${esc(p.name)}</div><div class="mx-setting-desc">${tr("累计 {d}{a}", { d: dur(p.sec), a: p.aliases.length ? tr(" · 也叫 {x}", { x: esc(p.aliases.join(tr("、"))) }) : "" })}</div></div>
      <button class="mx-btn mx-btn--text mx-btn--sm" data-pj-edit>${tr("编辑")}</button></div>`).join("")).join("");
  return `<p class="note pj-note">${tr("项目由摘要自动判断，你改过的不会被覆盖")}</p><div class="pj">${rows}</div>${PJ.err ? `<div class="mx-field-error">${esc(PJ.err)}</div>` : ""}`;
}

/** 编辑一个项目（小弹层）：改领域，或者合并到另一个项目 */
function projectDialog(p) {
  const others = (PJ.list || []).filter((q) => q.name !== p.name);
  const why = tr("同一件事的两个叫法，合并成一个");
  let domain = p.domain, into = "";
  openDialog({
    title: tr("编辑「{name}」", { name: p.name }), ok: tr("保存"),
    body: `<div class="mx-field" id="pj-dom-f"><label>${tr("领域")}</label><div class="mx-seg" role="tablist" id="pj-dom">${DOMAINS.map((x) => `<button role="tab" aria-selected="${x === domain}" class="${x === domain ? "is-on" : ""}" data-v="${x}">${tr(x)}</button>`).join("")}</div></div>
      ${others.length ? `<div class="mx-field"><label>${tr("合并到")}</label><div class="pick-list" role="listbox" id="pj-into"><button role="option" aria-selected="true" data-v=""><span>${tr("不合并")}</span>${ICON.check}</button>${others.map((q) => `<button role="option" aria-selected="false" data-v="${esc(q.name)}"><span>${esc(q.name)}</span><span class="mx-menu-val">${esc(tr(q.domain))}</span>${ICON.check}</button>`).join("")}</div>
        <p class="note" id="pj-why">${why}</p></div>` : ""}`,
    onOpen: (el, d) => {
      el.querySelector("#pj-dom").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        domain = b.dataset.v;
        el.querySelectorAll("#pj-dom button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
      };
      const lst = el.querySelector("#pj-into"); if (!lst) return;
      lst.onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        into = b.dataset.v;
        lst.querySelectorAll("button").forEach((x) => x.setAttribute("aria-selected", x === b));
        el.querySelector("#pj-why").textContent = into ? tr("记成「{a}」的时间都算到「{b}」", { a: p.name, b: into }) : why;
        el.querySelector("#pj-dom-f").hidden = !!into;
        d.setOk(into ? tr("合并到「{name}」", { name: into }) : tr("保存"));
      };
    },
    submit: async () => {
      if (into) await call("merge_project", { from: p.name, into });
      else if (domain !== p.domain) await call("set_project_domain", { project: p.name, domain });
      else return;
      PJ.err = null; renderProjects();
    },
  });
}

function bindProjects() {
  $app.querySelectorAll("[data-pj-edit]").forEach((b) => (b.onclick = () => projectDialog(PJ.list.find((p) => p.name === b.closest("[data-p]").dataset.p))));
}

/** 账号：msg＝页面上留一句结果；confirmLogout＝退出登录等确认。登录、注册、改密码这些都在小弹层里 */
const AC = { msg: null, confirmLogout: false };

const checkEmail = (e) => {
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e)) throw tr("邮箱格式不对。");
  // 常见手滑：.con、gmial 这类，邮箱错了以后登录不上
  if (/\.(con|cmo|cm|comm|om|vom|xom)$/i.test(e) || /@(gmial|gamil|gmai|gmal|gmaill|qq\.con)\b/i.test(e)) throw tr("邮箱像是拼错了，检查一下");
  return e;
};
const checkPw = (el) => {
  const a = el.querySelector("#pw1").value, b = el.querySelector("#pw2");
  if (a.length < 8) throw tr("主密码至少 8 位。");
  if (b && b.value !== a) throw tr("两次输入的主密码不一样。");
  return a;
};
/** 账号变了：登录着的设备列表重取，当前页重画（设置页和设备页都能开这些弹层） */
const accountChanged = () => { DS.account = null; SS.llm = null; return renderSettings(); };
/** 官方服务要登录：同步用的是 Beayy 服务或还没同步，登录就是开 Beayy 服务同步；用 iCloud、网盘、文件夹时只为官方服务登，同步不变 */
const officialLogin = () => loginDialog("login", "", !["none", "server"].includes(syncWay(S.sync)));

/** 登录、注册、用恢复码重设（小弹层，底下可以互相切换） */
function loginDialog(mode = "login", email = "", svc = false, after = accountChanged) {
  const sy = S.sync || {};
  const cmd = (c) => (svc ? c.replace("account_", "account_service_") : c);
  const em = dlgField("em", tr("邮箱"), { ph: "you@example.com", value: email || sy.email || "" });
  const pw2 = dlgField("pw2", tr("再输一次"), { type: "password" });
  const forms = {
    login: [tr("登录"), em + dlgField("pw1", tr("主密码"), { type: "password" }), tr("登录"), tr("登录中…")],
    signup: [tr("注册"), em + dlgField("pw1", tr("主密码"), { type: "password", ph: tr("至少 8 位，记得住又别人猜不到") }) + pw2
      + `<div id="inv-box" hidden>${dlgField("inv", tr("邀请码"), { type: "password", ph: tr("这台服务器注册要邀请码") })}</div><p class="note" id="inv-note"></p>`, tr("注册并登录"), tr("注册中…")],
    recover: [tr("用恢复码重设主密码"), em + dlgField("rc", tr("恢复码"), { ph: "XXXX-XXXX-…", mono: true }) + dlgField("pw1", tr("新主密码"), { type: "password", ph: tr("至少 8 位") }) + pw2, tr("重设并登录"), tr("重设中…")],
  };
  const links = { login: [["signup", tr("没有账号？注册")], ["recover", tr("忘了主密码")]], signup: [["login", tr("已经有账号？登录")]], recover: [["login", tr("想起来了？登录")]] }[mode];
  const [title, body, ok, busy] = forms[mode];
  // 登到哪台服务器：手机和电脑要选同一台，不然同一个邮箱在另一台上登不进去
  const cf = S.settings || {};
  const server = cf.sync_server ? `<p class="note">${tr("服务器 {host}", { host: `<span class="mono">${esc(hostOf(cf.sync_server))}</span>` })}${sy.configured ? "" : ` <button class="mx-btn mx-btn--text mx-btn--sm" data-server>${tr("更改")}</button>`}</p>` : "";
  openDialog({
    title, ok, busy,
    body: (mode === "recover" ? `<p class="note">${tr("重设后，其他设备要重新登录")}</p>` : `<p class="note">${!svc ? tr("记录在本机加密后再同步，服务器读不到内容") : syncWay(S.sync) === "none" ? tr("只用来开官方模型、转写和地图") : gap(tr("只用来开官方模型、转写和地图，还用{way}同步", { way: wayName(syncWay(S.sync)) }))}</p>`) + body
      + server + `<div class="dlg-links">${links.map(([k, v]) => `<button class="mx-btn mx-btn--text mx-btn--sm" data-to="${k}">${v}</button>`).join("")}</div>`,
    onOpen: (el) => {
      el.querySelectorAll("[data-to]").forEach((b) => (b.onclick = () => loginDialog(b.dataset.to, dlgVal(el, "em"), svc, after)));
      const sv = el.querySelector("[data-server]");
      if (sv) sv.onclick = () => { const em = dlgVal(el, "em"); syncServerDialog(() => loginDialog(mode, em, svc, after)); };
      // 注册：问服务器要不要邀请码；开放注册的写明每个账号能存多少，满了怎么办
      if (mode === "signup") call("signup_info").then((i) => {
        const box = el.querySelector("#inv-box"), note = el.querySelector("#inv-note");
        if (!box) return;
        box.hidden = !i.invite;
        note.textContent = i.invite ? tr("没有邀请码也能用：记录都在本机；要多台设备同步，可以在「设置 → 连接 → 同步方式」里选同步文件夹，或自己部署服务端")
          : i.open === false ? tr("这台服务器暂停注册了。可以先不登录（记录都在本机），或者在「设置 → 连接 → 同步方式」里选同步文件夹")
          : i.quota && !svc ? tr("每个账号能存 {q}，满了以后新记录只在本机，不再上传", { q: bytesText(i.quota) }) : "";
      }).catch(() => { const b = el.querySelector("#inv-box"); if (b) b.hidden = false; });
    },
    submit: async (el) => {
      const email = checkEmail(dlgVal(el, "em"));
      AC.msg = null;
      if (mode === "login") await call(cmd("account_login"), { email, password: el.querySelector("#pw1").value });
      else if (mode === "signup") { const code = await call(cmd("account_signup"), { invite: dlgVal(el, "inv"), email, password: checkPw(el) }); setTimeout(() => codeDialog(code, true)); }
      else { await call(cmd("account_recover"), { email, code: dlgVal(el, "rc"), password: checkPw(el) }); AC.msg = tr("已重设，其他设备要重新登录"); }
      after();
    },
  });
}

/** 早期配对码账号设邮箱和主密码 */
function claimDialog() {
  openDialog({
    title: tr("设邮箱和主密码"), ok: tr("设好账号"), busy: tr("设置中…"),
    body: `<p class="note">${tr("设好后，别的设备用邮箱和主密码登录")}</p>`
      + dlgField("em", tr("邮箱"), { ph: "you@example.com" }) + dlgField("pw1", tr("主密码"), { type: "password", ph: tr("至少 8 位，记得住又别人猜不到") }) + dlgField("pw2", tr("再输一次"), { type: "password" }),
    submit: async (el) => {
      const email = checkEmail(dlgVal(el, "em")), code = await call("account_claim", { email, password: checkPw(el) });
      AC.msg = null; accountChanged(); setTimeout(() => codeDialog(code, true));
    },
  });
}

function passwordDialog() {
  openDialog({
    title: tr("改主密码"), ok: tr("改主密码"), busy: tr("改密码中…"),
    body: `<p class="note">${tr("以后登录新设备用新主密码")}</p>`
      + dlgField("pw1", tr("新主密码"), { type: "password", ph: tr("至少 8 位") }) + dlgField("pw2", tr("再输一次"), { type: "password" }),
    submit: async (el) => { await call("account_password", { password: checkPw(el) }); AC.msg = tr("主密码已改。"); renderSettings(); },
  });
}

/** 恢复码。fresh＝刚注册或刚设好账号：必须点「我已存好」才关 */
function codeDialog(code, fresh) {
  openDialog({
    title: fresh ? tr("存好恢复码") : tr("恢复码"), closable: !fresh, cancel: fresh ? false : tr("关闭"), ok: fresh ? tr("我已存好") : tr("复制恢复码"),
    body: `<div class="code-box"><code class="mono">${esc(code)}</code>${fresh ? `<button class="mx-btn mx-btn--secondary mx-btn--sm" id="copy-rc" autofocus>${tr("复制")}</button>` : ""}</div>
      <p class="note warn-text">${tr("恢复码是忘了主密码时找回数据的唯一办法：存进密码管理器或抄在纸上。主密码和恢复码都丢了，云端的数据就再也解不开，我们也帮不了。")}</p>`,
    onOpen: (el, d) => {
      const b = el.querySelector("#copy-rc");
      if (b) b.onclick = async () => { try { await navigator.clipboard.writeText(code); b.textContent = tr("已复制"); } catch (x) { d.setErr(tr("复制不了，手动选中复制。")); } };
    },
    submit: async () => { if (!fresh) { try { await navigator.clipboard.writeText(code); } catch (x) { throw tr("复制不了，手动选中复制。"); } } },
  });
}

/** 不经服务器的同步（iCloud、网盘、同步文件夹）：第一台新开（设主密码，给恢复码）、别的加入（输主密码）、用恢复码重设。
 *  命令按方式取名：folder_create、icloud_join、webdav_recover…；网盘另带地址、账号、密码 */
function vaultDialog(mode, way, cred = {}) {
  const where = wayName(way);
  const forms = {
    create: [gap(tr("在{where}新开同步", { where })), `<p class="note">${gap(tr("记录加密后放进{where}，{where}读不到内容。主密码用来解开，别的设备加入时要输", { where }))}</p>` + dlgField("pw1", tr("主密码"), { type: "password", ph: tr("至少 8 位，记得住又别人猜不到") }) + dlgField("pw2", tr("再输一次"), { type: "password" }), tr("新开同步")],
    join: [gap(tr("加入{where}里的同步", { where })), `<p class="note">${tr("输第一台设备上设的主密码")}</p>` + dlgField("pw1", tr("主密码"), { type: "password" }), tr("加入")],
    recover: [tr("用恢复码重设主密码"), `<p class="note">${tr("重设后，别的设备下次加入用新主密码")}</p>` + dlgField("rc", tr("恢复码"), { ph: "XXXX-XXXX-…", mono: true }) + dlgField("pw1", tr("新主密码"), { type: "password", ph: tr("至少 8 位") }) + dlgField("pw2", tr("再输一次"), { type: "password" }), tr("重设并加入")],
  };
  const [title, body, ok] = forms[mode];
  openDialog({
    title, ok, busy: tr("处理中…"),
    body: body + (mode === "join" ? `<div class="dlg-links"><button class="mx-btn mx-btn--text mx-btn--sm" data-to="recover">${tr("忘了主密码")}</button></div>` : ""),
    onOpen: (el) => el.querySelectorAll("[data-to]").forEach((b) => (b.onclick = () => vaultDialog(b.dataset.to, way, cred))),
    submit: async (el) => {
      AC.msg = null;
      if (mode === "create") { const code = await call(`${way}_create`, { ...cred, password: checkPw(el) }); setTimeout(() => codeDialog(code, true)); }
      else if (mode === "join") await call(`${way}_join`, { ...cred, password: el.querySelector("#pw1").value });
      else { await call(`${way}_recover`, { ...cred, code: dlgVal(el, "rc"), password: checkPw(el) }); AC.msg = tr("已重设主密码"); }
      accountChanged();
    },
  });
}

/** 到明早 8 点还有几分钟 */
function minutesToMorning() {
  const now = new Date(), t = new Date(now);
  t.setHours(8, 0, 0, 0);
  if (t <= now) t.setDate(t.getDate() + 1);
  return Math.ceil((t - now) / 60000);
}

// ---------- 设置：地图（高德 Key、常去的地方） ----------

const PL = { labels: [] };
const PL_TYPES = [["home", tr("家")], ["work", tr("工作")], ["other", tr("其他")]];
const PL_ICON = { home: tr("家"), work: tr("工作") };

/** 改了常去的地方以后：在哪一页就重画哪一页 */
const afterPlaces = () => (S.page === "settings" ? renderSettings() : renderToday());

/** 改常去的地方里的一项：名字、地址、种类、范围；名字清空＝删掉 */
async function labelDialog(i) {
  PL.labels = await call("list_place_labels").catch(() => PL.labels);
  const l = PL.labels[i]; if (!l) return;
  let kind = l.type || "other", r = Math.round(l.r || 150);
  const seg = (id, items, cur) => `<div class="mx-seg" role="tablist" id="${id}">${items.map(([k, t]) => `<button role="tab" aria-selected="${String(k) === String(cur)}" class="${String(k) === String(cur) ? "is-on" : ""}" data-v="${k}">${t}</button>`).join("")}</div>`;
  openDialog({
    title: tr("改「{name}」", { name: l.label }), ok: tr("保存"),
    body: dlgField("pl-n", tr("名字"), { value: l.label, help: tr("清空就删掉这个地方。") })
      + dlgField("pl-a", tr("地址"), { value: l.address || "", ph: tr("比如 某某小区 3 幢") })
      + `<div class="mx-field"><label>${tr("种类")}</label>${seg("pl-k", PL_TYPES, kind)}</div>`
      + `<div class="mx-field"><label>${tr("范围")}</label>${seg("pl-r", [[100, tr("{n} 米", { n: 100 })], [150, tr("{n} 米", { n: 150 })], [250, tr("{n} 米", { n: 250 })], [400, tr("{n} 米", { n: 400 })]], r)}<p class="note">${tr("离得近的两个地方，范围别重叠")}</p></div>`,
    onOpen: (el) => {
      const bindSeg = (id, set) => { el.querySelector("#" + id).onclick = (e) => { const b = e.target.closest("[data-v]"); if (!b) return; set(b.dataset.v); el.querySelectorAll(`#${id} button`).forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); }); }; };
      bindSeg("pl-k", (v) => (kind = v)); bindSeg("pl-r", (v) => (r = +v));
    },
    submit: async (el) => { await call("edit_place_label", { index: i, label: dlgVal(el, "pl-n"), kind, r, address: dlgVal(el, "pl-a") || null }); afterPlaces(); },
  });
}

/** 添加一个常去的地方：搜高德（以第一个常去的地方为中心，按距离），挑一个就加上，名字先用店名，之后可以改 */
function addPlaceDialog() {
  const c = PL.labels[0];
  let found = [];
  openDialog({
    title: tr("添加一个地方"), cancel: tr("关闭"),
    body: `<div class="mx-field"><label for="pa-q">${tr("搜小区、楼栋、店名、公司")}</label><input class="mx-input" id="pa-q" autocomplete="off"><div class="pick-list" id="pa-list"></div></div>`,
    onOpen: (el, d) => {
      let timer = null;
      el.querySelector("#pa-q").oninput = (e) => {
        clearTimeout(timer);
        const q = e.target.value.trim(), box = el.querySelector("#pa-list");
        if (!q) { box.innerHTML = ""; return; }
        timer = setTimeout(async () => {
          try { found = await call("amap_search", { text: q, lat: c ? c.lat : null, lon: c ? c.lon : null }); }
          catch (x) { box.innerHTML = `<p class="note">${esc(String(x))}</p>`; return; }
          if (el.querySelector("#pa-q").value.trim() !== q) return;
          box.innerHTML = found.slice(0, 8).map((p, i) => `<button class="pick-row" data-add="${i}"><span class="pk-n">${esc(p.name)}<span class="pk-t">${esc((p.type || "").split(";")[1] || "")}</span></span><span class="pk-r mono">${c && p.distance != null ? (p.distance < 1000 ? tr("{n} 米", { n: Math.round(p.distance) }) : tr("{n} 公里", { n: (p.distance / 1000).toFixed(1) })) : ""}</span></button>`).join("") || `<p class="note">${tr("没搜到。")}</p>`;
        }, 350);
      };
      el.addEventListener("click", async (e) => {
        const b = e.target.closest("[data-add]"); if (!b) return;
        const p = found[+b.dataset.add];
        try { await call("set_place_label", { lat: p.lat, lon: p.lon, label: shopShort(p.name), kind: "other" }); closeDialog(); afterPlaces(); }
        catch (x) { d.setErr(tr("没加上：{err}", { err: x })); }
      });
    },
  });
}

/** 设置分六页，放在页头的页签里：通用｜账号｜记录｜连接｜数据｜项目。改了就存，不再有「保存设置」；要填几项的（地址、模型、密钥）开小弹层 */
const SET_TABS = [["general", tr("通用")], ["record", tr("记录")], ["ai", "AI"], ["phone", tr("手机与手表")], ["features", tr("功能")], ["data", tr("数据")]];
// 以前的分页（账号、连接、项目）记在本机的，换成现在的
try { const t = localStorage.getItem("settings-tab") || "general"; SS.tab = { account: "data", connect: "ai", projects: "general" }[t] || t; } catch (e) { SS.tab = "general"; }
const hostOf = (u) => { try { return new URL(u).host; } catch (e) { return u || ""; } };
/** 路径里的家目录写成 ~，iCloud 云盘写成「iCloud 云盘」 */
const tildePath = (p) => String(p || "").replace(/^\/Users\/[^/]+\/Library\/Mobile Documents\/com~apple~CloudDocs/, tr("iCloud 云盘")).replace(/^\/Users\/[^/]+/, "~");
const setRow = (name, desc, right) => `<div class="mx-setting"><div class="mx-setting-main"><div class="mx-setting-name">${name}</div>${desc ? `<div class="mx-setting-desc">${desc}</div>` : ""}</div>${right}</div>`;
const smallBtn = (id, text, attrs = "") => `<button class="mx-btn mx-btn--secondary mx-btn--sm" id="${id}" ${attrs}>${text}</button>`;
const pill = (ok, text) => `<span class="mx-status mx-status--${ok ? "ok" : "warn"}">${text}</span>`;
/** 只改几个键：save_settings 只写传进去的键 */
async function saveSome(part) {
  await call("save_settings", { settings: part });
  S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings));
}
const isUrl = (v) => /^https?:\/\/\S+$/.test(v);
const KEEP = tr("已存，留空不改");

/** 设置：电脑上是盖在当前页上的大弹层（照 Claude），关掉回到原来那页；手机（窄屏）还是整页 */
const SET = { back: "#/today" };
const setModal = () => !NARROW.matches && !(S.status && S.status.mobile);
function setHost() {
  if (!setModal()) return $app;
  let root = document.getElementById("set-root");
  if (!root) {
    root = document.createElement("div");
    root.id = "set-root"; root.className = "mx-scrim set-root";
    root.innerHTML = `<div class="mx-dialog set-dialog" role="dialog" aria-modal="true" aria-labelledby="set-title">
      <div class="mx-dialog-head"><h2 id="set-title">${tr("设置")}</h2><button class="mx-iconbtn" id="set-close" aria-label="${tr("关闭")}">${ICON.close}</button></div>
      <div class="mx-dialog-body" id="set-body"></div></div>`;
    document.body.appendChild(root);
    root.addEventListener("mousedown", (e) => { if (e.target === root) closeSettings(); });
    root.querySelector("#set-close").onclick = closeSettings;
  }
  return root.querySelector("#set-body");
}
function closeSettings() { location.hash = SET.back || "#/today"; }
function dropSettings() { const r = document.getElementById("set-root"); if (r) r.remove(); }

const tokText = (n) => (n >= 1e4 ? tr("{n} 万 token", { n: numText(Math.round(n / 1e4)), k: numText(Math.round(n / 1e3)) }) : tr("{n} token", { n: numText(n) }));
/** 官方的某一项（模型、转写、地图）现在怎样：要登录、没开通，或者本月用了多少（上限 0＝不限，负数＝这一项没开） */
function officialDesc(u, fmt) {
  const L = SS.llm;
  if (!L) return "";
  if (L.error) return tr("要登录 Beayy 账号");
  if (L.plan === "stopped") return tr("已停用");
  if (L.plan !== "official") return tr("这个账号没开通");
  if (L.until && L.until < Date.now()) return tr("试用已结束");
  const q = (u && u.quota) || 0;
  return q < 0 ? tr("这一项没开通") : (L.until ? tr("试用到 {d}", { d: mmdd(L.until) }) + " · " : "") + tr("本月 {u} / {q}", { u: fmt((u && u.used) || 0), q: q ? fmt(q) : tr("不限") });
}
/** 毫秒时刻 → 本地的「11-12」 */
const mmdd = (ms) => { const t = new Date(ms); return `${String(t.getMonth() + 1).padStart(2, "0")}-${String(t.getDate()).padStart(2, "0")}`; };

/** 设置「AI」页：写总结用的模型（本月用量写在这一行）、语言、棒喝；接到别的 AI */
function aiBody(cf, st) {
  const mobile = !!st.mobile, official = cf.model_mode === "official", ks = cf.api_key_source;
  const used30 = SS.usage && !SS.usage.error ? (SS.usage.days || []).reduce((a, d) => a + d.in + d.out, 0) : null;
  const model = official ? [tr("官方"), officialDesc(SS.llm && SS.llm.llm, tokText)].filter(Boolean).join(" · ")
    : `${tr("自己的")} · <span class="mono">${esc(hostOf(cf.gateway))} · ${esc(cf.model_segment || tr("没填模型"))}</span> · ${ks ? tr("密钥已存") : tr("没有密钥")}${used30 != null ? ` · ${tr("近 30 天 {n}", { n: tokText(used30) })}` : ""}`;
  const login = official && SS.llm && SS.llm.error ? smallBtn("ac-login", tr("登录")) : "";
  const seg = (id, list, cur) => `<div class="mx-seg" role="tablist" id="${id}">${list.map(([k, v]) => `<button role="tab" aria-selected="${k === cur}" class="${k === cur ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div>`;
  return section(tr("总结"), setRow(tr("模型"), model, `<span class="btns">${mobile ? "" : `<button class="mx-btn mx-btn--text mx-btn--sm" id="c-usage">${tr("用量")}</button>`}${login}${smallBtn("c-model", tr("更改"))}</span>`)
      + setRow(tr("语言"), "", seg("sum-lang", SUM_LANGS, cf.summary_language || ""))
      + setRow(tr("棒喝"), tr("日总结、周总结多一段狠话，low 不带脏字"), seg("roast", ROASTS, cf.roast || "off")))
    + (mobile ? "" : section(tr("连接"), setRow(tr("本机 AI 工具"), tr("Claude、Cursor 能读你的记录，只读，数据不出这台电脑"), smallBtn("c-mcp", tr("接入方法")))));
}

/** 设置「手机与手表」页的服务：手表录音转文字、停留点取名用的地图（用官方的写本月用量） */
function servicesRows(cf, st) {
  const asrOfficial = cf.transcribe_server === cf.default_transcribe_server, mapOfficial = cf.map_mode === "official", osm = cf.map_provider === "osm";
  const L = SS.llm || {};
  const amap = cf.has_amap_key && cf.has_amap_js ? tr("Key 已存") : cf.has_amap_key ? tr("缺地图 Key") : tr("还没有 Key");
  const minutes = (sec) => tr("{n} 分钟", { n: numText(Math.round(sec / 60)) }), calls = (n) => tr("{n} 次", { n: numText(n) });
  const asr = [asrOfficial ? tr("官方") : `${tr("自己的")} · <span class="mono">${esc(hostOf(cf.transcribe_server))}</span>`, asrOfficial ? officialDesc(L.asr, minutes) : "", tr("你叫「{name}」", { name: esc(cf.speaker_name) })].filter(Boolean).join(" · ")
    + (st.cloud_sync ? `<br>${esc(st.cloud_sync)}` : "");
  // 说明都按「谁的 · 用的什么 · 状态」排
  const map = osm ? tr("OpenStreetMap · 免费，不用 Key") : [mapOfficial ? tr("官方") : tr("自己的"), tr("高德地图"), mapOfficial ? officialDesc(L.map, calls) : amap].filter(Boolean).join(" · ");
  const login = L.error && (asrOfficial || (mapOfficial && !osm)) ? smallBtn("ac-login", tr("登录")) : "";
  const btns = (id) => `<span class="btns">${login}${smallBtn(id, tr("更改"))}</span>`;
  return setRow(tr("录音转写"), asr, asrOfficial ? btns("c-asr") : smallBtn("c-asr", tr("更改"))) + setRow(tr("地图"), map, !asrOfficial && mapOfficial && !osm ? btns("c-amap") : smallBtn("c-amap", tr("更改")));
}

async function renderSettings() {
  // 弹层里的操作（登录、改资料）完了会叫它；已经不在设置了就不画
  if (S.page !== "settings") return;
  const tk = RT.seq;
  if (!S.settings) { S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings)); }
  const mobile = !!(S.status && S.status.mobile);
  // 手机版不采集：没有「记录」页（暂停、不记录的 app、删最近记录都是电脑上的事），也没有「功能」（游戏只在电脑上）
  const tabsHere = mobile ? SET_TABS.filter(([k]) => k !== "record" && k !== "features") : SET_TABS;
  const tab = tabsHere.some(([k]) => k === SS.tab) ? SS.tab : "general";
  // 几样互不相干，一起问（原来一个等一个）
  const [sy, prof, st0, up, devs] = await Promise.all([
    call("sync_status").catch(() => ({})),
    call("get_profile").catch(() => S.profile),
    call("get_status"),
    TAURI ? call("update_status").catch(() => S.update) : S.update,
    tab === "data" ? call("list_devices").catch(() => []) : DS.devs,
  ]);
  S.sync = sy; S.profile = prof; S.status = st0; S.update = up;
  if (tab === "data") { DS.devs = devs; DS.names = Object.fromEntries((devs || []).map((d) => [d.id, d.name])); }
  renderSide();
  const st = S.status, cf = S.settings;
  const theme = document.documentElement.dataset.theme || "";
  let langPref = ""; try { langPref = localStorage.getItem("lang") || ""; } catch (e) {}
  // 照 Claude 设置页：左边一列分区（设计系统 SubNav），窄屏收成一排
  const tabs = `<nav class="mx-subnav" id="set-tabs" aria-label="${tr("设置分区")}">${tabsHere.map(([k, v]) =>
    `<button class="${k === tab ? "is-on" : ""}" data-st="${k}"${k === tab ? ' aria-current="page"' : ""}>${v}${k === "general" && updateReady() ? `<i class="sub-dot" title="${esc(tr("新版本 {v} 已就绪", { v: S.update.latest }))}"></i>` : ""}</button>`).join("")}</nav>`;

  let body = "";
  if (tab === "general") {
    // 电脑上个人资料在左下角菜单里改；手机没有那个菜单，放这里
    body = (mobile ? section(tr("个人资料"), profileRow()) : "")
      + group((mobile ? "" : setRow(tr("开机启动"), "", `<button class="mx-switch" role="switch" id="login" aria-checked="${!!st.login_item}" aria-label="${tr("开机启动")}"></button>`))
        + setRow(tr("主题"), "", `<div class="mx-seg" role="tablist" id="theme">${THEMES.map(([k, v]) => `<button role="tab" aria-selected="${k === theme}" class="${k === theme ? "is-on" : ""}" data-theme="${k}">${v}</button>`).join("")}</div>`)
        + setRow(tr("语言"), "", `<div class="mx-seg" role="tablist" id="lang">${LANGS.map(([k, v]) => `<button role="tab" aria-selected="${k === langPref}" class="${k === langPref ? "is-on" : ""}" data-lang="${k}">${v}</button>`).join("")}</div>`)
        + (mobile ? "" : setRow(tr("字体"), fontDesc(), smallBtn("font-edit", tr("更改")))))
      + (mobile ? section(tr("关于"), setRow(tr("版本"), `${esc(st.version || "")} · ${tr("跟着 TestFlight 更新")}`, "")) : aboutSection());
  } else if (tab === "record") {
    const pause = st.paused
      ? setRow(tr("暂停记录"), st.paused_until ? tr("已暂停，到 {t} 自动继续", { t: hm(st.paused_until) }) : tr("已暂停，点继续才恢复"), smallBtn("pause-off", tr("继续")))
      : setRow(tr("暂停记录"), "", smallBtn("pause-open", tr("暂停")));
    const block = `<div class="block"><div><div class="mx-setting-name">${tr("不记录的 app")}</div><div class="mx-setting-desc">${tr("在前台时只记用了多久，不记内容")}</div></div>
        ${cf.blocklist.length ? `<div class="mx-chips" id="chips">${cf.blocklist.map((b, i) => `<button class="mx-chip" data-i="${i}" title="${tr("移出")}">${esc(b)}${ICON.close}</button>`).join("")}</div>` : ""}
        <div class="inline"><input class="mx-input mono" id="block-new" placeholder="${tr("app 名或 bundle id，比如 1Password")}" aria-label="${tr("不记录的 app")}"><button class="mx-btn mx-btn--secondary" id="block-add">${tr("加入")}</button></div></div>`;
    const del = setRow(tr("删除最近记录"), SS.deleted != null ? tr("刚删掉 {n} 条", { n: SS.deleted }) : tr("比如刚输过的私人内容"), smallBtn("del-open", tr("删除")));
    body = group(pause + block + del);
  } else if (tab === "ai") {
    body = aiBody(cf, st);
  } else if (tab === "phone") {
    body = (mobile ? phoneSection() : "") + (mobile ? section(tr("服务"), servicesRows(cf, st)) : group(servicesRows(cf, st)));
  } else if (tab === "features") {
    // 可选的功能，默认都关；每台电脑各自开
    // 游戏：总开关默认关；打开后下面每款各一个开关，默认开
    const sub = cf.games ? `<div class="set-sub">${Object.entries(GM_GAMES).map(([k, v]) => setRow(esc(tr(v)), tr(GM_DESC[k]), `<button class="mx-switch" role="switch" data-game-on="${k}" aria-checked="${cf["games_" + k] !== false}" aria-label="${esc(tr(v))}"></button>`)).join("")}</div>` : "";
    body = group(setRow(tr("游戏"), tr("侧栏出现「游戏」，记下面开着的游戏"), `<button class="mx-switch" role="switch" id="games-on" aria-checked="${!!cf.games}" aria-label="${tr("游戏")}"></button>`) + sub);
  } else {
    body = syncPanel(sy) + devicesPanel(devs || [], st) + (SS.storage ? storageView(SS.storage) : "");
  }
  if (stale(tk)) return;
  const host = setHost();
  host.innerHTML = (host === $app ? header(tr("设置")) : "") + `<div class="set-wrap">${tabs}<div class="form">${body}</div></div>`;

  document.getElementById("set-tabs").onclick = (e) => {
    const b = e.target.closest("[data-st]"); if (!b) return;
    SS.tab = b.dataset.st; try { localStorage.setItem("settings-tab", SS.tab); } catch (x) {}
    AC.confirmLogout = false;
    renderSettings();
  };
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  const segSave = (id, key, num) => { const el = document.getElementById(id); if (el) el.onclick = async (e) => { const b = e.target.closest("[data-v]"); if (!b) return; await saveSome({ [key]: num ? +b.dataset.v : b.dataset.v }); renderSettings(); }; };
  if (tab === "general") {
    if (mobile) bindProfile(); else bindAbout();
    document.getElementById("theme").onclick = (e) => {
      const b = e.target.closest("button"); if (!b) return;
      const t = b.dataset.theme;
      if (t) document.documentElement.dataset.theme = t; else delete document.documentElement.dataset.theme;
      try { t ? localStorage.setItem("theme", t) : localStorage.removeItem("theme"); } catch (err) {}
      renderSettings();
    };
    // 换语言重新加载：常量和侧栏是加载时按语言生成的
    document.getElementById("lang").onclick = (e) => {
      const b = e.target.closest("button"); if (!b || b.dataset.lang === langPref) return;
      try { b.dataset.lang ? localStorage.setItem("lang", b.dataset.lang) : localStorage.removeItem("lang"); } catch (err) {}
      location.reload();
    };
    on("login", async () => { try { await call("set_login_item", { on: !st.login_item }); } catch (x) { alert(tr("没改成：{err}", { err: x })); } renderSettings(); });
    on("font-edit", () => fontDialog(renderSettings));
  }
  if (tab === "features") {
    on("games-on", async () => { await saveSome({ games: !cf.games }); renderSide(); renderSettings(); });
    document.querySelectorAll("[data-game-on]").forEach((b) => (b.onclick = async () => { const k = "games_" + b.dataset.gameOn; await saveSome({ [k]: cf[k] === false }); renderSettings(); }));
  }
  if (tab === "record") {
    on("pause-off", async () => { await call("set_paused", { paused: false }); renderSettings(); });
    on("pause-open", () => choiceDialog({ title: tr("暂停记录"), options: PAUSES, note: tr("暂停期间不记录，到时间自动继续"), ok: tr("暂停"),
      go: async (k) => { await call("set_paused", { paused: true, minutes: k === "on" ? null : k === "morning" ? minutesToMorning() : +k }); renderSettings(); } }));
    on("del-open", () => choiceDialog({ title: tr("删除最近的记录"), options: DELETES, note: tr("删了找不回来"), ok: tr("删除"), okClass: "danger",
      go: async (k) => { SS.deleted = await call("delete_since", k === "today" ? { today: true } : { minutes: +k }); renderSettings(); } }));
    const setBlock = async (list) => { await saveSome({ blocklist: list }); renderSettings(); };
    const addBlock = () => { const v = document.getElementById("block-new").value.trim(); if (v && !cf.blocklist.includes(v)) setBlock([...cf.blocklist, v]); };
    on("block-add", addBlock);
    document.getElementById("block-new").onkeydown = (e) => { if (e.key === "Enter") addBlock(); };
    const chips = document.getElementById("chips");
    if (chips) chips.onclick = (e) => { const b = e.target.closest("[data-i]"); if (b) setBlock(cf.blocklist.filter((_, i) => i !== +b.dataset.i)); };
  }
  // 「AI」「手机与手表」都写官方额度：取一次，一分钟内不重取；电脑上「AI」另取近 30 天的用量
  if (tab === "ai" || tab === "phone") {
    const again = () => { if (S.page === "settings" && SS.tab === tab && !document.querySelector(".dlg-root")) renderSettings(); };
    if (!SS.llmLoading && (!SS.llm || Date.now() - SS.llmAt > 60e3)) {
      SS.llmLoading = true;
      call("llm_status").then((v) => { SS.llm = v; }).catch((x) => { SS.llm = { error: String(x) }; })
        .finally(() => { SS.llmAt = Date.now(); SS.llmLoading = false; again(); });
    }
    if (tab === "ai" && !mobile && !SS.usageLoading && (!SS.usage || Date.now() - SS.usageAt > 60e3)) {
      SS.usageLoading = true;
      call("llm_usage").then((v) => { SS.usage = v; }).catch((x) => { SS.usage = { error: String(x) }; })
        .finally(() => { SS.usageAt = Date.now(); SS.usageLoading = false; again(); });
    }
  }
  if (tab === "ai") {
    segSave("sum-lang", "summary_language");
    segSave("roast", "roast");
    on("c-model", modelDialog);
    on("c-usage", usageDialog);
    on("c-mcp", mcpDialog);
    on("ac-login", officialLogin);
  }
  if (tab === "phone") {
    on("ac-login", officialLogin);
    if (mobile) bindPhone();
    on("c-asr", asrDialog);
    on("c-amap", amapDialog);
  }
  clearInterval(SS.storageTimer);
  if (tab === "data") {
    bindData(sy);
    const load = async () => {
      try { SS.storage = await call("storage_stats"); } catch (x) { SS.storage = { error: String(x) }; }
      if (S.page === "settings" && SS.tab === "data" && !document.querySelector(".dlg-root")) renderSettings();
    };
    if (!SS.storage || Date.now() - (SS.storage.at || 0) > 60e3) load();
    SS.storageTimer = setInterval(load, 60e3);
    on("open-dir", () => call("open_data_dir"));
    on("repo-edit", () => openDialog({
      title: tr("人生记录仓库"), ok: tr("保存"),
      body: dlgField("repo", tr("路径"), { value: cf.repo_path || "", mono: true, ph: "~/人生记录", help: tr("日数据包和日总结的文件夹") }),
      submit: async (el) => { await saveSome({ repo_path: dlgVal(el, "repo") }); renderSettings(); },
    }));
  }
}

/** 手机版设置「通用」里的「这台手机」：定位、手表、用这台 iPhone 录、本人声纹、上传进度；安卓是使用情况和上传（原生做的，经 native_ 命令） */
function phoneSection() {
  const n = SS.native;
  if (!n) return section(tr("这台手机"), emptyInline(tr("在看这台手机的状态……")));
  if (n.error) return section(tr("这台手机"), `<div class="mx-field-error">${esc(n.error)}</div>`);
  const ago = (iso) => (iso ? mdhm(iso) : tr("还没有"));
  // 安卓：读每个 app 的使用时长（系统设置里允许「使用情况访问权限」），读到的随账号同步给电脑
  if (n.android) return section(tr("这台手机"),
    setRow(tr("使用情况"), n.usage ? tr("记每个 app 用了多久") : tr("在系统设置里允许 Beayy 读取使用情况"), `${pill(!!n.usage, n.usage ? tr("已允许") : tr("没允许"))}${n.usage ? "" : smallBtn("ph-usage", tr("去设置"))}`)
    + setRow(tr("上传"), `${tr("最近读 {t}", { t: ago(n.last_read) })}${n.last_error ? `<br>${esc(n.last_error)}` : ""}${n.logged_in ? "" : `<br>${tr("登录账号后开始上传。")}`}`, smallBtn("ph-usage-read", tr("现在读"), n.usage ? "" : "disabled")));
  const loc = { always: [true, tr("始终允许")], when_in_use: [false, tr("只在使用时")], denied: [false, tr("没允许")], not_determined: [false, tr("还没问过")] }[n.location] || [false, n.location];
  // 苹果健康：只读；苹果不告诉 app 给了哪些类型，改授权要去系统设置
  const h = n.health || {};
  const health = h.available === false ? "" : setRow(tr("苹果健康"),
    !h.asked ? tr("睡眠、步数、锻炼、心率、心情等全部类型，只读，传到你的电脑")
      : `${h.running ? tr("在读") : tr("最近读 {t}", { t: ago(h.last_run) })}${h.pending_days ? ` · ${tr("还有 {n} 天没传", { n: h.pending_days })}` : ""}${h.error ? `<br>${esc(h.error)}` : ""}<br>${tr("要改读哪些：系统设置 → 健康 → 数据访问与设备 → Beayy")}`,
    (h.asked ? smallBtn("ph-health-read", tr("现在读"), h.running ? "disabled" : "") : smallBtn("ph-health", tr("连接"))) + (h.simulator ? smallBtn("ph-health-demo", tr("造演示数据")) : ""));
  return section(tr("这台手机"),
    setRow(tr("位置"), loc[0] ? (n.precise ? tr("记停留点用") : tr("「精确位置」关着，停留点会不准")) : tr("改成「始终」才能记停留点"), `${pill(loc[0] && n.precise, loc[1])}${loc[0] && n.precise ? "" : smallBtn("ph-settings", tr("去设置"))}`)
    + setRow(tr("手表"), n.watch_recording ? `${tr("在录")}${n.watch_updated ? ` · ${tr("{t} 收到状态", { t: ago(n.watch_updated) })}` : ""}` : `${tr("没在录")}${n.watch_updated ? ` · ${tr("{t} 收到状态", { t: ago(n.watch_updated) })}` : ""}`, pill(!!n.watch_recording, n.watch_recording ? tr("在录") : tr("没在录")))
    + setRow(tr("用这台 iPhone 录"), n.phone_recording ? tr("在录，{t} 开始", { t: ago(n.phone_since) }) : tr("没戴手表时用"), `<button class="mx-switch" role="switch" id="ph-rec" aria-checked="${!!n.phone_recording}" aria-label="${tr("用这台 iPhone 录")}"></button>`)
    + health
    + setRow(tr("本人声纹"), tr("用来认出哪些话是你说的"), smallBtn("ph-voice", tr("录一段"), n.logged_in ? "" : "disabled"))
    + setRow(tr("上传"), `${n.pending ? tr("还有 {n} 段录音", { n: n.pending }) : tr("录音都传完了")}${n.pending_place_days ? tr("、{n} 天停留点", { n: n.pending_place_days }) : ""}${tr("没传 · 最近 {t}", { t: ago(n.last_upload) })}${n.last_error ? `<br>${esc(n.last_error)}` : ""}${n.logged_in ? "" : `<br>${tr("登录账号后开始上传。")}`}`, smallBtn("ph-up", tr("现在传"))));
}

function bindPhone() {
  const load = async () => {
    try { SS.native = await call("native_status"); } catch (x) { SS.native = { error: String(x) }; }
    if (S.page === "settings" && SS.tab === "phone" && !document.querySelector(".dlg-root")) renderSettings();
  };
  if (!SS.nativeAt || Date.now() - SS.nativeAt > 5e3) { SS.nativeAt = Date.now(); load(); }
  const act = (name) => call("native_action", { name }).then(() => setTimeout(() => { SS.nativeAt = 0; renderSettings(); }, 800)).catch((x) => alert(String(x)));
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  on("ph-settings", () => act("open_settings"));
  on("ph-rec", () => act(SS.native && SS.native.phone_recording ? "record_stop" : "record_start"));
  on("ph-voice", () => act("voiceprint"));
  on("ph-up", () => act("upload_now"));
  on("ph-health", () => act("health_connect"));
  on("ph-health-read", () => act("health_read"));
  on("ph-health-demo", () => act("health_demo"));
  on("ph-usage", () => act("open_usage_settings"));
  on("ph-usage-read", () => act("usage_read"));
}

/** 「数据」页：本机、同步服务、转写服务三处各存了什么、多大、留多久（方案见 报告/方案_20261021_录音与存储.md）；开着时每分钟刷新 */
function storageView(d) {
  if (d.error) return section(tr("数据"), `<div class="mx-field-error">${esc(d.error)}</div>`);
  const table = (rows, cols) => `<table class="mx-table store"><thead><tr>${cols.map((c) => `<th${c[2] ? ' class="num"' : ""}>${c[0]}</th>`).join("")}</tr></thead><tbody>${rows.map((r) => `<tr>${cols.map((c) => `<td${c[2] ? ' class="num mono"' : ""}>${c[1](r)}</td>`).join("")}</tr>`).join("")}</tbody></table>`;
  const L = d.local, C = d.cloud, T = d.transcribe;
  const local = section(tr("本机存储"), setRow(tr("数据目录"), `<span class="mono">${esc((L.path || "").replace(/^\/Users\/[^/]+/, "~"))}</span> · ${tr("共 {size}", { size: bytesText(L.total) })}`, (S.status && S.status.mobile ? "" : smallBtn("open-dir", tr("在访达中打开"))))
    + table(L.items, [[tr("内容"), (r) => esc(trMsg(r.name))], [tr("大小"), (r) => bytesText(r.bytes), 1], [tr("留多久"), (r) => esc(trMsg(r.keep))]]), { aside: tr("全部数据，以这里为准") });
  const cloud = section(tr("同步服务存储"), C.error ? emptyInline(C.error)
    : setRow(tr("地址"), `<span class="mono">${esc(hostOf(C.server))}</span> · ${tr("共 {size}", { size: bytesText(C.total) })}`, "")
      + table(C.items, [[tr("内容"), (r) => esc(trMsg(r.name))], [tr("块数"), (r) => numText(r.count), 1], [tr("大小"), (r) => bytesText(r.bytes), 1], [tr("留多久"), (r) => esc(trMsg(r.keep))]]));
  const jobs = T.jobs ? Object.entries(T.jobs).map(([k, n]) => `${{ queued: tr("排队"), running: tr("在转"), done: tr("转好待取"), failed: tr("失败") }[k] || k} ${n}`).join(tr("、")) : "";
  const asr = section(tr("转写服务存储"), T.error ? `<p class="note" style="margin:0"><span class="mono">${esc(hostOf(T.server))}</span></p>${emptyInline(T.error)}`
    : setRow(tr("地址"), `<span class="mono">${esc(hostOf(T.server))}</span> · ${jobs || tr("没有你的任务")} · ${tr("占 {size}", { size: bytesText(T.bytes) })}`, `<span class="note">${esc(trMsg(T.keep))}</span>`));
  const repo = !(d.repo || (S.settings && S.settings.repo_path)) ? "" : section(tr("人生记录仓库"), setRow(d.repo ? `<span class="mono">${esc((d.repo.path || "").replace(/^\/Users\/[^/]+/, "~"))}</span>` : tr("没填"),
    d.repo ? `raw/ ${tr("共 {size}", { size: bytesText(d.repo.bytes) })} · ${esc(trMsg(d.repo.keep))}` : tr("日数据包和日总结的文件夹"), smallBtn("repo-edit", tr("更改"))));
  return local + cloud + asr + repo + `<p class="note">${d.at ? tr("{t} 更新", { t: hm(new Date(d.at).toISOString()) }) : ""}</p>`;
}

/** 模型：官方（按账号开通，经同步服务代调，不用填密钥）或自己的网关（地址、模型、密钥；地址和模型有默认值，密钥不预填） */
function modelDialog() {
  const cf = S.settings, ks = cf.api_key_source;
  // 间隔没自己选过的跟着模型来源走：官方 60 分钟（省账号额度），自己的 10 分钟
  let mode = cf.model_mode === "official" ? "official" : "custom", every = cf.summary_minutes || 10, picked = !!cf.summary_minutes_set;
  const showEvery = (el) => el.querySelectorAll("#md-every button").forEach((x) => { const on = +x.dataset.v === every; x.classList.toggle("is-on", on); x.setAttribute("aria-selected", on); });
  const L = SS.llm || {}, mobile = !!(S.status && S.status.mobile);
  const state = L.error ? tr("要先登录 Beayy 账号。") : L.plan === "official" ? (!L.until ? tr("这个账号已开通。") : L.until < Date.now() ? tr("试用已结束，改用自己的网关。") : tr("试用到 {d}，之后改用自己的网关。", { d: mmdd(L.until) })) : L.plan === "stopped" ? tr("这个账号的官方服务已停用。") : tr("这个账号还没开通。");
  openDialog({
    title: tr("模型"), ok: tr("保存"),
    body: `<div class="mx-field"><div class="mx-seg" role="tablist" id="md-mode">${[["official", tr("官方")], ["custom", tr("自己的网关")]].map(([k, v]) => `<button role="tab" aria-selected="${k === mode}" class="${k === mode ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div></div>
      <div id="md-official" ${mode === "official" ? "" : "hidden"}><p class="note">${tr("开通后不用填地址和密钥。")}${esc(state)}</p></div>
      <div id="md-custom" class="stack" ${mode === "custom" ? "" : "hidden"}>`
      + dlgField("gw", tr("网关地址"), { value: cf.gateway, mono: true, ph: cf.default_gateway, help: tr("兼容 OpenAI 接口，到 /v1 为止") })
      + dlgField("md", tr("模型"), { value: cf.model_segment, mono: true, ph: cf.default_model || tr("比如 gpt-4.1-mini"), help: tr("10 分钟摘要和日总结都用它。") })
      + dlgField("key", tr("密钥"), { type: "password", mono: true, ph: ks === "env" ? tr("在用环境变量里的") : ks ? KEEP : "sk-…", help: "" })
      + (cf.default_gateway ? `<button class="mx-btn mx-btn--text mx-btn--sm" style="align-self:flex-start" id="md-reset">${tr("恢复默认地址和模型")}</button>` : "") + `</div>`
      + (mobile ? "" : `<div class="mx-field"><label>${tr("摘要间隔")}</label><div class="mx-seg" role="tablist" id="md-every">${[10, 20, 30, 60].map((k) => `<button role="tab" aria-selected="${k === every}" class="${k === every ? "is-on" : ""}" data-v="${k}">${tr("{n} 分钟", { n: k })}</button>`).join("")}</div><p class="note">${tr("越长越省 token，细节越少")}</p></div>`),
    onOpen: (el) => {
      const r = el.querySelector("#md-reset");
      if (r) r.onclick = () => { el.querySelector("#gw").value = cf.default_gateway; el.querySelector("#md").value = cf.default_model; };
      el.querySelector("#md-mode").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        mode = b.dataset.v;
        el.querySelectorAll("#md-mode button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
        el.querySelector("#md-official").hidden = mode !== "official";
        el.querySelector("#md-custom").hidden = mode !== "custom";
        if (!picked) { every = mode === "official" ? 60 : 10; showEvery(el); }
      };
      const ev = el.querySelector("#md-every");
      if (ev) ev.onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        every = +b.dataset.v; picked = true; showEvery(el);
      };
    },
    submit: async (el) => {
      if (picked && (every !== cf.summary_minutes || !cf.summary_minutes_set)) await saveSome({ summary_minutes: every });
      if (mode === "official") { await saveSome({ model_mode: "official" }); SS.llm = null; return renderSettings(); }
      const gw = dlgVal(el, "gw").replace(/\/+$/, ""), md = dlgVal(el, "md"), key = dlgVal(el, "key");
      if (!isUrl(gw)) throw tr("网关地址要以 https:// 开头，比如 https://api.openai.com/v1");
      if (!md) throw tr("填一个模型名。");
      await saveSome({ model_mode: "custom", gateway: gw, model_segment: md, model_daily: md });
      if (key) { try { await call("set_api_key", { key }); } catch (x) { throw tr("地址和模型存好了，密钥没存进去：{err}", { err: x }); } S.settings = await call("get_settings"); }
      renderSettings();
    },
  });
}

/** 转写服务：地址（默认官方，可换自建）、你在对话里的称呼。认账号用同步服务发的票据，不用另填口令 */
function asrDialog() {
  const cf = S.settings;
  openDialog({
    title: tr("转写服务"), ok: tr("保存"),
    body: dlgField("ta", tr("地址"), { value: cf.transcribe_server, mono: true, ph: cf.default_transcribe_server, help: tr("官方的是 {url}，也可以填自己搭的", { url: esc(cf.default_transcribe_server) }) })
      + `<button class="mx-btn mx-btn--text mx-btn--sm" style="align-self:flex-start" id="ta-reset">${tr("用官方的")}</button>`
      + dlgField("tn", tr("你在对话里叫什么"), { value: cf.speaker_name, ph: tr("我"), help: tr("转写里你说的话标成这个名字") }),
    onOpen: (el) => { el.querySelector("#ta-reset").onclick = () => { el.querySelector("#ta").value = cf.default_transcribe_server; }; },
    submit: async (el) => {
      const a = dlgVal(el, "ta").replace(/\/+$/, ""), n = dlgVal(el, "tn");
      if (a && !isUrl(a)) throw tr("地址要以 https:// 开头。");
      await saveSome({ transcribe_server: !a || a === cf.default_transcribe_server ? "" : a, speaker_name: n });
      renderSettings();
    },
  });
}

/** 高德：「Web 服务」Key（地名、附近的店、搜地点），「Web端(JS API)」Key 和安全密钥（能缩放的地图）。留空不改 */
/** 地图服务：高德（中国大陆的地点和店最全，官方或自己的 Key）或 OpenStreetMap（全球免费、不用 Key，地名和附近的店来自 OSM 社区数据） */
function amapDialog() {
  const cf = S.settings, hex = /^[0-9a-f]{32}$/i;
  let mode = cf.map_mode === "official" ? "official" : "custom";
  let prov = cf.map_provider === "osm" ? "osm" : "amap";
  openDialog({
    title: tr("地图"), ok: tr("保存"),
    body: `<div class="mx-field"><div class="mx-seg" role="tablist" id="mp-prov">${[["amap", tr("高德地图")], ["osm", "OpenStreetMap"]].map(([k, v]) => `<button role="tab" aria-selected="${k === prov}" class="${k === prov ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div></div>
      <div id="mp-osm" ${prov === "osm" ? "" : "hidden"}><p class="note" style="margin:0">${tr("全球免费，不用 Key；中国大陆的地点和店名不如高德全")}</p></div>
      <div id="mp-amap" class="stack" ${prov === "amap" ? "" : "hidden"}>
      <div class="mx-field"><div class="mx-seg" role="tablist" id="am-mode">${[["official", tr("官方")], ["custom", tr("自己的 Key")]].map(([k, v]) => `<button role="tab" aria-selected="${k === mode}" class="${k === mode ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div></div>
      <div id="am-official" ${mode === "official" ? "" : "hidden"}><p class="note">${tr("开通后不用填 Key")}</p></div>
      <div id="am-custom" class="stack" ${mode === "custom" ? "" : "hidden"}>`
      + dlgField("ak", tr("Web 服务 Key"), { type: "password", mono: true, ph: cf.has_amap_key ? KEEP : "", help: `${tr("用于地名和附近的店")} · ${howto("amap", tr("怎么获取"))}` })
      + dlgField("jk", tr("地图 Key"), { type: "password", mono: true, ph: cf.has_amap_js ? KEEP : "" })
      + dlgField("jc", tr("地图安全密钥"), { type: "password", mono: true, ph: cf.has_amap_js ? KEEP : "", help: `${tr("用于显示可拖动的地图")} · ${howto("amap", tr("怎么获取"))}` }) + `</div></div>`,
    onOpen: (el) => {
      el.querySelector("#mp-prov").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        prov = b.dataset.v;
        el.querySelectorAll("#mp-prov button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
        el.querySelector("#mp-osm").hidden = prov !== "osm";
        el.querySelector("#mp-amap").hidden = prov !== "amap";
      };
      el.querySelector("#am-mode").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        mode = b.dataset.v;
        el.querySelectorAll("#am-mode button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
        el.querySelector("#am-official").hidden = mode !== "official";
        el.querySelector("#am-custom").hidden = mode !== "custom";
      };
    },
    submit: async (el) => {
      LM.loading = null; LM.kind = null;
      if (prov === "osm") { await saveSome({ map_provider: "osm" }); return renderSettings(); }
      if (mode === "official") { await saveSome({ map_provider: "amap", map_mode: "official" }); return renderSettings(); }
      const ak = dlgVal(el, "ak"), jk = dlgVal(el, "jk"), jc = dlgVal(el, "jc");
      if (ak && !hex.test(ak)) throw tr("Web 服务 Key 是 32 位的字母数字。");
      if ((jk || jc) && !(hex.test(jk) && hex.test(jc))) throw tr("地图 Key 和安全密钥要一起填，各 32 位");
      if (!ak && !cf.has_amap_key) throw tr("填一个 Web 服务 Key，或者改用官方。");
      if (ak) await call("set_amap_key", { key: ak });
      if (jk) await call("set_amap_js", { key: jk, code: jc });
      await saveSome({ map_provider: "amap", map_mode: "custom" }); renderSettings();
    },
  });
}

/** Beayy 服务用哪台：官方（默认）或自己部署的（app/server）。只能在没登录时换。after＝存好以后接着做的（从登录框点进来的回到登录框） */
function syncServerDialog(after) {
  const cf = S.settings;
  let mode = cf.sync_server && cf.sync_server !== cf.default_sync_server ? "custom" : "official";
  openDialog({
    title: tr("Beayy 服务"), ok: tr("保存"),
    body: `<div class="mx-field"><div class="mx-seg" role="tablist" id="sm-mode">${[["official", tr("官方")], ["custom", tr("自己部署的")]].map(([k, v]) => `<button role="tab" aria-selected="${k === mode}" class="${k === mode ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div></div>
      <div id="sm-official" ${mode === "official" ? "" : "hidden"}><p class="note">${tr("记录加密后存在官方服务器，服务器读不到内容")}</p></div>
      <div id="sm-custom" class="stack" ${mode === "custom" ? "" : "hidden"}>${dlgField("ss", tr("地址"), { value: mode === "custom" ? cf.sync_server : "", mono: true, ph: "https://sync.example.com", help: tr("自己部署的服务端（app/server，Docker 一条命令）") })}</div>`,
    onOpen: (el) => {
      el.querySelector("#sm-mode").onclick = (e) => {
        const b = e.target.closest("[data-v]"); if (!b) return;
        mode = b.dataset.v;
        el.querySelectorAll("#sm-mode button").forEach((x) => { x.classList.toggle("is-on", x === b); x.setAttribute("aria-selected", x === b); });
        for (const k of ["official", "custom"]) el.querySelector("#sm-" + k).hidden = mode !== k;
      };
    },
    submit: async (el) => {
      const v = mode === "official" ? "" : dlgVal(el, "ss").replace(/\/+$/, "");
      if (mode === "custom" && !isUrl(v)) throw tr("地址要以 https:// 开头。");
      await saveSome({ sync_mode: "server", sync_server: v });
      renderSettings();
      if (typeof after === "function") setTimeout(after);
    },
  });
}

/** 模型用量：最近 30 天每天的 token（输入＋输出），按用途分。官方模型的本月额度写在设置「AI」的模型那一行 */
const USE_KINDS = { money: tr("钱"), segment: tr("10 分钟摘要"), hourly: tr("小时摘要"), daily: tr("日总结"), weekly: tr("周总结"), monthly: tr("月总结"), tag: tr("补标签"), alias: tr("项目归并"), other: tr("其他") };
async function usageDialog() {
  let u;
  try { u = await call("llm_usage"); } catch (x) { return alert(String(x)); }
  const days = u.days || [], sum = (xs) => xs.reduce((a, d) => a + d.in + d.out, 0);
  const tok = (n) => (n >= 1e4 ? tr("{n}万", { n: (n / 1e4).toFixed(n >= 1e6 ? 0 : 1), k: numText(Math.round(n / 1e3)) }) : numText(n));
  // 读数：数字和单位分开（单位跟在大数字后面）
  const ro = (k, n) => LANG === "zh" ? readout(k, n >= 1e4 ? (n / 1e4).toFixed(n >= 1e6 ? 0 : 1) : numText(n), n >= 1e4 ? "万 token" : "token")
    : readout(k, n >= 1e6 ? (n / 1e6).toFixed(1) : n >= 1e4 ? numText(Math.round(n / 1e3)) : numText(n), n >= 1e6 ? "M tokens" : n >= 1e4 ? "k tokens" : "tokens");
  const max = Math.max(1, ...days.map((d) => d.in + d.out));
  const kinds = {};
  for (const d of days) for (const [k, v] of Object.entries(d.kinds || {})) { const x = kinds[k] || (kinds[k] = { in: 0, out: 0, calls: 0 }); x.in += v.in; x.out += v.out; x.calls += v.calls; }
  const all = sum(days) || 1;
  const rows = Object.entries(kinds).sort((a, b) => b[1].in + b[1].out - (a[1].in + a[1].out));
  openDialog({
    title: tr("模型用量"), cancel: tr("关闭"), ok: null,
    body: `<div class="readouts ub-ro">${ro(tr("今天"), sum(days.slice(-1)))}${ro(tr("近 7 天"), sum(days.slice(-7)))}${ro(tr("近 30 天"), sum(days))}</div>
      <div class="ub-cols" role="img" aria-label="${tr("最近 30 天每天的用量")}">${days.map((d) => `<span title="${tr("{date}：输入 {in}，输出 {out} token", { date: esc(short(d.date)), in: numText(d.in), out: numText(d.out) })}"><i style="height:${Math.round(((d.in + d.out) / max) * 100)}%"></i></span>`).join("")}</div>
      <div class="ub-axis"><span>${days[0] ? short(days[0].date) : ""}</span><span>${tr("今天")}</span></div>
      ${rows.length ? `<table class="mx-table ub-table"><thead><tr><th>${tr("近 30 天")}</th><th class="num">${tr("次数")}</th><th class="num">${tr("输入")}</th><th class="num">${tr("输出")}</th><th class="num">${tr("占比")}</th></tr></thead><tbody>${rows.map(([k, v]) => `<tr><td>${esc(USE_KINDS[k] || k)}</td><td class="num mono">${numText(v.calls)}</td><td class="num mono">${tok(v.in)}</td><td class="num mono">${tok(v.out)}</td><td class="num mono">${Math.round(((v.in + v.out) / all) * 100)}%</td></tr>`).join("")}</tbody></table>` : `<p class="mx-empty-inline">${tr("最近 30 天还没用过模型")}</p>`}
      <p class="note">${u.official ? tr("在这台电脑上记的；官方模型的本月额度写在「模型」那一行") : tr("在这台电脑上记的，按你的网关怎么计费去算钱")}</p>`,
  });
}

/** 接到 AI 里（本机 MCP 服务）：Claude Code 一条命令，Claude 桌面版、Cursor 写进配置文件 */
function mcpDialog() {
  const exe = (S.status && S.status.exe) || "/Applications/Beayy.app/Contents/MacOS/beayy";
  const cli = `claude mcp add beayy -- "${exe}" --mcp`;
  const conf = JSON.stringify({ mcpServers: { beayy: { command: exe, args: ["--mcp"] } } }, null, 2);
  const block = (id, title, how, code) => `<div class="mx-field"><label>${title}</label><p class="note" style="margin:0">${how}</p><div class="code-box"><code class="mono" id="${id}">${esc(code)}</code><button class="mx-btn mx-btn--secondary mx-btn--sm" data-copy="${id}">${tr("复制")}</button></div></div>`;
  openDialog({
    title: tr("接到 AI 里"), cancel: tr("关闭"), ok: null,
    body: `<p class="note">${tr("Beayy 自带一个本机 MCP 服务，AI 能查每天的概况、某一天的详情、搜 10 分钟摘要、看规矩和项目。只读，数据不出这台电脑")}</p>`
      + block("mcp-cli", "Claude Code", tr("在终端里运行"), cli)
      + block("mcp-json", tr("Claude 桌面版、Cursor"), tr("写进配置文件（Claude 桌面版：设置 → 开发者 → 编辑配置；Cursor：设置 → MCP），重启后生效"), conf)
      + `<p class="note">${tr("接好后直接问：「上周哪天熬夜最多？」「Beayy 这个月推进到哪了？」")}</p>`,
    onOpen: (el) => el.querySelectorAll("[data-copy]").forEach((b) => (b.onclick = async () => {
      try { await navigator.clipboard.writeText(el.querySelector("#" + b.dataset.copy).textContent); b.textContent = tr("已复制"); } catch (x) { b.textContent = tr("手动复制"); }
    })),
  });
}

// ---------- 首次启动引导 ----------

const OB = { step: 0, err: null, msg: null, busy: false, timer: null, mode: "official" };

async function renderOnboarding() {
  document.documentElement.classList.add("onboarding");
  if (!S.settings) { S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings)); }
  const st = (S.status = await call("get_status"));
  S.sync = await call("sync_status").catch(() => S.sync || {});
  const perm = st.permissions || {};
  const steps = [tr("这是什么"), tr("授权"), tr("模型"), tr("同步")];
  // 官方模型要一个 Beayy 账号：在这一步就注册或登录（只为官方服务，同步下一步另选）
  const who = S.sync.service_email || (syncWay(S.sync) === "server" && S.sync.email);
  const dots = `<div class="ob-steps">${steps.map((x, i) => `<span class="${i === OB.step ? "is-on" : i < OB.step ? "done" : ""}">${i + 1}. ${x}</span>`).join("")}</div>`;
  const permRow = (kind, name, desc) => `<div class="mx-setting"><div class="mx-setting-main"><div class="mx-setting-name">${name}</div><div class="mx-setting-desc">${desc}</div></div>
    <span class="mx-status mx-status--${perm[kind] ? "ok" : "warn"}">${perm[kind] ? tr("已授权") : tr("未授权")}</span>${perm[kind] ? "" : `<button class="mx-btn mx-btn--secondary mx-btn--sm" data-perm="${kind}">${tr("去授权")}</button>`}</div>`;
  const msg = (OB.err ? `<div class="mx-field-error">${esc(OB.err)}</div>` : "") + (OB.msg ? `<p class="note">${esc(OB.msg)}</p>` : "");
  const body = [
    `<h1 class="ob-title">Be Authentic. Your Yesterdays.</h1>
     <p class="ob-lead">${tr("Beayy 在后台记下你在电脑和手机上的一天，写成摘要和日总结。")}</p>
     <div class="ob-list">
       <div><b>${tr("记什么")}</b><span>${tr("电脑上的应用、窗口、输入、点击和屏幕文字；手机上每个 app 的时长。密码框不记，可以排除某些 app、随时暂停或删除。")}</span></div>
       <div><b>${tr("存在哪")}</b><span>${tr("都存在这台电脑上；写摘要时，记录会发给你选的模型服务。")}</span></div>
     </div>`,
    IS_WIN ? `<h2>${tr("授权")}</h2><p class="ob-lead">${tr("Windows 上不用授权。杀毒软件拦键盘监听时，放行 Beayy。")}</p>`
    : `<h2>${tr("授权")}</h2><p class="ob-lead">${tr("macOS 要你亲自打开这三项权限。")}</p>
     ${permRow("accessibility", tr("辅助功能"), tr("读取窗口标题和屏幕上的文字"))}
     ${permRow("input_monitoring", tr("输入监控"), tr("判断你在打字还是程序在操作"))}
     ${permRow("full_disk_access", tr("完全磁盘访问（可选）"), `${tr("读取 iPhone 的屏幕使用时间")} · ${howto("screentime", tr("怎么设置"))}`)}`,
    `<h2>${tr("模型")}</h2><p class="ob-lead">${tr("摘要和日总结要用一个大模型。用官方的：账号开通后登录就能用，不用填；也可以填自己的网关（任何兼容 OpenAI 接口的服务）。不配也照常记录，时间线先只列用过的 app，以后在设置里改。")}</p>
     <div class="mx-seg" role="tablist" id="ob-mode" style="margin-bottom:var(--space-4)">${[["official", tr("用官方")], ["custom", tr("自己的网关")]].map(([k, v]) => `<button role="tab" aria-selected="${k === OB.mode}" class="${k === OB.mode ? "is-on" : ""}" data-v="${k}">${v}</button>`).join("")}</div>
     ${OB.mode === "official" ? (who ? `<p class="note">${tr("已登录 {email}", { email: esc(who) })}</p>`
       : `<p class="note">${tr("新账号每月送 5 万 token，够试用，天天用要填自己的网关")}</p><div class="btns sync-btns"><button class="mx-btn mx-btn--secondary" id="ob-signup">${tr("注册")}</button><button class="mx-btn mx-btn--text" id="ob-login">${tr("已有账号，登录")}</button></div>`) : `<div class="stack"><div class="mx-field"><label for="ob-gw">${tr("接口地址")}</label><input class="mx-input mono" id="ob-gw" value="${esc(S.draft.gateway || "https://api.openai.com/v1")}"></div>
       <div class="mx-field"><label for="ob-model">${tr("模型名")}</label><input class="mx-input mono" id="ob-model" value="${esc(S.draft.model_segment || "")}" placeholder="${tr("比如 gpt-4.1-mini")}"></div>
       <div class="mx-field"><label for="ob-key">${tr("密钥")}</label><input class="mx-input mono" id="ob-key" type="password" autocomplete="off" placeholder="${S.settings.has_api_key ? tr("已经存过一把，不改就留空") : "sk-…"}"></div>
       <div class="btns sync-btns"><button class="mx-btn mx-btn--secondary mx-btn--sm" id="ob-test" ${OB.busy ? "disabled" : ""}>${OB.busy ? tr("测试中…") : tr("存下并测试一下")}</button></div></div>`}`,
    `<h2>${tr("同步（可选）")}</h2><p class="ob-lead">${tr("有好几台设备，选一种同步方式：iCloud、网盘或 Beayy 服务；只用这一台可以跳过。")}</p>
     <div class="btns sync-btns"><button class="mx-btn mx-btn--secondary" id="ob-account">${tr("选同步方式")}</button></div>`,
  ][OB.step];
  const last = OB.step === steps.length - 1;
  $app.innerHTML = `<div class="ob">${dots}<div class="ob-body">${illus(["day", "key", "notebook", "sync"][OB.step])}${body}${msg}</div>
    <div class="ob-nav">${OB.step ? `<button class="mx-btn mx-btn--text" id="ob-back">${tr("上一步")}</button>` : "<span></span>"}
      <button class="mx-btn mx-btn--primary" id="ob-next">${last ? tr("开始使用") : OB.step === 2 && OB.mode === "custom" && !S.settings.has_api_key ? tr("先跳过") : tr("继续")}</button></div></div>`;
  const on = (id, f) => { const el = document.getElementById(id); if (el) el.onclick = f; };
  on("ob-mode", (e) => { const b = e.target.closest("[data-v]"); if (b) { OB.mode = b.dataset.v; OB.err = OB.msg = null; renderOnboarding(); } });
  $app.querySelectorAll("[data-perm]").forEach((b) => (b.onclick = () => call("open_permission", { kind: b.dataset.perm })));
  // 授权这一步：每 2 秒看一次权限，用户从系统设置回来就更新
  clearInterval(OB.timer);
  if (OB.step === 1) OB.timer = setInterval(async () => { const p = (await call("get_status")).permissions || {}; if (JSON.stringify(p) !== JSON.stringify(perm)) renderOnboarding(); }, 2000);
  const saveModel = async () => {
    const gw = document.getElementById("ob-gw").value.trim(), model = document.getElementById("ob-model").value.trim(), key = document.getElementById("ob-key").value.trim();
    S.draft = { ...S.draft, model_mode: "custom", gateway: gw, model_segment: model, model_daily: model };
    const { has_api_key, api_key_source, ...rest } = S.draft;
    await call("save_settings", { settings: rest });
    if (key) await call("set_api_key", { key });
    S.settings = await call("get_settings");
  };
  on("ob-test", async () => {
    OB.busy = true; OB.err = OB.msg = null; renderOnboarding();
    try { await saveModel(); await call("test_model"); OB.msg = tr("连上了，模型能用。"); } catch (x) { OB.err = tr("没连上：{err}", { err: x }); }
    OB.busy = false; renderOnboarding();
  });
  on("ob-back", () => { OB.step--; OB.err = OB.msg = null; renderOnboarding(); });
  const finish = async (to) => {
    clearInterval(OB.timer);
    const { has_api_key, api_key_source, ...rest } = { ...S.draft, onboarded: true };
    await call("save_settings", { settings: rest });
    S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings));
    document.documentElement.classList.remove("onboarding");
    location.hash = to; route();
  };
  on("ob-account", () => { SS.tab = "data"; finish("#/settings"); });
  on("ob-signup", () => loginDialog("signup", "", true, renderOnboarding));
  on("ob-login", () => loginDialog("login", "", true, renderOnboarding));
  on("ob-next", async () => {
    if (OB.step === 2 && OB.mode === "official") { S.draft = { ...S.draft, model_mode: "official" }; await call("save_settings", { settings: { model_mode: "official" } }); }
    else if (OB.step === 2 && (document.getElementById("ob-model").value.trim() || document.getElementById("ob-key").value.trim())) { try { await saveModel(); } catch (x) { OB.err = String(x); return renderOnboarding(); } }
    if (last) return finish("#/today");
    OB.step++; OB.err = OB.msg = null; renderOnboarding();
  });
}

// ---------- 路由 ----------

// 换页：后台命令不在主线程排队了，回来的先后不定，慢的那页晚回来不许盖掉已经换过去的页（各页写页面前看 stale）。
// 点了先把这页上次的样子摆出来（还点不了），没看过的把旧页淡下去，数据到了再换成新的
const RT = { seq: 0, snap: new Map() };
const stale = (tk) => tk !== RT.seq;

async function route() {
  // 旧地址：规矩并进了「方向」，设备列表并进了设置的「数据」
  if (/^#\/rules\/?$/.test(location.hash)) history.replaceState(null, "", "#/direction");
  if (/^#\/devices\/?$/.test(location.hash)) { SS.tab = "data"; history.replaceState(null, "", "#/settings"); }
  const tk = ++RT.seq, key = location.hash || "#/today";
  const [page = "today", arg, arg2] = location.hash.replace(/^#\/?/, "").split("/");
  // 换页就关掉弹层；刚拿到恢复码那个必须点「我已存好」才关
  if (DLG && DLG.closable) closeDialog();
  S.page = ["today", "summary", "money", "games", "direction", "devices", "settings"].includes(page) ? page : "today";
  // 设置在电脑上是盖在页面上的弹层：底下那页不动，关掉回到它
  if (S.page !== "settings") { SET.back = key; dropSettings(); }
  if (S.page === "settings" && setModal()) return routeSettings(tk);
  markNav(S.page);
  try {
    if (!S.days.length && !S.status) await boot();
    if (!S.settings) {
      S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings));
      renderSide();
      // AI 总结「跟随界面」时后台要知道界面语言（界面语言只存在这台的 localStorage）：变了就报一次
      if (TAURI && S.settings.ui_lang !== LANG) saveSome({ ui_lang: LANG }).catch(() => {});
    }
    if (!S.settings.onboarded && TAURI && !(S.status && S.status.mobile)) return renderOnboarding();
    if (S.page !== "devices") { DEV.id = null; stopEventsTimer(); }
    const snap = RT.snap.get(key);
    if (snap && $app.dataset.key !== key) { $app.innerHTML = snap; document.getElementById("main").scrollTop = 0; }
    $app.classList.add(snap ? "is-stale" : "is-wait");
    if (S.page === "summary") await renderSummary(arg, arg2);
    else if (S.page === "money") await renderMoney(arg);
    else if (S.page === "games") await renderGames(arg);
    else if (S.page === "direction") await (arg === "projects" ? renderProjects() : renderRules());
    else if (S.page === "devices") await renderDevice(decodeURIComponent(arg), arg2);
    else if (S.page === "settings") await renderSettings();
    else { TD.date = /^\d{4}-\d{2}-\d{2}$/.test(arg || "") ? arg : null; await renderToday(); }
    if (stale(tk)) return;
    document.getElementById("main").scrollTop = 0;
    RT.snap.delete(key); RT.snap.set(key, $app.innerHTML); $app.dataset.key = key;
    if (RT.snap.size > 30) RT.snap.delete(RT.snap.keys().next().value);
  } catch (e) {
    if (stale(tk)) return;
    $app.innerHTML = header(tr("出错了")) + `<p class="mx-error">${esc(e && e.message ? e.message : e)}</p>`;
  } finally {
    if (!stale(tk)) $app.classList.remove("is-stale", "is-wait");
  }
}

/** 设置（弹层）：底下还没有页（直接打开 #/settings）就先画今天 */
async function routeSettings(tk) {
  try {
    if (!S.days.length && !S.status) await boot();
    if (!S.settings) { S.settings = await call("get_settings"); S.draft = JSON.parse(JSON.stringify(S.settings)); }
    if (!S.settings.onboarded && TAURI && !(S.status && S.status.mobile)) return renderOnboarding();
    if (!$app.dataset.key) { TD.date = null; S.page = "today"; await renderToday(); $app.dataset.key = "#/today"; SET.back = "#/today"; S.page = "settings"; markNav("today"); }
    if (stale(tk)) return;
    await renderSettings();
  } catch (e) {
    if (!stale(tk)) setHost().innerHTML = `<p class="mx-error">${esc(e && e.message ? e.message : e)}</p>`;
  }
}

document.addEventListener("click", (e) => {
  for (const menu of document.querySelectorAll(".pick .mx-menu")) if (!menu.hidden && !menu.parentElement.contains(e.target)) menu.hidden = true;
  // 指到设置某一页的链接（「设置模型」「去开」这类）：先记下要开哪一页
  const t = e.target.closest("[data-set-tab]");
  if (t) { SS.tab = t.dataset.setTab; try { localStorage.setItem("settings-tab", SS.tab); } catch (x) {} }
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") for (const menu of document.querySelectorAll(".pick .mx-menu")) menu.hidden = true;
});
// 后退／前进：自己记一份走过的页面，没得退或没得进时按钮置灰（照 Claude）
const NAV = { stack: [], pos: -1, jumping: false };
function trackNav() {
  const h = location.hash || "#/today";
  if (NAV.jumping) NAV.jumping = false;
  else if (NAV.stack[NAV.pos] !== h) { NAV.stack = NAV.stack.slice(0, NAV.pos + 1).concat(h); NAV.pos = NAV.stack.length - 1; }
  document.getElementById("go-back").disabled = NAV.pos <= 0;
  document.getElementById("go-fwd").disabled = NAV.pos >= NAV.stack.length - 1;
}
const jump = (d) => { const p = NAV.pos + d; if (p < 0 || p >= NAV.stack.length) return; NAV.pos = p; NAV.jumping = true; location.hash = NAV.stack[p]; };
document.getElementById("go-back").onclick = () => jump(-1);
document.getElementById("go-fwd").onclick = () => jump(1);
window.addEventListener("hashchange", trackNav);
trackNav();
document.getElementById("side-toggle").onclick = () => {
  const root = document.documentElement, collapsed = root.dataset.side === "collapsed";
  if (collapsed) delete root.dataset.side; else root.dataset.side = "collapsed";
  const btn = document.getElementById("side-toggle"), label = collapsed ? tr("收起侧栏") : tr("展开侧栏");
  btn.setAttribute("aria-label", label); btn.title = label;
  try { collapsed ? localStorage.removeItem("side") : localStorage.setItem("side", "collapsed"); } catch (e) {}
};
if (document.documentElement.dataset.side === "collapsed") { const b = document.getElementById("side-toggle"); b.setAttribute("aria-label", tr("展开侧栏")); b.title = tr("展开侧栏"); }
window.addEventListener("hashchange", route);
// 今天页每分钟自动刷新（展开着某一段、或正在看别的页时不刷）
async function refreshToday() {
  if (S.page !== "today" || isPastDay() || document.hidden || $app.querySelector("details[open]") || document.getElementById("pl-live") || EDITOR) return;
  const y = document.getElementById("main").scrollTop;
  try { [S.days, S.status] = await Promise.all([call("list_days"), call("get_status")]); renderSide(); await renderToday(); } catch (e) { return; }
  document.getElementById("main").scrollTop = y;
}
setInterval(refreshToday, 60000);
// 手机回到前台马上同步一轮：后台那轮要等满 5 分钟，app 在后台时又停着。下到新的就刷新今天页
document.addEventListener("visibilitychange", async () => {
  if (document.hidden || !(S.status && S.status.mobile)) return;
  const r = await call("sync_now").catch(() => null);
  if (r && r.downloaded > 0) refreshToday();
});
route();
