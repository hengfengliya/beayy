import CoreLocation
import Foundation

/// 看的是哪一天：今天、地图、对话三个页签共用，翻到哪天三个页签都停在哪天
final class DayState: ObservableObject {
    static let shared = DayState()
    @Published var day = Calendar.current.startOfDay(for: .now)

    var isToday: Bool { Calendar.current.isDateInToday(day) }
    /// 停留点在本机只留 30 天，再往前没有可看的
    var canGoBack: Bool { day > Calendar.current.date(byAdding: .day, value: -30, to: Calendar.current.startOfDay(for: .now))! }

    func shift(_ n: Int) {
        guard let d = Calendar.current.date(byAdding: .day, value: n, to: day), d <= Date() else { return }
        day = Calendar.current.startOfDay(for: d)
    }

    func backToToday() { day = Calendar.current.startOfDay(for: .now) }

    /// 标题：今天、昨天、10-16 周五
    var title: String {
        let cal = Calendar.current
        if cal.isDateInToday(day) { return tr("今天") }
        if cal.isDateInYesterday(day) { return tr("昨天") }
        return DayState.label(day)
    }

    static func label(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: I18n.chinese ? "zh_CN" : "en_US")
        f.dateFormat = "MM-dd EEE"
        return f.string(from: d)
    }

    static func key(_ d: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: d)
    }
}

/// 一句话：谁、几点到几点、说了什么（同一个人 20 秒内接着说的并成一句）
struct Line: Identifiable, Hashable {
    let id = UUID()
    var start: Date
    var end: Date
    var speaker: String
    var text: String
    /// 本人：服务器按声纹认出的名字（不是「他人N」「未辨」）
    var isSelf: Bool { Line.isSelf(speaker) }
    static func isSelf(_ name: String) -> Bool { !name.hasPrefix("他人") && name != "未辨" }
    /// 显示的说话人：本人写「我」，别人照服务器给的（他人1、未辨）
    static func who(_ name: String) -> String { isSelf(name) ? tr("我") : name }
}

/// 一段对话：两句之间空 3 分钟以上算另一段（同电脑上的手表页签）
struct Conversation: Identifiable, Hashable {
    var id: Date { start }
    var start: Date
    var end: Date
    var lines: [Line]
    /// 每个人说了多少秒，本人排第一、其余按时长
    var speakers: [(String, TimeInterval)] {
        var m: [String: TimeInterval] = [:]
        for l in lines { m[l.speaker, default: 0] += max(1, l.end.timeIntervalSince(l.start)) }
        return m.sorted { a, b in Line.isSelf(a.key) != Line.isSelf(b.key) ? Line.isSelf(a.key) : a.value > b.value }
    }
    var talk: TimeInterval { speakers.reduce(0) { $0 + $1.1 } }
    static func == (a: Conversation, b: Conversation) -> Bool { a.start == b.start && a.lines.count == b.lines.count }
    func hash(into h: inout Hasher) { h.combine(start) }
}

/// 某天的对话：同步服务上电脑转写好传上来的 transcript.<段 id>（见 TranscriptStore），分成一段段。
/// 以前的日子读一次就记在内存里，今天每次打开都重读（新转写好的段）。
/// 模拟器不连服务器（见 Account.send），改读 Application Support/fixtures/transcripts-<日>.jsonl（测试用，假的）
final class Talk: ObservableObject {
    static let shared = Talk()
    @Published private(set) var days: [String: [Conversation]] = [:]
    @Published private(set) var errors: [String: String] = [:]
    @Published private(set) var loading: Set<String> = []

    func conversations(_ day: Date) -> [Conversation] { days[DayState.key(day)] ?? [] }

    /// 退出登录：忘掉读过的
    func reset() {
        days = [:]
        errors = [:]
    }

    @MainActor
    func load(_ day: Date, force: Bool = false) async {
        let key = DayState.key(day)
        if !force, days[key] != nil, !Calendar.current.isDateInToday(day) { return }
        loading.insert(key)
        defer { loading.remove(key) }
        do {
            let (text, warning) = try await Self.fetch(day)
            days[key] = Self.parse(text, day: day)
            errors[key] = warning
        } catch {
            errors[key] = error.localizedDescription
            if days[key] == nil { days[key] = [] }
        }
    }

