import CoreLocation
import Foundation
import UIKit

/// 位置：CLVisit 记到访；显著位置变化唤醒后在当前点圈一个地理围栏，在圈里待够 8 分钟、出圈时记一个短停留（补 CLVisit 漏掉的）。
/// 两种都不需要后台定位模式：app 被杀了系统也会在后台拉起来送事件，所以启动时就要建好 manager（AppDelegate 里）。
final class Places: NSObject, ObservableObject, CLLocationManagerDelegate {
    static let shared = Places()
    static let stayRadius: CLLocationDistance = 120
    static let stayMin: TimeInterval = 8 * 60
    private static let regionID = "beayy-stay"

    @Published private(set) var auth: CLAuthorizationStatus = .notDetermined
    @Published private(set) var precise = true
    @Published private(set) var recent: [Visit] = []
    /// 自检：系统最近一次送来位置事件的时刻和种类（到访／位置变化／离开圈），存在本机，app 被杀也留着
    @Published private(set) var lastEvent: (at: Date, kind: String)? = Places.loadLastEvent()
    /// 显著位置变化圈住的这个地方从几点开始待着（还没够 8 分钟或还没离开）
    @Published private(set) var hereSince: Date?
    /// 自己起的地名
    @Published private(set) var labels: [PlaceLabel] = Places.loadLabels()
    private let manager = CLLocationManager()

    /// 显著位置变化圈的点：只在本机 UserDefaults 里放一个，出圈就清，不落盘、不上传
    private struct Candidate: Codable { var lat, lon, acc: Double; var at: Date }
    private var candidate: Candidate? {
        get { UserDefaults.standard.data(forKey: "stayCandidate").flatMap { try? JSONDecoder().decode(Candidate.self, from: $0) } }
        set {
            UserDefaults.standard.set(newValue.flatMap { try? JSONEncoder().encode($0) }, forKey: "stayCandidate")
            DispatchQueue.main.async { self.hereSince = newValue?.at }
        }
    }

    private static func loadLastEvent() -> (at: Date, kind: String)? {
        guard let t = UserDefaults.standard.object(forKey: "lastPlaceEvent") as? Date else { return nil }
        return (t, UserDefaults.standard.string(forKey: "lastPlaceEventKind") ?? "")
    }

    private func noteEvent(_ kind: String) {
        let now = Date()
        UserDefaults.standard.set(now, forKey: "lastPlaceEvent")
        UserDefaults.standard.set(kind, forKey: "lastPlaceEventKind")
        DispatchQueue.main.async { self.lastEvent = (now, kind) }
    }

