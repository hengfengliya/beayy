#!/usr/bin/env python3
"""查英文译文：全 app 只有一张译文表 ui/locales/en.js（键是中文原句），这里把各处会显示给用户的中文都对一遍。

查哪些：
  - 网页：app.js 里 tr("…") 的原句、显示时查译文的固定词（总结的类型和领域，读 Rust 源码）、index.html 写死的字
  - Rust（src-tauri、crates、mobile、setup）：tr/tr_with/tr_msg 的原句，以及报错（Err、map_err、ok_or…）里的中文——
    报错照旧写中文原句，网页显示前查表（app.js 的 trMsg），{…} 部分当占位符对
  - 服务端（server/*.py）：返回的 "error" 里的中文（经 app 显示，同样查表）
  - iPhone、手表（ios/**/*.swift）和安卓（android/**/*.kt）：tr("…") 的原句
  - 上面这些以外的中文字面量（给模型的提示词、日志、拿去比对的数据词……）要么在表里，要么写进 ui/tools/不翻.txt 说明为什么不翻；
    两边都没有的就报出来——新加的中文漏了翻译，这里拦得住

用法：python3 app/ui/tools/查译文.py [--rest] [--baseline]
  缺译文、占位符对不上、没登记的中文 → 列出来、退出码 1（发版脚本据此拦下）
  表里有、代码里已经找不到的原句（改过文案留下的）只列出来，不算错
  --rest：再列出 app.js 里（注释以外）还带中文、又不在 tr() 里的行
  --baseline：把现在没登记的中文追加进 不翻.txt 末尾（只在第一次建表、或确认一批都不用翻时用）
"""
import ast
import json
import plistlib
import re
import sys
from pathlib import Path

UI = Path(__file__).resolve().parent.parent
APP = UI.parent
HAN = re.compile(r"[一-鿿]")
STR = r'"((?:[^"\\\n]|\\.)*)"'
TR = re.compile(r"\btr\(\s*" + STR)
PH = re.compile(r"\{[^{}]*\}")
SKIP = APP / "ui" / "tools" / "不翻.txt"


def norm(s):
    """比对用：占位符都当 {}"""
    return PH.sub("{}", s)


def load_en():
    text = (UI / "locales" / "en.js").read_text(encoding="utf-8")
    body = text[text.index("{", text.index("window.I18N_EN")): text.rindex("}") + 1]
    return json.loads(body)


def data_words():
    """界面显示时查译文的固定词：总结那边的类型（ACTIVITY_TREE）和领域（DOMAINS），以 Rust 为准"""
    rs = (APP / "crates" / "beayy-summary" / "src" / "lib.rs").read_text(encoding="utf-8")
    words = set()
    for name in ("ACTIVITY_TREE", "DOMAINS"):
        m = re.search(r"pub const " + name + r"\b.*?;\n", rs, re.S)
        words |= set(re.findall(r'"([^"]+)"', m.group(0)))
    return words


def code_lines(text):
    """去掉注释后的每一行（行号从 1 起）：// 到行尾、/* … */ 跨行；字符串里的 // 不当注释"""
    out, in_block = [], False
    for no, line in enumerate(text.split("\n"), 1):
        buf, i, q = [], 0, None
        while i < len(line):
            c = line[i]
            if in_block:
                if line.startswith("*/", i): in_block = False; i += 2
                else: i += 1
                continue
            if q:
                buf.append(c)
                if c == "\\" and i + 1 < len(line): buf.append(line[i + 1]); i += 2; continue
                if c == q: q = None
                i += 1; continue
            if c in "\"'`": q = c; buf.append(c); i += 1; continue
            if line.startswith("//", i) and not (i and line[i - 1] == ":"): break
            if line.startswith("/*", i): in_block = True; i += 2; continue
            buf.append(c); i += 1
        if q == "`": q = None  # 跨行模板字符串：下一行接着当代码看
        out.append((no, "".join(buf)))
    return out


# ---------- Rust、Swift、Kotlin：取出字符串字面量（跳过注释、Rust 的字符和生命周期、测试模块） ----------

ESC = {"n": "\n", "t": "\t", "r": "\r", '"': '"', "\\": "\\", "'": "'", "0": "\0"}


