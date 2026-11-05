import Foundation

/// 界面语言：和网页（app/ui/i18n.js）同一套写法、同一张表。代码里直接写中文原句 tr("开始录")，
/// 英文译文在 ui/locales/en.js（window.I18N_EN = {…}，以中文原句做键；iPhone 包里是 ui/locales/en.js，手表包里只带这一个文件）。
/// 系统首选语言 zh 开头用中文、不查表，否则用英文，表里没有就退回中文原句。
/// 动态部分用 {名字} 占位：tr("在这里 {t}", ["t": …])；英文要分单复数时译文写成 {"one", "other"}，按 vars["n"] 选。
enum I18n {
    /// 跟系统首选语言（这个 app 在系统设置里单独选了语言时，就是那一个）
    static let chinese: Bool = (Locale.preferredLanguages.first ?? "zh").lowercased().hasPrefix("zh")

    /// 网页视图用：同一个判断交给 i18n.js，免得网页按 app 本地化另算一遍（系统是日文时 app 本地化会退回 zh-Hans）
    static var lang: String { chinese ? "zh" : "en" }

    /// en.js 里的表：取 `window.I18N_EN = ` 后面到最后一个 `}`，是合法 JSON
    static let table: [String: Any] = {
        let url = Bundle.main.url(forResource: "en", withExtension: "js", subdirectory: "ui/locales")
            ?? Bundle.main.url(forResource: "en", withExtension: "js")
        guard let url, let s = try? String(contentsOf: url, encoding: .utf8),
              let a = s.range(of: "window.I18N_EN = "), let b = s.range(of: "}", options: .backwards), a.upperBound < b.upperBound,
              let o = try? JSONSerialization.jsonObject(with: Data(s[a.upperBound..<b.upperBound].utf8)) as? [String: Any]
        else { return [:] }
        return o
    }()
}

/// 界面上的一句话：英文查表，中文用原句；{名字} 换成 vars 里的值
func tr(_ s: String, _ vars: [String: String] = [:]) -> String {
    var r = s
    if !I18n.chinese {
        switch I18n.table[s] {
        case let t as String: r = t
        case let p as [String: Any]:
            // 英文只有 one／other：n 是 1 用 one（同 Intl.PluralRules("en")）
            let one = vars["n"].flatMap(Double.init) == 1
            r = (one ? p["one"] as? String : nil) ?? p["other"] as? String ?? s
        default: break
        }
    }
    for (k, v) in vars { r = r.replacingOccurrences(of: "{\(k)}", with: v) }
    return r
}