    static var dir: URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("places", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    func start() {
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyHundredMeters
        manager.pausesLocationUpdatesAutomatically = true
        updateAuth()
        if auth == .authorizedAlways {
            manager.startMonitoringVisits()
            manager.startMonitoringSignificantLocationChanges()
        }
        hereSince = candidate?.at
        loadRecent()
    }

    var ready: Bool { auth == .authorizedAlways && precise }
    var authText: String {
        switch auth {
        case .authorizedAlways: return tr("始终允许")
        case .authorizedWhenInUse: return tr("使用期间（要改成始终）")
        case .denied: return tr("不允许")
        case .restricted: return tr("受限")
        default: return tr("还没问")
        }
    }

    func requestPermission() {
        if auth == .notDetermined { manager.requestWhenInUseAuthorization() }
        else if auth == .authorizedWhenInUse && !UserDefaults.standard.bool(forKey: "askedAlways") {
            UserDefaults.standard.set(true, forKey: "askedAlways")
            manager.requestAlwaysAuthorization()  // 系统只问一次，之后得去设置里改
        }
        else if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
    }

    private func updateAuth() {
        let a = manager.authorizationStatus, p = manager.accuracyAuthorization == .fullAccuracy
        DispatchQueue.main.async { self.auth = a; self.precise = p }
    }

    func locationManagerDidChangeAuthorization(_ m: CLLocationManager) {
        updateAuth()
        switch m.authorizationStatus {
        case .authorizedWhenInUse where !UserDefaults.standard.bool(forKey: "askedAlways"):
            UserDefaults.standard.set(true, forKey: "askedAlways")
            m.requestAlwaysAuthorization()
        case .authorizedAlways: m.startMonitoringVisits(); m.startMonitoringSignificantLocationChanges()
        default: break
        }
    }

    // MARK: CLVisit

    func locationManager(_ m: CLLocationManager, didVisit v: CLVisit) {
        noteEvent("到访")
        let arrive = v.arrivalDate == .distantPast ? nil : v.arrivalDate
        let depart = v.departureDate == .distantFuture ? nil : v.departureDate
        guard let key = arrive ?? depart else { return }
        let id = (arrive != nil ? "visit-" : "visit-d") + Visit.stamp(key)
        if var old = load(id) {  // 离开时系统再送一次同一个到访，补上离开时间
            guard old.depart != depart || old.lat != v.coordinate.latitude else { return }
            old.depart = depart ?? old.depart
            old.lat = v.coordinate.latitude; old.lon = v.coordinate.longitude; old.acc = v.horizontalAccuracy
            old.bump()
            return record(old)
        }
        record(Visit(id: id, arrive: arrive, depart: depart, lat: v.coordinate.latitude,
                     lon: v.coordinate.longitude, acc: v.horizontalAccuracy, source: "visit"))
    }

    // MARK: 显著位置变化 → 地理围栏补短停留

    func locationManager(_ m: CLLocationManager, didUpdateLocations locs: [CLLocation]) {
        guard let loc = locs.last else { return }
        noteEvent("位置变化")
        if loc.horizontalAccuracy > 200 {  // 显著位置变化给的多半是基站级的粗位置，要一个更准的再圈
            m.requestLocation()
            return
        }
        if let c = candidate {
            let d = loc.distance(from: CLLocation(latitude: c.lat, longitude: c.lon))
            if d < Self.stayRadius { return }  // 还在圈里
            closeCandidate(at: loc.timestamp)    // 出圈事件没来（或晚了），用这次的时间当离开
        }
        candidate = Candidate(lat: loc.coordinate.latitude, lon: loc.coordinate.longitude,
                              acc: loc.horizontalAccuracy, at: loc.timestamp)
        for r in m.monitoredRegions where r.identifier == Self.regionID { m.stopMonitoring(for: r) }
        let region = CLCircularRegion(center: loc.coordinate, radius: Self.stayRadius, identifier: Self.regionID)
        region.notifyOnEntry = false
        region.notifyOnExit = true
        m.startMonitoring(for: region)
    }

    func locationManager(_ m: CLLocationManager, didExitRegion region: CLRegion) {
        guard region.identifier == Self.regionID else { return }
        noteEvent("离开")
        closeCandidate(at: Date())
        m.stopMonitoring(for: region)
        m.requestLocation()  // 在新位置再圈一个，停下来的地方才圈得住
    }

    func locationManager(_ m: CLLocationManager, didFailWithError error: Error) {}
    func locationManager(_ m: CLLocationManager, monitoringDidFailFor region: CLRegion?, withError error: Error) {}

    private func closeCandidate(at t: Date) {
        guard let c = candidate else { return }
        candidate = nil
        guard t.timeIntervalSince(c.at) >= Self.stayMin else { return }  // 路过，不算停留
        record(Visit(id: "stay-" + Visit.stamp(c.at), arrive: c.at, depart: t, lat: c.lat, lon: c.lon, acc: c.acc, source: "slc"))
    }

    // MARK: 存、反查地名、上传

    private func record(_ visit: Visit) {
        var v = visit
        // 要取名：还没名字（套了自己填的地址也要取一次，拿区名）；或者刚离开，名字还是到达时猜的（知道待了多久能挑得更准）。自己挑过的不动
        let needsName = v.name == nil || (v.depart != nil && v.final != true && v.picked != true)
        applyLabel(&v)
        save(v)
        let bg = UIApplication.shared.beginBackgroundTask(withName: "place")
        let finish = {
            Uploader.shared.uploadPending()
            self.loadRecent()
            UIApplication.shared.endBackgroundTask(bg)
        }
        guard needsName else { return finish() }
        Task { @MainActor in
            if let (name, gs) = await self.lookUp(v), var cur = self.load(v.id), cur.picked != true {
                cur.name = name
                cur.guesses = gs
                if cur.depart != nil { cur.final = true }
                self.applyLabel(&cur)
                cur.bump()
                self.save(cur)
            }
            finish()
        }
    }

    /// 停留点的地名和候选（只问高德）：名字是「区 · 区域」；候选店带置信度。以前在这儿确认过的店排第一、置信度到 60% 以上，
    /// 就当已经确认，写进名字「区 · 区域 · 店」。没配 Key、没网、高德出错返回 nil：先不取名，下次打开 app 再取（retryNames）
    func lookUp(_ v: Visit) async -> (String, [Guess])? {
        guard let reg = try? await Amap.region(lat: v.lat, lon: v.lon, acc: v.acc) else { return nil }
        try? await Task.sleep(nanoseconds: 350_000_000)  // 高德限 3 次／秒
        let here = CLLocation(latitude: v.lat, longitude: v.lon)
        let pois = (try? await Amap.around(lat: v.lat, lon: v.lon, radius: Int(max(50, min(v.acc * 2, 120))))) ?? []
        let gs = Array(Amap.guesses(pois, remembered: picked, arrive: v.arrive, depart: v.depart, acc: v.acc, here: here).prefix(5))
        var parts = [reg.district, reg.area]
        if let top = gs.first, top.p >= 0.6,
           picked.contains(where: { $0.name == top.name && CLLocation(latitude: $0.lat, longitude: $0.lon).distance(from: here) <= 30 }) {
            parts.append(Amap.short(top.name))
        }
        let name = parts.compactMap { $0 }.joined(separator: " · ")
        return name.isEmpty ? nil : (name, gs)
    }

    /// 还没取到名的（当时没网、高德出错）再取一次：打开 app 时调
    func retryNames() {
        for v in allVisits() where v.name == nil { record(v) }
    }

    // MARK: 确认是哪家店

    /// 自己确认过的店（picked.json）：以后 30 米内的停留，它排在候选最前面
    @Published private(set) var picked: [Picked] = Places.loadPicked()

    private static var pickedURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("picked.json")
    }