def c_literals(text, lang):
    """[(行号, 字面量的值, 前文)]；Swift 的 \\(…) 插值当成 {}"""
    if lang == "rust":
        cut = re.search(r"^#\[cfg\(test\)\]", text, re.M)
        if cut: text = text[:cut.start()]
    out, i, n, line = [], 0, len(text), 1
    while i < n:
        c = text[i]
        if c == "\n": line += 1; i += 1; continue
        if text.startswith("//", i):
            j = text.find("\n", i); i = n if j < 0 else j; continue
        if text.startswith("/*", i):
            j = text.find("*/", i + 2); seg = text[i: n if j < 0 else j + 2]; line += seg.count("\n"); i += len(seg); continue
        if lang == "rust" and c == "'":
            m = re.match(r"'(\\.|\\u\{[0-9a-fA-F]+\}|[^\\'])'", text[i:])
            i += len(m.group(0)) if m else 1; continue
        raw = re.match(r'r(#*)"', text[i:]) if lang == "rust" and c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")) else None
        if raw:
            close = '"' + raw.group(1); start = i + len(raw.group(0)); j = text.find(close, start)
            val = text[start:j]; out.append((line, val, text[max(0, i - 240):i])); line += text[i:j].count("\n"); i = j + len(close); continue
        if c == '"':
            if lang == "swift" and text.startswith('"""', i):
                j = text.find('"""', i + 3); body = text[i + 3:j]
                # Swift 的多行字面量：去掉首尾换行，按结尾 """ 的缩进去掉每行开头
                ind = body[body.rfind("\n") + 1:] if body.rstrip(" ").endswith("\n") else ""
                val = "\n".join(l[len(ind):] if l.startswith(ind) else l for l in body.strip("\n").rstrip(" ").rstrip("\n").split("\n"))
                out.append((line, val, text[max(0, i - 240):i])); line += body.count("\n"); i = j + 3; continue
            j, buf = i + 1, []
            while j < n and text[j] != '"':
                if text[j] == "\\" and j + 1 < n:
                    nx = text[j + 1]
                    if lang == "swift" and nx == "(":
                        depth, k = 1, j + 2
                        while k < n and depth: depth += {"(": 1, ")": -1}.get(text[k], 0); k += 1
                        buf.append("{}"); j = k; continue
                    if lang == "rust" and nx == "\n":
                        j += 2
                        while j < n and text[j] in " \t\n": j += 1
                        continue
                    buf.append(ESC.get(nx, "\\" + nx)); j += 2; continue
                if lang == "kotlin" and text[j] == "$":
                    m = re.match(r"\$(\{[^}]*\}|[A-Za-z_]\w*)", text[j:])
                    if m: buf.append("{}"); j += len(m.group(0)); continue
                buf.append(text[j]); j += 1
            val = "".join(buf); out.append((line, val, text[max(0, i - 240):i])); line += text[i:j].count("\n"); i = j + 1; continue
        i += 1
    return out


TR_CALL = re.compile(r"\b(tr|tr_with|tr_msg|trMsg)\(\s*(&?)\s*$")
# 报错：字面量所在这句里有这些（往前看到上一个 ; { } 为止）
ERR_CTX = re.compile(r"\bErr\(|map_err|ok_or|\bbail!|\berr\(|Error::|\"error\"|=> *\"|unwrap_or_else\(\|_?e?\| *format!")


def stmt(before):
    k = max(before.rfind(";"), before.rfind("{\n"), before.rfind("}\n"))
    return before[k + 1:]


def rust_strings():
    """Rust 里带中文的字面量：(文件:行, 值, 是不是必须有译文)"""
    roots = [APP / "src-tauri" / "src", APP / "mobile" / "src", APP / "setup" / "src"] + sorted((APP / "crates").glob("*/src"))
    for root in roots:
        for p in sorted(root.rglob("*.rs")):
            for no, val, before in c_literals(p.read_text(encoding="utf-8"), "rust"):
                if not HAN.search(val): continue
                st = stmt(before)
                must = bool(TR_CALL.search(before)) or bool(ERR_CTX.search(st) and not re.search(r"\b(log|eprintln!|println!|log_line|write)\(", st[-120:]))
                yield f"{p.relative_to(APP)}:{no}", val, must


def shell_strings():
    """iPhone、手表、安卓外壳：tr("…") 的必须有译文，其余带中文的要登记"""
    for lang, pattern in (("swift", "ios/**/*.swift"), ("kotlin", "android/**/*.kt")):
        for p in sorted(APP.glob(pattern)):
            if "/build/" in str(p) or "Tests" in p.parts[-2]: continue
            for no, val, before in c_literals(p.read_text(encoding="utf-8"), lang):
                if HAN.search(val):
                    yield f"{p.relative_to(APP)}:{no}", val, bool(TR_CALL.search(before))


def py_strings():
    """服务端：返回的 error 里的中文必须有译文；f-string 的 {…} 当占位符"""
    for p in sorted((APP / "server").glob("*.py")):
        tree = ast.parse(p.read_text(encoding="utf-8"))
        docs = {id(n.body[0].value) for n in ast.walk(tree) if isinstance(n, (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)) and n.body and isinstance(n.body[0], ast.Expr)}
        shown = set()
        for n in ast.walk(tree):
            if isinstance(n, ast.Dict):
                for k, v in zip(n.keys, n.values):
                    if isinstance(k, ast.Constant) and k.value == "error": shown.add(id(v))
        for n in ast.walk(tree):
            if isinstance(n, ast.JoinedStr):
                val = "".join(v.value if isinstance(v, ast.Constant) else "{}" for v in n.values)
                for v in n.values: docs.add(id(v))
            elif isinstance(n, ast.Constant) and isinstance(n.value, str) and id(n) not in docs:
                val = n.value
            else:
                continue
            if HAN.search(val):
                yield f"{p.relative_to(APP)}:{n.lineno}", val, id(n) in shown


