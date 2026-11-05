import CoreLocation
import Foundation

/// 停留点在哪：只用高德一个来源（苹果在国内反查只给门牌街道、给不了附近地点；两个来源合用店名、坐标、分类都会冲突）。
/// 名字只写有把握的一层——区域（高德逆地理编码的 AOI：商场、小区、园区；不在任何区域里就写街道「…一带」）；
/// 具体是哪家店给几个带置信度的候选（高德周边搜索，按距离、待的时长和时段算），自己点一下确认了才写进名字，并记住。
/// 类别一律用高德的 POI 分类（「餐饮服务;咖啡厅;咖啡厅」），不自己编。
/// 官方服务（电脑选的，或者没有自己的 Key）：经同步服务 /v2/amap/… 中转，用服务器的高德 Key、记账号的地图用量；
/// 自己的：Key 用电脑经 account/config.json 推来的，没有就用编进去的 Secrets.xcconfig 的 BEAYY_AMAP_KEY（不进 git）。
/// 坐标发给高德前换成 GCJ-02；每个停留点的坐标会发给高德
enum Amap {
    static var key: String { Account.shared.amapKey ?? Bundle.main.object(forInfoDictionaryKey: "BeayyAmapKey") as? String ?? "" }

    /// 附近的一个地点：名字、高德分类（「餐饮服务;咖啡厅;咖啡厅」）、离停留点多远、坐标（换回 WGS-84）
    struct POI: Identifiable, Hashable {
        var id: String
        var name: String
        var type: String
        var distance: Double
        var lat: Double
        var lon: Double
        /// 高德分类的大类、中类
        var major: String { type.split(separator: ";").first.map(String.init) ?? "" }
        var minor: String { type.split(separator: ";").dropFirst().first.map(String.init) ?? major }
    }

    enum Failure: LocalizedError {
        case noKey, bad(String)
        var errorDescription: String? {
            switch self {
            case .noKey: return tr("没配高德 Key")
            case .bad(let s): return tr("高德没返回结果：{s}", ["s": s])
            }
        }
    }

    // MARK: 请求