    private static func loadPicked() -> [Picked] {
        (try? Data(contentsOf: pickedURL)).flatMap { try? JSONDecoder().decode([Picked].self, from: $0) } ?? []
    }

    /// 「这次在哪」里确认了一家店（或自己输入的店名）：地名换成「区 · 区域 · 店」（区域用显示的那条，同一段有两条记录时是定位更准的那条），以后不再自动改，换一版重传；
    /// 记下这家店（同一处同名的只留一条，新的在后）。店的坐标用高德给的，自己输入的用停留点的
    func pick(_ id: String, region: String?, shop: String, type: String?, lat: Double?, lon: Double?) {
        guard var v = load(id) else { return }
        let base = (region ?? v.name ?? "").components(separatedBy: " · ").prefix(2).joined(separator: " · ")
        v.name = [base.isEmpty ? nil : base, Amap.short(shop)].compactMap { $0 }.joined(separator: " · ")
        v.picked = true
        v.final = true
        applyLabel(&v)
        v.bump()
        save(v)
        let p = Picked(name: shop, type: type, lat: lat ?? v.lat, lon: lon ?? v.lon, at: Date())
        var all = picked.filter {
            !($0.name == p.name && CLLocation(latitude: $0.lat, longitude: $0.lon).distance(from: CLLocation(latitude: p.lat, longitude: p.lon)) <= 30)
        }
        all.append(p)
        try? JSONEncoder().encode(all).write(to: Self.pickedURL, options: .atomic)
        picked = all
        loadRecent()
        Uploader.shared.uploadPending()
    }

    /// 撤销确认：名字退回只写区域，以后照常自动取
    func unpick(_ id: String) {
        guard var v = load(id) else { return }
        v.name = (v.name ?? "").components(separatedBy: " · ").prefix(2).joined(separator: " · ")
        v.picked = nil
        applyLabel(&v)
        v.bump()
        save(v)
        loadRecent()
        Uploader.shared.uploadPending()
    }

    /// 重新取名（最近 30 天，自己确认过的不动）：换了取名方式以后把以前的也改过来。高德限 3 次／秒，每个停留要 2–3 次，隔 1 秒一个
    @Published private(set) var renaming: (done: Int, total: Int)?

    @MainActor
    func renameRecent() async {
        let since = Date().addingTimeInterval(-30 * 86400)
        let todo = allVisits().filter { $0.picked != true && ($0.arrive ?? $0.depart ?? .distantPast) > since }
        renaming = (0, todo.count)
        for (i, v) in todo.enumerated() {
            if let (name, gs) = await lookUp(v), var cur = load(v.id), cur.picked != true {
                let before = (cur.name, cur.guesses)
                cur.name = name
                cur.guesses = gs
                cur.final = cur.depart != nil
                applyLabel(&cur)
                if cur.name != before.0 || cur.guesses != before.1 { cur.bump() }
                save(cur)
            }
            renaming = (i + 1, todo.count)
            try? await Task.sleep(nanoseconds: 1_000_000_000)
        }
        renaming = nil
        loadRecent()
        Uploader.shared.uploadPending()
    }

