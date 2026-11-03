// 安装器页面：欢迎 → 安装（进度）→ 完成；出错停在「没装上」可以重试。
// 不在 Tauri 里（浏览器直接打开看样子）时走演示：假数据、假进度；?screen=installing|done|failed 直接看某一屏
const T = window.__TAURI__;
const call = (cmd, args) => T ? T.core.invoke(cmd, args) : demo(cmd, args);
const $ = (id) => document.getElementById(id);

const EN = {
  "安装": "Install", "安装到": "Install to", "更改": "Change", "正在安装": "Installing", "安装完成": "Installed",
  "桌面快捷方式": "Desktop shortcut", "开机自动启动": "Open at login", "登录电脑后自动开始记录": "Start recording when you log in",
  "完成": "Done", "打开 Beayy": "Open Beayy", "没装上": "Couldn't install", "重试": "Try again", "最小化": "Minimize", "关闭": "Close",
};
const en = !/^zh/i.test(navigator.language || "zh");
const tr = (s) => (en && EN[s]) || s;
if (en) document.documentElement.lang = "en";
document.querySelectorAll("[data-t]").forEach((el) => { el.textContent = tr(el.dataset.t); });
document.querySelectorAll("[data-t-aria]").forEach((el) => { el.setAttribute("aria-label", tr(el.dataset.tAria)); el.title = tr(el.dataset.tAria); });

let dir = "", exe = "", busy = false;

// 插画每次进屏重画一遍（线条有描出来的动画）
function show(id) {
  document.querySelectorAll(".screen").forEach((s) => { s.hidden = s.id !== id; });
  const art = $(id).querySelector("[data-illus]");
  if (art) {
    const svg = (window.MX_ILLUS || {})[art.dataset.illus] || "";
    art.innerHTML = art.hasAttribute("data-sm") ? svg.replace('class="mx-illus"', 'class="mx-illus mx-illus--sm"') : svg;
  }
  $("ver").hidden = id !== "welcome";
  const focus = { welcome: "go", done: "open", failed: "retry" }[id];
  if (focus) $(focus).focus();
}

// 进度条：后端报的进度可能一下就到 1，按最少 1.6 秒走完，看得见在装。
// 用定时器不用 requestAnimationFrame：窗口最小化时动画帧停了，进度会卡住、到不了完成页
let shown = 0, target = 0, started = 0, timer = 0;
function tick() {
  const minimum = Math.min(1, (performance.now() - started) / 1600);
  shown += (Math.min(target, minimum) - shown) * 0.25;
  if (Math.min(target, minimum) - shown < 0.003) shown = Math.min(target, minimum);
  $("fill").style.width = (shown * 100).toFixed(1) + "%";
  $("pct").textContent = Math.round(shown * 100) + "%";
}

async function install() {
  busy = true; $("close").disabled = true;
  shown = 0; target = 0; started = performance.now();
  show("installing"); tick(); clearInterval(timer); timer = setInterval(tick, 30);
  try {
    exe = await call("install", { dir });
    target = 1;
    await new Promise((r) => { const wait = () => (shown >= 1 ? r() : setTimeout(wait, 50)); wait(); });
    await new Promise((r) => setTimeout(r, 250));
    show("done");
  } catch (e) {
    $("err").textContent = String(e);
    show("failed");
  } finally {
    clearInterval(timer);
    busy = false; $("close").disabled = false;
  }
}

async function finish(open) {
  $("finish").disabled = $("open").disabled = true;
  const on = (id) => $(id).getAttribute("aria-checked") === "true";
  try {
    await call("finish", { exe, desktop: on("desktop"), autostart: on("autostart"), open });
  } catch (e) {
    $("err").textContent = String(e);
    show("failed");
    $("finish").disabled = $("open").disabled = false;
  }
}

$("go").onclick = install;
$("retry").onclick = install;
$("change").onclick = async () => { const d = await call("pick_dir"); if (d) { dir = d; $("dir").textContent = d; $("dir").title = d; } };
$("finish").onclick = () => finish(false);
$("open").onclick = () => finish(true);
document.querySelectorAll(".mx-switch").forEach((b) => { b.onclick = () => b.setAttribute("aria-checked", String(b.getAttribute("aria-checked") !== "true")); });
$("min").onclick = () => T && T.window.getCurrentWindow().minimize();
$("close").onclick = () => { if (!busy) T ? T.window.getCurrentWindow().close() : null; };
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !busy && T) T.window.getCurrentWindow().close(); });
document.addEventListener("contextmenu", (e) => e.preventDefault());

if (T) T.event.listen("progress", (e) => { target = Math.max(target, Number(e.payload) || 0); });

(async () => {
  const i = await call("info");
  dir = i.dir; $("dir").textContent = dir; $("dir").title = dir;
  $("ver").textContent = i.version;
  const s = new URLSearchParams(location.search).get("screen");
  if (s === "installing") { show("installing"); target = 0.62; shown = 0.62; started = -1e6; tick(); }
  else if (s === "failed") { $("err").textContent = tr("Beayy 还在运行，退出后再装"); show("failed"); }
  else show(s === "done" ? "done" : "welcome");
})();

// 浏览器里看样子用
function demo(cmd) {
  if (cmd === "info") return Promise.resolve({ version: "0.2.44", dir: "C:\\Users\\you\\AppData\\Local\\Programs\\Beayy", demo: true });
  if (cmd === "install") return new Promise((r) => { let p = 0; const t = setInterval(() => { p += 0.1; target = Math.min(1, p); if (p >= 1) { clearInterval(t); r(dir + "\\beayy.exe"); } }, 120); });
  return Promise.resolve(null);
}