    private static func fetch(_ day: Date) async throws -> (String, String?) {
        #if targetEnvironment(simulator)
        let f = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("fixtures/transcripts-\(DayState.key(day)).jsonl")
        return ((try? String(contentsOf: f, encoding: .utf8)) ?? "", nil)
        #else
        guard Account.shared.loggedIn else { return ("", nil) }  // 页面上写「登录账号后…」，不算出错
        return try await TranscriptStore.shared.text(day: day)
        #endif
    }

    /// transcripts 格式的 JSONL → 一段段对话（只看 type＝utt 的行；给了 day 就只要这一天开始的句子）
    static func parse(_ text: String, day: Date? = nil) -> [Conversation] {
        let f = ISO8601DateFormatter()
        let d0 = day.map { Calendar.current.startOfDay(for: $0) }
        var utts: [Line] = []
        for raw in text.split(separator: "\n") {
            guard let o = try? JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any], o["type"] as? String == "utt",
                  let s = (o["start"] as? String).flatMap(f.date(from:)) else { continue }
            if let d0, s < d0 || s >= Calendar.current.date(byAdding: .day, value: 1, to: d0)! { continue }
            let e = (o["end"] as? String).flatMap(f.date(from:)) ?? s
            let t = (o["text"] as? String ?? "").trimmingCharacters(in: .whitespaces)
            guard !t.isEmpty else { continue }
            utts.append(Line(start: s, end: max(e, s), speaker: o["speaker"] as? String ?? "未辨", text: t))
        }
        utts.sort { $0.start < $1.start }
        var out: [Conversation] = []
        for u in utts {
            if var c = out.last, u.start.timeIntervalSince(c.end) <= 180 {
                if var l = c.lines.last, l.speaker == u.speaker, u.start.timeIntervalSince(l.end) <= 20 {
                    l.text += u.text
                    l.end = max(l.end, u.end)
                    c.lines[c.lines.count - 1] = l
                } else {
                    c.lines.append(u)
                }
                c.end = max(c.end, u.end)
                out[out.count - 1] = c
            } else {
                out.append(Conversation(start: u.start, end: u.end, lines: [u]))
            }
        }
        return out
    }
}

/// 一天里的一段停留：起止已经截到这一天之内，没走的到下一段开始（今天的到现在）
struct Stay: Identifiable {
    var visit: Visit
    var start: Date
    var end: Date
    var isNow: Bool
    var id: String { visit.id }
    var name: String? { visit.display }
    var short: String? { visit.short }
    var length: TimeInterval { end.timeIntervalSince(start) }
    /// 前一天就到了（时间线上写「昨天」）
    var sinceYesterday: Bool { (visit.arrive ?? start) < start }
    /// 起止：09:20–现在；前一天到的写「昨天 20:45–08:35」
    var span: String {
        let a = sinceYesterday ? tr("昨天") + " " + hm(visit.arrive!) : hm(start)
        return "\(a)–\(isNow ? tr("现在") : hm(end))"
    }
    /// 时间线左边那一栏
    var startText: String { sinceYesterday ? tr("昨天") : hm(start) }
}

/// 说话多久：不到一分钟写秒，再长同 spanText
func talkText(_ sec: TimeInterval) -> String {
    sec < 60 ? tr("{n} 秒", ["n": "\(max(1, Int(sec.rounded())))"]) : spanText(sec)
}

extension Conversation {
    /// 起止：同一分钟里的只写一个时刻
    var span: String { hm(start) == hm(end) ? hm(start) : "\(hm(start))–\(hm(end))" }
    var length: TimeInterval { end.timeIntervalSince(start) }
}

