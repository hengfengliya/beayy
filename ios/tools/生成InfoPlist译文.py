#!/usr/bin/env python3
"""权限弹窗说明的中英两份：从 Phone/、Watch/ 的 Info.plist 取 NS…UsageDescription 的中文，
到全 app 那张翻译表（app/ui/locales/en.js，以中文原句做键）查英文，生成
  <Phone|Watch>/zh-Hans.lproj/InfoPlist.strings（中文原句）
  <Phone|Watch>/en.lproj/InfoPlist.strings（英文）
表里缺哪句就列出来、报错退出，一个文件也不写。

用法：python3 app/ios/tools/生成InfoPlist译文.py [额外译文.json …]
额外译文是 {"中文": "English"}，盖在 en.js 上（还没合并进 en.js 时先用它跑）。
改了 Info.plist 的说明或译文以后重跑；生成的 .strings 勿手改。
"""
import json
import plistlib
import sys
from pathlib import Path

IOS = Path(__file__).resolve().parent.parent
EN_JS = IOS.parent / "ui" / "locales" / "en.js"
TARGETS = ["Phone", "Watch"]
HEAD = "/* 由 tools/生成InfoPlist译文.py 生成，勿手改：中文在 Info.plist，英文在 ui/locales/en.js */\n"


def load_table():
    s = EN_JS.read_text(encoding="utf-8")
    mark = "window.I18N_EN = "
    table = json.loads(s[s.index(mark) + len(mark): s.rindex("}") + 1])
    for extra in sys.argv[1:]:
        table.update(json.loads(Path(extra).read_text(encoding="utf-8")))
    return table


def quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'


def main():
    table = load_table()
    out, missing = {}, []
    for t in TARGETS:
        with open(IOS / t / "Info.plist", "rb") as f:
            plist = plistlib.load(f)
        rows = sorted((k, v) for k, v in plist.items() if k.startswith("NS") and k.endswith("UsageDescription"))
        for _, zh in rows:
            en = table.get(zh)
            if not isinstance(en, str) or not en:
                missing.append(f"{t}: {zh}")
        out[t] = {"zh-Hans": [(k, zh) for k, zh in rows], "en": [(k, table.get(zh)) for k, zh in rows]}
    if missing:
        print("en.js 里没有这些说明的译文（先补上再跑）：", file=sys.stderr)
        for m in missing:
            print("  " + m, file=sys.stderr)
        sys.exit(1)
    for t, langs in out.items():
        for lang, rows in langs.items():
            d = IOS / t / f"{lang}.lproj"
            d.mkdir(exist_ok=True)
            body = HEAD + "".join(f"{quote(k)} = {quote(v)};\n" for k, v in rows)
            (d / "InfoPlist.strings").write_text(body, encoding="utf-8")
            print(f"{t}/{lang}.lproj/InfoPlist.strings：{len(rows)} 条")


if __name__ == "__main__":
    main()