def plist_strings():
    """iPhone、手表的权限说明（Info.plist 的 NS…UsageDescription）：英文由 ios/tools/生成InfoPlist译文.py 查表生成，必须有译文"""
    for p in sorted(APP.glob("ios/*/Info.plist")):
        for k, v in plistlib.loads(p.read_bytes()).items():
            if k.endswith("UsageDescription") and isinstance(v, str) and HAN.search(v):
                yield f"{p.relative_to(APP)}:{k}", v, True


def load_skip():
    """不翻.txt：一行一句（占位符写 {}、换行写 \\n），# 开头是分组说明，句子本身以 # 开头的写成 \\#"""
    if not SKIP.exists(): return set()
    lines = [l for l in SKIP.read_text(encoding="utf-8").split("\n") if l.strip() and not l.startswith("#")]
    return {norm((l[1:] if l.startswith("\\#") else l).replace("\\n", "\n")) for l in lines}


def main():
    en = load_en()
    en_norm = {norm(k) for k in en}
    skip = load_skip()
    js = (UI / "app.js").read_text(encoding="utf-8")
    keys = {json.loads('"' + m + '"') for m in TR.findall(js)} | data_words()
    html_keys = set()
    for no, line in code_lines((UI / "index.html").read_text(encoding="utf-8")):
        for m in re.findall(r'(?:title|aria-label)="([^"]*)"|>([^<>]+)<', line):
            k = (m[0] or m[1]).strip()
            if HAN.search(k): html_keys.add(k)
    bad = 0
    for k in sorted(keys | html_keys):
        if k not in en: print(f"缺译文：{k}"); bad += 1
    for k, v in en.items():
        want = set(re.findall(r"\{(\w+)\}", k))
        for form in (v.values() if isinstance(v, dict) else [v]):
            got = set(re.findall(r"\{(\w+)\}", form))
            if got - want: print(f"注意，译文用了原句没有的占位符（调用处要另传）：{k} → {form}")
            if form.count("{}") != k.count("{}"): print(f"占位符个数对不上：{k} → {form}"); bad += 1

    # 其他各处
    seen_other = set()
    unlisted = []
    for where, val, must in list(rust_strings()) + list(py_strings()) + list(shell_strings()) + list(plist_strings()):
        n = norm(val)
        seen_other.add(n)
        if n in en_norm: continue
        if must:
            print(f"缺译文：{val!r}（{where}）"); bad += 1
        elif n not in skip:
            unlisted.append((where, val))
    for where, val in unlisted:
        print(f"没登记（要么翻、要么写进 不翻.txt）：{val[:80]!r}（{where}）")
    if "--baseline" in sys.argv and unlisted:
        with SKIP.open("a", encoding="utf-8") as f:
            f.write("\n# ---- 基线 ----\n")
            for v in sorted({norm(v).replace("\n", "\\n") for _, v in unlisted}):
                f.write(("\\" + v if v.startswith("#") else v) + "\n")
        print(f"已把 {len(unlisted)} 处写进 不翻.txt")
        unlisted = []
    bad += len(unlisted)

    # 表里的句子在哪段代码字面量里出现过（整句或作为一部分，比如 Markdown 模板里的标题）就算还在用；只有占位符和标点的（拼接用的模板）也算
    quoted = set(re.findall(STR, js))
    blob = "\n".join(seen_other | {norm(json.loads('"' + q + '"')) for q in quoted if HAN.search(q)})
    # 代码里带 {} 的句子拼出来的整句（比如服务端 f"本月官方{…}额度用完了" 的三种）也算在用
    tpl = [re.compile("^" + ".+".join(map(re.escape, t.split("{}"))) + "$") for t in seen_other if "{}" in t and HAN.search(t)]
    stale = [k for k in en if k not in keys | html_keys and HAN.search(norm(k)) and re.sub(r"^(\{\})+|(\{\})+$", "", norm(k)) not in blob and not any(r.match(k) for r in tpl)]
    for k in stale: print(f"代码里找不到（可删）：{k}")
    if "--rest" in sys.argv:
        for no, line in code_lines(js):
            rest = TR.sub("", line)
            if HAN.search(rest): print(f"app.js:{no}: {line.strip()[:160]}")
    print(f"网页 tr() 原句和固定词 {len(keys)} 条＋页面 {len(html_keys)} 条，后台和外壳 {len(seen_other)} 句，译文表 {len(en)} 条，不翻 {len(skip)} 句，问题 {bad} 处")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