extension Stay {
    /// 某天的停留：iPhone 和手表记到同一段时只留一份（系统到访 > 显著位置变化 > 手表，同桌面版），
    /// 还没走的只认最后一段（系统偶尔漏送离开，更早的就截到下一段开始）
    static func day(_ day: Date, _ visits: [Visit]) -> [Stay] {
        let d0 = Calendar.current.startOfDay(for: day), d1 = d0.addingTimeInterval(86400)
        let today = Calendar.current.isDateInToday(day)
        let cap = today ? Date() : d1
        func rank(_ v: Visit) -> Int { v.source == "visit" ? 0 : v.source == "slc" ? 1 : 2 }
        var kept: [Visit] = []
        for v in visits.sorted(by: { rank($0) < rank($1) }) {
            guard let a = v.arrive ?? v.depart else { continue }
            let z = v.depart ?? cap
            let dup = kept.firstIndex { k in
                let ka = k.arrive ?? k.depart!, kz = k.depart ?? cap
                let overlap = min(z, kz).timeIntervalSince(max(a, ka))
                return overlap > 0.5 * min(z.timeIntervalSince(a), kz.timeIntervalSince(ka))
            }
            if let i = dup {
                // 同一段：时间用排在前面的（系统到访的起止最准），地名和候选用定位更准的那条（误差大的点容易落到隔壁区域）
                if v.acc < kept[i].acc, v.name != nil, kept[i].picked != true, kept[i].label == nil || v.label != nil {
                    kept[i].name = v.name
                    kept[i].guesses = v.guesses
                }
            } else {
                kept.append(v)
            }
        }
        kept.sort { ($0.arrive ?? $0.depart!) < ($1.arrive ?? $1.depart!) }
        var out: [Stay] = []
        for (i, v) in kept.enumerated() {
            let a = max(v.arrive ?? v.depart!, d0)
            let next = i + 1 < kept.count ? (kept[i + 1].arrive ?? kept[i + 1].depart!) : nil
            let z = min(v.depart ?? next ?? cap, cap, d1)
            guard z > a else { continue }
            out.append(Stay(visit: v, start: a, end: z, isNow: today && v.depart == nil && next == nil))
        }
        return out
    }
}

/// 两段停留之间不超过 3 小时算路上（同桌面版和日数据包），更长的是没记到
func transit(_ a: Stay, _ b: Stay) -> TimeInterval? {
    let g = b.start.timeIntervalSince(a.end)
    return g > 60 && g <= 3 * 3600 ? g : nil
}

/// 地图上画点要换成 GCJ-02：停留点存的是 WGS-84 原值，苹果地图在国内用 GCJ-02 的底图，不换会偏几百米。
/// 国外的坐标原样返回。公开的标准换算公式
enum ChinaOffset {
    static func gcj(_ lat: Double, _ lon: Double) -> (Double, Double) {
        guard lon > 72.004, lon < 137.8347, lat > 0.8293, lat < 55.8271 else { return (lat, lon) }
        let a = 6378245.0, ee = 0.00669342162296594323
        let x = lon - 105, y = lat - 35
        var dLat = -100 + 2 * x + 3 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * sqrt(abs(x))
        dLat += (20 * sin(6 * x * .pi) + 20 * sin(2 * x * .pi)) * 2 / 3
        dLat += (20 * sin(y * .pi) + 40 * sin(y / 3 * .pi)) * 2 / 3
        dLat += (160 * sin(y / 12 * .pi) + 320 * sin(y * .pi / 30)) * 2 / 3
        var dLon = 300 + x + 2 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * sqrt(abs(x))
        dLon += (20 * sin(6 * x * .pi) + 20 * sin(2 * x * .pi)) * 2 / 3
        dLon += (20 * sin(x * .pi) + 40 * sin(x / 3 * .pi)) * 2 / 3
        dLon += (150 * sin(x / 12 * .pi) + 300 * sin(x / 30 * .pi)) * 2 / 3
        let rad = lat / 180 * .pi
        var m = sin(rad)
        m = 1 - ee * m * m
        let s = sqrt(m)
        dLat = dLat * 180 / ((a * (1 - ee)) / (m * s) * .pi)
        dLon = dLon * 180 / (a / s * cos(rad) * .pi)
        return (lat + dLat, lon + dLon)
    }
}

extension ChinaOffset {
    /// 停留点（WGS-84）→ 地图上的点
    static func map(_ lat: Double, _ lon: Double) -> CLLocationCoordinate2D {
        let (a, b) = gcj(lat, lon)
        return CLLocationCoordinate2D(latitude: a, longitude: b)
    }

    /// 地图上的点（GCJ-02）→ WGS-84：反过来逼近两次，误差在 1 米内
    static func wgs(_ c: CLLocationCoordinate2D) -> (lat: Double, lon: Double) {
        var lat = c.latitude, lon = c.longitude
        for _ in 0..<2 {
            let (g1, g2) = gcj(lat, lon)
            lat -= g1 - c.latitude
            lon -= g2 - c.longitude
        }
        return (lat, lon)
    }
}
