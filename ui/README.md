# Beayy 网页界面（app/ui）

Tauri 2 外壳加载的网页：今天、总结、收支、规矩、设备五页，加上从左下角账号菜单进的设置。纯 HTML＋CSS＋原生 JS，不用打包工具，入口 `index.html`。iPhone 版用同一套（宽不到 760 时侧栏变底部页签）。

## 文件

| 文件 | 内容 |
|---|---|
| `index.html` | 壳：侧栏（四个入口，左下角账号菜单：设置、暂停记录、设备与同步、打开数据目录、退出；记录状态写在名字后面）＋正文 |
| `app.js` | 取数、路由（`#/today[/YYYY-MM-DD]`、`#/summary[/week｜month]/YYYY-MM-DD`、`#/money[/YYYY-MM｜YYYY]`（概览、明细、分类、对方、导入五个页签在页面内存里切）、`#/rules`、`#/devices`、`#/settings`）、各页 |
| `style.css` | 壳的布局、作息图／一天的形状／读数（设计系统「人生记录」；读数数字 10-29 起用苹方加等宽数字，不用等宽字体，取自 `网站/static/`，变量加 `rec-` 前缀）、页面专用的几样 |
| `vendor/moxian/` | 设计系统 Beayy（原名墨线）的 `tokens.css`（由 tokens.json 生成）和 `bundle.css`（原样拷贝），不要手改 |
| `tools/同步墨线.py` | 设计系统改了就跑一次：`python3 app/ui/tools/同步墨线.py` |
| `i18n.js` | 界面语言和翻译函数 `tr()`，见下面「语言」 |
| `locales/en.js` | 英文译文：中文原句 → 英文 |
| `tools/查译文.py` | 查缺译文：`python3 app/ui/tools/查译文.py`（`--rest` 列出没包 `tr()` 的中文，逐条看是不是故意留的）；`build.sh` 每次编译都跑一遍，缺了只提醒 |
| `mock/` | 样例数据（全是虚构的）：`<命令名>.json` 和合成的 `mock.js`，由 `mock/make_mock.py` 生成 |

## 语言（2026-10-30 起有英文）

- 设置 → 通用 → 语言：跟随系统（默认）／简体中文／English，只存本机（localStorage `lang`），换了重新加载页面。系统语言是中文就用中文，别的都用英文。iPhone 的外壳还没做英文，在手机上「跟随系统」先固定中文。
- 写法照 gettext：代码里直接写中文原句 `tr("今天")`，`locales/en.js` 以中文原句做键。改了中文文案，英文那条就对不上了，跑 `查译文.py` 会报缺。
- 动态部分用占位符：`tr("{n} 分钟", { n })`；英文分单复数的，计数变量叫 `n`，译文写成 `{ "one": …, "other": … }`。`tr()` 不转义，原来套 `esc()` 的照样套。
- 不包 `tr()` 的：拿去比对、存储、发给 Rust 的中文值（只在显示处包），总结正文、地点名、项目名这类数据，开发日志。总结的类型和领域是固定词（`crates/beayy-summary` 的 `ACTIVITY_TREE`、`DOMAINS`），在显示处查译文，`查译文.py` 直接读 Rust 源码核对。
- 英文比中文宽：按两个汉字定宽的地方用 `:lang(en)` 单独放宽（一天的形状的行名、地点时间线的时刻列）。
- 还是中文的（第 2 步做）：模型写的总结和摘要、Rust 和服务端发来的文字（总结页脚、存储表、报错），规矩的识别也只认中文。

## 设计系统怎么引用

Tauri 只能读 `app/ui/` 里面的文件，所以把 `设计系统/墨线/project/` 拷一份进 `vendor/moxian/`，不在页面里用相对路径跳出去。主题：默认跟随系统，设置页可固定成浅色或深色（`<html data-theme>`，记在 localStorage）。

## 取数

- 在 Tauri 里：`window.__TAURI__.core.invoke(命令, 参数)`，命令和返回格式见 `app/src-tauri/COMMANDS.md`。
- 浏览器里：读 `mock/<命令名>.json`；双击用 `file://` 打开时浏览器不让读本地文件，改读 `mock/mock.js`。写操作（保存规矩、设置、密钥、暂停）只改内存。
- `get_day.json` 一个文件装最近 14 天，按日期做键。
- 重新生成样例：`python3 app/ui/mock/make_mock.py`。

## 和 COMMANDS.md 对不上、需要 Rust 那边补的

- `get_day.segments[]` 多带一个 `recording`（「经过」，Markdown），页面点开时段要显示；没有时退回显示 `memory`。
- `get_day.segments[].kind` 除了 `10min` 还会有 `6h`（Codex 的 6 小时摘要），页面标「6 小时」。
- `get_day.phone.night_sec` 当成凌晨 0–5 点的使用秒数（同日数据包的 `dawn_sec`）；熬夜 = 电脑 `late_sec`＋手机 `late_sec`＋`night_sec`。
- `list_days[].hc`／`hp` 接受两种：`{"09": 秒}`（COMMANDS.md 的写法）或 24 个分钟数的数组（网站索引的写法）。
- 手机读不到时 `get_day.phone` 给 `null`；页面看 `get_status.permissions.full_disk_access`，没开就写「未授权完全磁盘访问」，不写 0。

## Tauri 窗口

`<html>` 在 Tauri 里会加 `is-tauri`：侧栏顶部左边让出 72 像素给系统窗口按钮，侧栏顶部和页头带 `data-tauri-drag-region` 可拖窗口。外壳那边窗口用 `titleBarStyle: "Overlay"`、隐藏标题。

## 自测

```bash
python3 -m http.server 8767 --directory "app/ui"
```

打开 http://localhost:8767 ，或直接双击 `index.html`。

## 界面检查（impeccable）

用 [impeccable](https://github.com/pbakaus/impeccable) 的 `audit` 查：无障碍、性能、主题、适配、实现一致性五项各 0–4 分。2026-10-30 修完一轮后检测器 0 条（源码和渲染后 7 个页面）。

```bash
~/.claude/skills/impeccable/scripts/impeccable detect --json app/ui/index.html app/ui/app.js app/ui/style.css
~/.claude/skills/impeccable/scripts/impeccable detect --json "http://localhost:8767/#/today"
```

守住的几条：

- 文字对比度 ≥ 4.5:1：设计系统的 `ink-4` 浅色是 `#75746e`，别再用更浅的灰写字；弱信息靠字号区分。
- 地点淡色（`--pl-N`）只作带文字的底，细条和色块边用同色相实色 `--pl-N-s`（≥ 3:1）。
- 手机断点是 `max-width: 759px`（style.css 和 app.js 的 `NARROW` 两处），电脑窗口最窄 760，别让两者碰上。
- 字体只有两组：界面 `--font-sans`、数字和钟点 `--font-mono`（都来自设计系统令牌）。不再用 `--font-serif`。用户在设置 → 通用 → 字体换的字体存在本机 localStorage（`font-sans`、`font-mono`，`{family, name}`），index.html 里样式后面那段 `FONTS` 脚本把它排在默认字体前面；系统字体列表来自 `list_fonts` 命令（手机核心没有，手机上不放这一行）。
- 页面标题 `h1`，区块标题 `h2`（`section()`、总结的段名），区块里的小标题 `h3`。
- 能点的东西用 `<button>`；一天的形状这种一排格子用 `role="toolbar"`＋方向键（`bindStripKeys`），整条只占一个 Tab。