    /// 改一个停留点的位置（定位偏了，在地图上对准实际的地方）：换坐标、重新反查地名、套名字，换一版重传
    func move(_ id: String, lat: Double, lon: Double) {
        guard var v = load(id) else { return }
        v.lat = lat
        v.lon = lon
        v.acc = 10
        v.name = nil
        v.bump()
        record(v)
    }

    /// 手表记的停留（经 WatchConnectivity 送来）：同一个 id 的新一版才收，收下后照样反查地名、上传
    func ingest(_ w: Visit) {
        var v = w
        if let old = load(w.id) {
            guard old.depart != w.depart || old.lat != w.lat else { return }
            v.name = old.name
            v.rev = abs(old.rev) + 1
        } else {
            v.rev = 1
        }
        record(v)
    }

    /// 某一天的停留点（本机只留 30 天）：这天到的，和前一天到、这天才走（或还没走）的；按到达先后
    func visits(on day: Date) -> [Visit] {
        let start = Calendar.current.startOfDay(for: day), end = start.addingTimeInterval(86400)
        let all = allVisits()
        return all.filter { v in
            guard let a = v.arrive ?? v.depart else { return false }
            if a >= start && a < end { return true }
            return a < start && (v.depart.map { $0 > start } ?? (all.allSatisfy { ($0.arrive ?? .distantPast) <= a || ($0.arrive ?? .distantPast) > start }))
        }
        .sorted { ($0.arrive ?? $0.depart ?? .distantPast) < ($1.arrive ?? $1.depart ?? .distantPast) }
    }

    /// 本机留着的全部停留点（最近 30 天，iPhone 和手表的）
    func allVisits() -> [Visit] {
        let files = (try? FileManager.default.contentsOfDirectory(at: Self.dir, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }.compactMap { try? JSONDecoder().decode(Visit.self, from: Data(contentsOf: $0)) }
            .filter { $0.arrive != $0.depart || $0.arrive == nil }  // 到达＝离开的是作废的
    }

    /// 今天记下的停留点（iPhone 和手表的都算）
    var todayCount: Int {
        recent.filter { ($0.arrive ?? $0.depart).map(Calendar.current.isDateInToday) ?? false }.count
    }

    // MARK: 自己起的地名（家、公司、健身房）

    /// 存在本机 Application Support/place-labels.json。起名后 150 米内的停留都带上这个名字（新记的，和最近 30 天已经记下的），随停留点传给电脑
    static var labelsURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("place-labels.json")
    }

    private static func loadLabels() -> [PlaceLabel] {
        (try? Data(contentsOf: labelsURL)).flatMap { try? JSONDecoder().decode([PlaceLabel].self, from: $0) } ?? []
    }

    /// 这个坐标落在哪个起过名的地方（半径内最近的那个）
    func label(lat: Double, lon: Double) -> PlaceLabel? {
        let p = CLLocation(latitude: lat, longitude: lon)
        return labels.map { ($0, CLLocation(latitude: $0.lat, longitude: $0.lon).distance(from: p)) }
            .filter { $0.1 <= $0.0.r }.min { $0.1 < $1.1 }?.0
    }

    /// 给停留点套上它所在的起名地方：名字；起名时填了地址（苹果反查给的门牌不对，比如小区楼栋查成了旁边路的门牌号），
    /// 地名换成「区 · 地址」，电脑上也跟着对。返回有没有改
    @discardableResult
    func applyLabel(_ v: inout Visit) -> Bool {
        let l = label(lat: v.lat, lon: v.lon)
        var changed = false
        if v.label != l?.label {
            v.label = l?.label
            changed = true
        }
        if let a = l?.address, !a.isEmpty, v.picked != true {
            let district = v.name?.components(separatedBy: " · ").first.flatMap { $0 == a ? nil : $0 }
            let want = [district, a].compactMap { $0 }.joined(separator: " · ")
            if v.name != want {
                v.name = want
                changed = true
            }
        }
        return changed
    }

    /// 删掉这里的名字
    func removeLabel(lat: Double, lon: Double) {
        replaceLabel(label(lat: lat, lon: lon), with: nil)
    }

