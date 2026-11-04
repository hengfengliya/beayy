# 第三方代码声明

本 app 引用或改编了以下开源代码。

## OpenChronicle（Einsia/OpenChronicle）

- 用到：`resources/mac-ax-watcher.swift` 的辅助功能读取函数和键鼠监听结构；`resources/mac-ax-helper.swift` 的遍历过滤思路（丢弃的视觉角色表），先改编进 Swift 版（`AXUtil.swift`、`InteractionTap.swift`、`UIText.swift`），10-16 随 Swift 版一起移植成 Rust：`crates/beayy-capture/src/macos/ax.rs`、`capture.rs`、`uitext.rs`。
- 许可证：MIT，原文如下。

```
MIT License

Copyright (c) 2026 Einsia

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Personal-Bill（zhu123279/Personal-Bill）

- 用到：收支导入（`src-tauri/src/bills.rs`）参考了它认微信、支付宝账单表头和判断重复、退款的思路，没有拷代码。
- 许可证：MIT。

## Lucide（lucide-icons/lucide）

- 用到：网页界面（`ui/index.html`、`ui/app.js`）里的图标路径：panel-left、arrow-left、arrow-right、clock、file-text、list-checks、monitor-smartphone、chevron-down、settings、pause、play、folder、info、log-out、monitor、smartphone。
- 许可证：ISC，原文如下。

```
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```
