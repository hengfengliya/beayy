import CoreLocation
import Foundation

/// 一个停留点：在哪（WGS-84 原值，不转 GCJ-02）、几点到、几点走（还没走为空）。只记停留，不记路上的轨迹。
/// iPhone 和手表都会记：iPhone 用系统的到访（CLVisit）和显著位置变化；手表没有这两样，录音时隔几分钟取一个点自己算。
struct Visit: Codable, Equatable {
    var id: String        // visit-20261016T014500Z（到达时刻 UTC；到达未知时用离开时刻，前缀 visit-d）／stay-…（显著位置变化补的）／wstay-…（手表）
    var arrive: Date?
    var depart: Date?
    var lat: Double
    var lon: Double
    var acc: Double       // 水平精度，米
    var name: String?     // 地名（高德，手表记的到 iPhone 上再查）：「区 · 区域」（商场、小区、园区，或「…一带」）；确认了是哪家店才是「区 · 区域 · 店」。10-21 前是苹果反查的「区 · 区域 · 门牌」
    var source: String    // visit＝CLVisit；slc＝显著位置变化＋地理围栏补的短停留；watch＝手表录音时取点算的
    var rev = 1           // 每次改动（补上离开时间、补上地名）换一版；正数＝待上传，上传成功后取负
    var device: String? = nil  // 谁记的：nil＝这台 iPhone；watch＝手表（上传时 device 写 watch，桌面版归到手表）
    var label: String? = nil   // 自己起的名字（家、公司、健身房）：这个停留点落在起过名的地方 150 米内就带上，随停留点传给电脑
    var final: Bool? = nil     // 地名是离开以后（知道待了多久）取的，不再重取；只在本机，不上传
    var picked: Bool? = nil    // 地名是自己在「这次在哪」里挑的，不再自动改；只在本机，不上传
    var guesses: [Guess]? = nil // 是哪家店的候选（高德周边，带置信度），名字里只写区域；上传给电脑显示（最多 5 个）

    mutating func bump() { rev = abs(rev) + 1 }

    /// 同步服务上 places.<日> 里的一行（字段同以前 PUT /v1/places/{id} 的 body 加上 id，格式见 录音/README.md「位置」）
    func body(device phone: String) -> Data {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        f.timeZone = .current  // 带本地时区偏移，读的一方按它分到本地日
        let d: [String: Any] = [
            "id": id, "device": device ?? phone, "kind": "place.visit", "source": source,
            "arrive": arrive.map(f.string(from:)) ?? NSNull(), "depart": depart.map(f.string(from:)) ?? NSNull(),
            "lat": lat, "lon": lon, "acc": acc, "name": name ?? NSNull(), "label": label ?? NSNull(), "rev": abs(rev),
            // 是哪家店的候选：确认过的（名字里有店）和起过名的地方不传
            "guesses": (picked == true || label != nil ? nil : guesses?.prefix(5).map {
                ["name": $0.name, "type": $0.type, "p": $0.p, "distance": $0.distance] as [String: Any]
            }) ?? NSNull(),
        ]
        return try! JSONSerialization.data(withJSONObject: d, options: [.sortedKeys])
    }

    /// 归到哪一天（YYYY-MM-DD，按到达、没有就按离开，手机当前时区的本地日）：同步服务上一天一块 places.<日>
    var dayKey: String? {
        guard let t = arrive ?? depart else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = .current
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: t)
    }

    static func stamp(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        return f.string(from: d)
    }

    /// 显示的名字（不带区名）：只有区域时写区域，起过名的前面加名字（「家 · 青石小区」）；确认了店是「区域 · 店」，起过名的是「名字 · 店」。
    /// 后一层已经带着前一层（「青石小区二期」）就只写后一层
    var display: String? { Visit.display(name, label: label) }

    /// 短名字（地图上的点、小卡）：起的名字，没起就是店，没确认店就是区域
    var short: String? { label ?? name?.components(separatedBy: " · ").last }

    static func display(_ name: String?, label: String? = nil) -> String? {
        guard let name, !name.isEmpty else { return label }
        let parts = name.components(separatedBy: " · ")
        let b = parts.last!
        let a = label ?? (parts.count > 2 ? parts[1] : nil)
        guard let a, a != b else { return b }
        return label == nil && b.contains(a) ? b : "\(a) · \(b)"
    }
}