    private static func get(_ path: String, _ q: [String: String]) async throws -> [String: Any] {
        // 电脑上选了 OpenStreetMap：手机不问高德，地名和候选由电脑取、同步过来（10-30）
        guard Account.shared.mapProvider == "amap" else { throw Failure.bad(tr("用的是 OpenStreetMap，地名由电脑取")) }
        let r: URLRequest
        if Account.shared.amapOfficial, Account.shared.credentials?.isServer == true {
            // 官方服务（香港那台，到高德快），凭地图票据
            var c = URLComponents(string: "\(Account.defaultTranscribe)/v2/amap/\(path)")!
            c.queryItems = q.map { URLQueryItem(name: $0.key, value: $0.value) }
            var x = URLRequest(url: c.url!)
            x.setValue("Ticket \(try await Account.shared.ticket("map"))", forHTTPHeaderField: "Authorization")
            r = x
        } else {
            guard !key.isEmpty else { throw Failure.noKey }
            var c = URLComponents(string: "https://restapi.amap.com/v3/\(path)")!
            c.queryItems = ([("key", key)] + q.map { ($0.key, $0.value) }).map { URLQueryItem(name: $0.0, value: $0.1) }
            r = URLRequest(url: c.url!)
        }
        let (data, _) = try await URLSession.shared.data(for: r)
        guard let o = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw Failure.bad(tr("不是 JSON")) }
        if let e = (o["error"] as? String) ?? (o["detail"] as? String) { throw Failure.bad(e) }  // 官方服务：没开通、额度用完
        guard str(o["status"]) == "1" else { throw Failure.bad(str(o["info"]) ?? tr("未知")) }
        return o
    }

    private static func location(_ lat: Double, _ lon: Double) -> String {
        let g = ChinaOffset.map(lat, lon)
        return String(format: "%.6f,%.6f", g.longitude, g.latitude)
    }

    /// 高德的字段有值时是字符串、没值时是 []，数字有时也是字符串，统一在这里取
    private static func str(_ v: Any?) -> String? {
        if let s = v as? String, !s.isEmpty { return s }
        if let n = v as? NSNumber { return n.stringValue }
        return nil
    }

    private static func pois(_ arr: Any?) -> [POI] {
        (arr as? [[String: Any]] ?? []).compactMap { p in
            guard let name = str(p["name"]), let loc = str(p["location"]) else { return nil }
            let xy = loc.split(separator: ",").compactMap { Double($0) }
            guard xy.count == 2 else { return nil }
            let w = ChinaOffset.wgs(CLLocationCoordinate2D(latitude: xy[1], longitude: xy[0]))
            return POI(id: str(p["id"]) ?? name, name: name, type: str(p["type"]) ?? "",
                       distance: Double(str(p["distance"]) ?? "") ?? 0, lat: w.lat, lon: w.lon)
        }
    }

    // MARK: 区域

    /// 一个点所在的区域：区名（城东区）和区域（万象广场／青石小区／「青石路一带」）
    struct Region { var district: String?; var area: String? }

    /// 逆地理编码取区域：点落在 AOI 面内就从这些里挑；一个都不在，才用定位误差内挨着的。几个区域套在一起时，
    /// 先挑不是住宅、写字楼的（商场、景区、学校），再挑范围大的（区域要的是大范围：校区而不是里面的一栋楼）；都没有就写街道「…一带」
    static func region(lat: Double, lon: Double, acc: Double) async throws -> Region {
        let o = try await get("geocode/regeo", ["location": location(lat, lon), "extensions": "all", "radius": "100", "roadlevel": "0"])
        guard let r = o["regeocode"] as? [String: Any] else { throw Failure.bad(tr("没有 regeocode")) }
        let comp = r["addressComponent"] as? [String: Any] ?? [:]
        let tolerance = max(15, min(acc / 2, 40))
        let aois = (r["aois"] as? [[String: Any]] ?? []).compactMap { a -> (name: String, type: String, area: Double, d: Double)? in
            guard let name = str(a["name"]) else { return nil }
            return (name, str(a["type"]) ?? "", Double(str(a["area"]) ?? "") ?? .infinity, Double(str(a["distance"]) ?? "") ?? .infinity)
        }
        let within = aois.filter { $0.d == 0 }
        let inside = within.isEmpty ? aois.filter { $0.d <= tolerance }.sorted { $0.d < $1.d }.prefix(1).map { $0 } : within
        let best = inside.sorted { x, y in
            let bx = x.type.hasPrefix("12"), by = y.type.hasPrefix("12")  // 12xxxx 商务住宅（小区、写字楼）排后
            return bx != by ? !bx : x.area > y.area
        }.first
        let street = (comp["streetNumber"] as? [String: Any]).flatMap { str($0["street"]) }
        let area = best?.name ?? street.map { "\($0)一带" } ?? str(comp["township"])
        return Region(district: str(comp["district"]), area: area)
    }

    // MARK: 附近的店

    /// 算「去的是哪家」时要的地点：高德大类里的去处（汽车服务～风景名胜、政府、科教文化、金融、公司），不要商务住宅、交通设施、道路、地名、公共设施
    static let placeTypes = "010000|020000|030000|040000|050000|060000|070000|080000|090000|100000|110000|130000|140000|160000|170000"

    /// 周边搜索：半径内的地点按距离，最多 50 个
    static func around(lat: Double, lon: Double, radius: Int) async throws -> [POI] {
        var out: [POI] = []
        for page in 1...2 {
            let o = try await get("place/around", ["location": location(lat, lon), "radius": "\(radius)", "sortrule": "distance",
                                                    "types": placeTypes, "offset": "25", "page": "\(page)"])
            let ps = pois(o["pois"])
            out += ps
            if ps.count < 25 { break }
            try? await Task.sleep(nanoseconds: 350_000_000)  // 高德限 3 次／秒
        }
        return out
    }

    /// 按名字搜地点（起名、添加常去的地方、「都不是」时用）：有位置就按离这里多远排，没有就全国按相关度
    static func search(_ text: String, lat: Double?, lon: Double?) async throws -> [POI] {
        if let lat, let lon {
            let o = try await get("place/around", ["location": location(lat, lon), "keywords": text, "radius": "50000",
                                                    "sortrule": "distance", "offset": "20"])
            return pois(o["pois"])
        }
        let o = try await get("place/text", ["keywords": text, "offset": "20"])
        return pois(o["pois"])
    }

    // MARK: 候选和置信度

    /// 店名去掉括号里的分店名：「星巴克(万象广场店)」→「星巴克」
    static func short(_ name: String) -> String {
        var s = name
        for (open, close) in [("(", ")"), ("（", "）")] {
            if let i = s.range(of: open), s.hasSuffix(close), i.lowerBound > s.startIndex { s = String(s[..<i.lowerBound]) }
        }
        return s.trimmingCharacters(in: .whitespaces)
    }

    /// 喝东西的那几种（高德中类）
    private static let drinks = ["咖啡厅", "冷饮店", "甜品店", "茶艺馆", "糕饼店"]

    /// 给附近的地点打分算置信度：离得越近越像；待的时长和时段对得上的加分（饭点待 18 分钟以上的餐馆，待 5–35 分钟的咖啡、冷饮、甜品）；
    /// 以前在这附近（30 米内）确认过的地点加很多分。分数换成概率：exp(-分/τ)，τ 跟定位误差走（误差大就更拿不准）
    static func guesses(_ pois: [POI], remembered: [Picked], arrive: Date?, depart: Date?, acc: Double, here: CLLocation) -> [Guess] {
        let dur = arrive.flatMap { a in depart.map { $0.timeIntervalSince(a) } }
        let hour = arrive.map { Double(Calendar.current.component(.hour, from: $0)) + Double(Calendar.current.component(.minute, from: $0)) / 60 }
        let meal = hour.map { (11.0...14.0).contains($0) || (17.0...21.0).contains($0) } ?? false
        func bonus(_ type: String) -> Double {
            guard let dur else { return 0 }
            let p = POI(id: "", name: "", type: type, distance: 0, lat: 0, lon: 0)
            let drink = drinks.contains(p.minor)
            if meal, dur >= 18 * 60, p.major == "餐饮服务", !drink { return 40 }
            if (5.0 * 60...35.0 * 60).contains(dur), drink { return 35 }
            return 0
        }
        // 记住的：30 米内确认过的地点，不在高德列表里（自己输入的店名）也加进来
        let near = remembered.filter { CLLocation(latitude: $0.lat, longitude: $0.lon).distance(from: here) <= 30 }
        var cands: [(name: String, type: String, d: Double, score: Double)] = pois.map { p in
            let r = near.contains { $0.name == p.name }
            return (p.name, p.type, p.distance, p.distance - bonus(p.type) - (r ? 60 : 0))
        }
        for r in near where !cands.contains(where: { $0.name == r.name }) {
            let d = CLLocation(latitude: r.lat, longitude: r.lon).distance(from: here)
            cands.append((r.name, r.type ?? "", d, d - bonus(r.type ?? "") - 60))
        }
        guard !cands.isEmpty else { return [] }
        let tau = max(15, acc / 2)
        let best = cands.map(\.score).min()!
        let w = cands.map { exp(-($0.score - best) / tau) }
        let z = w.reduce(0, +)
        return zip(cands, w).map { c, w in Guess(name: c.name, type: c.type, p: w / z, distance: c.d) }
            .sorted { $0.p > $1.p }
    }

    /// 在做什么：候选的置信度按高德大类加起来，取最大的（「餐饮服务 85%」）
    static func doing(_ gs: [Guess]) -> (String, Double)? {
        var m: [String: Double] = [:]
        for g in gs { m[g.major, default: 0] += g.p }
        return m.filter { !$0.key.isEmpty }.max { $0.value < $1.value }
    }
}

/// 自己确认过的一个地点（「这次在哪」里点的，或自己输入的店名）：在哪、叫什么、高德分类。存在 Application Support/picked.json，
/// 以后在 30 米内的停留，它排在候选最前面
struct Picked: Codable, Equatable {
    var name: String
    var type: String?
    var lat: Double
    var lon: Double
    var at: Date
}
