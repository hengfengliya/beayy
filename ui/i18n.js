// 界面语言：设置 → 通用 → 语言（只存本机 localStorage "lang"：zh／en，没设＝跟随系统）。
// 写法照 gettext：代码里直接写中文原句 tr("今天")，英文译文在 locales/en.js，以中文原句做键；
// 中文界面不查表，英文缺译文就显示中文。查缺：python3 app/ui/tools/查译文.py
// 动态部分用 {名字} 占位：tr("{n} 分钟", { n })；英文要分单复数时译文写成 { one, other }，按 n 选。
// 换语言要重新加载页面（常量、侧栏都在加载时按当时的语言生成）。
(function () {
  let pref = null;
  try { pref = localStorage.getItem("lang"); } catch (e) {}
  // 电脑（WKWebView、WebView2）报的是系统语言（10-30 在中文 Mac 上实测 zh-CN）。
  // iPhone 外壳在页面加载前写好 window.__BEAYY_LANG（系统首选语言 zh 开头为 zh，否则 en，和外壳同一个判断，见 ios/Shared/I18n.swift）；
  // 它的网页视图自己报的是 app 本地化（zh-Hans／en），系统是两者以外的语言时会退回 zh-Hans，所以先看外壳给的
  const sys = window.__BEAYY_LANG || (navigator.languages && navigator.languages[0]) || navigator.language || "zh-CN";
  const lang = pref === "zh" || pref === "en" ? pref : /^zh/i.test(sys) ? "zh" : "en";
  document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  const dict = lang === "en" ? window.I18N_EN || {} : {};
  const plural = new Intl.PluralRules("en");
  window.LANG = lang;
  window.tr = (s, v) => {
    let r = dict[s];
    if (r && typeof r === "object") r = r[plural.select(Number(v && v.n))] || r.other;
    if (r == null) r = s;
    return v ? r.replace(/\{(\w+)\}/g, (m, k) => (k in v ? v[k] : m)) : r;
  };
  /** 页面里写死的文字（index.html 的侧栏、按钮）：文字节点和 title、aria-label 整句在表里就换掉 */
  window.trDom = (root) => {
    if (lang === "zh") return;
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let n; (n = w.nextNode()); ) { const k = n.nodeValue.trim(); if (k && dict[k] != null) n.nodeValue = n.nodeValue.replace(k, tr(k)); }
    for (const el of root.querySelectorAll("[title], [aria-label]"))
      for (const a of ["title", "aria-label"]) { const k = el.getAttribute(a); if (k && dict[k] != null) el.setAttribute(a, tr(k)); }
  };
})();