    /// 改「常去的地方」名单里的一项（名字、种类、半径），new 为空＝删掉；然后给最近 30 天的停留点重新套名字、上传
    func replaceLabel(_ old: PlaceLabel?, with new: PlaceLabel?) {
        var all = labels
        if let old, let i = all.firstIndex(of: old) {
            if let new { all[i] = new } else { all.remove(at: i) }
        } else if let new {
            all.append(new)
        }
        try? JSONEncoder().encode(all).write(to: Self.labelsURL, options: .atomic)
        labels = all
        let files = (try? FileManager.default.contentsOfDirectory(at: Self.dir, includingPropertiesForKeys: nil)) ?? []
        let since = Date().addingTimeInterval(-30 * 86400)
        for f in files where f.pathExtension == "json" {
            guard var v = try? JSONDecoder().decode(Visit.self, from: Data(contentsOf: f)),
                  (v.arrive ?? v.depart ?? .distantPast) > since else { continue }
            guard applyLabel(&v) else { continue }
            v.bump()
            save(v)
        }
        loadRecent()
        Uploader.shared.uploadPending()
    }

    func load(_ id: String) -> Visit? {
        (try? Data(contentsOf: Self.dir.appendingPathComponent("\(id).json")))
            .flatMap { try? JSONDecoder().decode(Visit.self, from: $0) }
    }

    private func save(_ v: Visit) {
        try? JSONEncoder().encode(v).write(to: Self.dir.appendingPathComponent("\(v.id).json"), options: .atomic)
    }

    /// 上传成功：同一版就把待传标记去掉（文件留着给首页显示最近到访，30 天后删）
    func uploaded(id: String, rev: Int) {
        guard var v = load(id), v.rev == rev else { return }
        v.rev = -rev  // 负数＝这一版已上传
        save(v)
        loadRecent()
    }

    /// 待上传的：rev > 0
    func pendingVisits() -> [Visit] {
        let files = (try? FileManager.default.contentsOfDirectory(at: Self.dir, includingPropertiesForKeys: nil)) ?? []
        return files.filter { $0.pathExtension == "json" }
            .compactMap { try? JSONDecoder().decode(Visit.self, from: Data(contentsOf: $0)) }
            .filter { $0.rev > 0 }
    }

    /// 某一天（dayKey）要传的一整块：这天每个停留点一行（作废的不传），按到达排；revs 是这块里待传的各版，传成功后标记已传
    func daySnapshot(_ day: String, device: String) -> (rows: Data, revs: [String: Int]) {
        let files = (try? FileManager.default.contentsOfDirectory(at: Self.dir, includingPropertiesForKeys: nil)) ?? []
        let all = files.filter { $0.pathExtension == "json" }
            .compactMap { try? JSONDecoder().decode(Visit.self, from: Data(contentsOf: $0)) }
            .filter { $0.dayKey == day }
        var rows = Data()
        for v in all.filter({ $0.arrive != $0.depart || $0.arrive == nil })
            .sorted(by: { ($0.arrive ?? $0.depart ?? .distantPast) < ($1.arrive ?? $1.depart ?? .distantPast) }) {
            rows.append(v.body(device: device))
            rows.append(0x0a)
        }
        return (rows, Dictionary(all.filter { $0.rev > 0 }.map { ($0.id, $0.rev) }, uniquingKeysWith: max))
    }

    private func loadRecent() {
        let fm = FileManager.default
        let files = (try? fm.contentsOfDirectory(at: Self.dir, includingPropertiesForKeys: nil)) ?? []
        var all = files.filter { $0.pathExtension == "json" }
            .compactMap { try? JSONDecoder().decode(Visit.self, from: Data(contentsOf: $0)) }
        let old = Date().addingTimeInterval(-30 * 86400)
        for v in all where v.rev < 0 && (v.depart ?? v.arrive ?? .distantFuture) < old {
            try? fm.removeItem(at: Self.dir.appendingPathComponent("\(v.id).json"))
        }
        all.sort { ($0.arrive ?? $0.depart ?? .distantPast) > ($1.arrive ?? $1.depart ?? .distantPast) }
        let top = Array(all.prefix(30))
        DispatchQueue.main.async { self.recent = top }
    }
}

/// 自己起的一个地名：中心坐标（WGS-84）和半径，种类 home／work／other（同电脑上 labels.json 的 type）
struct PlaceLabel: Codable, Equatable {
    var label: String
    var kind: String
    var lat: Double
    var lon: Double
    var r: Double = 150
    /// 自己填的地址（「青石小区1幢」）：苹果反查的门牌不对时用，范围内的停留地名都换成它
    var address: String? = nil
}