/// 是哪家店的一个候选：店名、高德分类（「餐饮服务;咖啡厅;咖啡厅」）、置信度（0–1）、离停留点多远
struct Guess: Codable, Equatable, Hashable {
    var name: String
    var type: String
    var p: Double
    var distance: Double
    var major: String { type.split(separator: ";").first.map(String.init) ?? "" }
    var minor: String { type.split(separator: ";").dropFirst().first.map(String.init) ?? major }
}

/// 用一串位置点判停留（手表用）：和上一个点在 120 米内就算还在原地，待够 8 分钟记一个停留（还没走，depart 为空），
/// 离开时再记一版补上离开时间。规则和 iPhone 的地理围栏补短停留一样。
struct StayDetector: Codable {
    static let radius: CLLocationDistance = 120
    static let minStay: TimeInterval = 8 * 60

    struct Here: Codable { var lat, lon, acc: Double; var since, last: Date; var id: String? }
    private(set) var here: Here?

    /// 现在这个地方待了多久（还没离开的那段）
    var stayed: TimeInterval? { here.map { $0.last.timeIntervalSince($0.since) } }

    /// 喂一个点，返回要记的停留（新的，或者补上离开时间的）
    mutating func feed(lat: Double, lon: Double, acc: Double, at t: Date, device: String) -> [Visit] {
        guard acc > 0, acc <= 200 else { return [] }  // 太粗的点不用
        guard var h = here else {
            here = Here(lat: lat, lon: lon, acc: acc, since: t, last: t, id: nil)
            return []
        }
        let d = CLLocation(latitude: lat, longitude: lon).distance(from: CLLocation(latitude: h.lat, longitude: h.lon))
        if d < Self.radius {
            h.last = t
            var out: [Visit] = []
            if h.id == nil && t.timeIntervalSince(h.since) >= Self.minStay {
                h.id = "wstay-" + Visit.stamp(h.since)
                out.append(visit(h, depart: nil, device: device))
            }
            here = h
            return out
        }
        // 走了：之前那段够了就补上离开时间，从这个点重新开始
        let out = h.id != nil ? [visit(h, depart: t, device: device, rev: 2)] : []
        here = Here(lat: lat, lon: lon, acc: acc, since: t, last: t, id: nil)
        return out
    }

    /// 不再取点了（停止录音）：在记着的那段以最后一个点为离开时间收尾
    mutating func flush(device: String) -> [Visit] {
        defer { here = nil }
        guard let h = here, h.id != nil else { return [] }
        return [visit(h, depart: h.last, device: device, rev: 2)]
    }

    private func visit(_ h: Here, depart: Date?, device: String, rev: Int = 1) -> Visit {
        Visit(id: h.id ?? "wstay-" + Visit.stamp(h.since), arrive: h.since, depart: depart, lat: h.lat, lon: h.lon,
              acc: h.acc, name: nil, source: "watch", rev: rev, device: device)
    }
}

extension Visit {
    /// 地名是门牌号或只有路名（反查给的最近的门牌）：挑名字时排在小区、园区、店名后面
    static func plainAddress(_ name: String) -> Bool {
        let n = name.components(separatedBy: " · ").last ?? name
        return (n.hasSuffix("号") && n.contains { $0.isNumber }) || ["路", "街", "巷", "道"].contains { n.hasSuffix($0) }
    }

    /// 列表里显示的起止：09:12–18:40；没走写「现在」
    var span: String {
        let a = arrive.map(hm) ?? "?"
        return "\(a)–\(depart.map(hm) ?? tr("现在"))"
    }
}
