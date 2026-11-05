import Foundation

/// 转写好的文字：电脑转写后传到同步服务的 transcript.<段 id>（transcripts 格式的 JSONL，AAD＝「<设备>/transcript.<段 id>」）。
/// 列表增量取（GET /v1/list?prefix=transcript.&since=…，满 5000 条接着取），按段 id 里的时刻挑出某一天的；
/// 解开的每段存本机 Application Support/transcripts/seg/，服务器上那段没更新就不再下
actor TranscriptStore {
    static let shared = TranscriptStore()

    private struct Index: Codable {
        var user: String
        var since: Int64 = 0
        /// "<设备>/<块名>" → 服务器上的更新时刻（毫秒）
        var remote: [String: Int64] = [:]
        /// 本机存的那份是哪次更新的
        var local: [String: Int64] = [:]
    }

    private var index: Index?
    /// 同一天同时只取一次（今天、对话两个页签会一起要）
    private var running: [String: Task<(String, String?), Error>] = [:]

    private static var root: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("transcripts", isDirectory: true)
    }

    private static var segDir: URL {
        let d = root.appendingPathComponent("seg", isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    private static var indexURL: URL { root.appendingPathComponent("index.json") }

    private static func file(_ key: String) -> URL { segDir.appendingPathComponent(key.replacingOccurrences(of: "/", with: "_") + ".jsonl") }

    /// 退出登录：解开的都删掉
    func reset() {
        index = nil
        try? FileManager.default.removeItem(at: Self.root)
    }

    /// 某天的转写（这天开始的段，加上前一天最后一段），返回 JSONL 和一句没取全时的提示
    func text(day: Date) async throws -> (String, String?) {
        let k = DayState.key(day)
        if let t = running[k] { return try await t.value }
        let t = Task { try await self.load(day) }
        running[k] = t
        defer { running[k] = nil }
        return try await t.value
    }

    private func load(_ day: Date) async throws -> (String, String?) {
        guard let c = Account.shared.credentials else { throw ServiceError(tr("没登录账号")) }
        guard c.isServer else { throw ServiceError(Account.noServer) }
        var idx = loadIndex(user: c.user)
        var warning: String?
        do {
            idx = try await Self.refreshList(c, idx)
        } catch {
            warning = tr("转写列表没取到（{e}），先显示手机上已有的", ["e": error.localizedDescription])
        }
        let d0 = Calendar.current.startOfDay(for: day)
        let d1 = Calendar.current.date(byAdding: .day, value: 1, to: d0)!
        let keys = idx.remote.keys.filter { k in Self.start(of: k).map { $0 >= d0.addingTimeInterval(-15 * 60) && $0 < d1 } ?? false }.sorted()
        let need = keys.filter { idx.local[$0] != idx.remote[$0] || !FileManager.default.fileExists(atPath: Self.file($0).path) }
        var failed: [String] = []
        await withTaskGroup(of: (String, Result<Data, Error>).self) { g in
            var rest = need.makeIterator()
            for _ in 0..<8 {
                guard let k = rest.next() else { break }
                g.addTask { (k, await Self.fetch(c, k)) }
            }
            while let (k, r) = await g.next() {
                switch r {
                case .success(let d):
                    if (try? d.write(to: Self.file(k), options: .atomic)) != nil { idx.local[k] = idx.remote[k] }
                case .failure(let e):
                    failed.append(e.localizedDescription)
                }
                if let k2 = rest.next() { g.addTask { (k2, await Self.fetch(c, k2)) } }
            }
        }
        merge(idx)
        if !failed.isEmpty { warning = tr("有 {n} 段没取到（{e}），下拉重试", ["n": "\(failed.count)", "e": failed[0]]) }
        var text = keys.compactMap { try? String(contentsOf: Self.file($0), encoding: .utf8) }.joined(separator: "\n")
        // 换到同步服务以前（v1）从录音服务器读过、存在本机的那天
        if keys.isEmpty, let old = try? String(contentsOf: Self.root.appendingPathComponent("\(DayState.key(day)).jsonl"), encoding: .utf8) {
            text = old
        }
        return (text, warning)
    }

    private func loadIndex(user: String) -> Index {
        if let i = index, i.user == user { return i }
        if let d = try? Data(contentsOf: Self.indexURL), let i = try? JSONDecoder().decode(Index.self, from: d), i.user == user {
            index = i
            return i
        }
        try? FileManager.default.removeItem(at: Self.segDir)  // 换了账号：之前那个账号解开的不留
        let i = Index(user: user)
        index = i
        return i
    }

    /// 两天同时在取时各自拿回的合到一起再存
    private func merge(_ i: Index) {
        var cur = index?.user == i.user ? index! : i
        cur.since = max(cur.since, i.since)
        cur.remote.merge(i.remote) { max($0, $1) }
        cur.local.merge(i.local) { max($0, $1) }
        index = cur
        try? JSONEncoder().encode(cur).write(to: Self.indexURL, options: .atomic)
    }

    /// 段 id 里的开始时刻（UTC）：watch/transcript.watch-20261021T010000Z → 2026-10-21 01:00:00Z
    static func start(of key: String) -> Date? {
        guard let stamp = key.split(separator: "-").last else { return nil }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        return f.date(from: String(stamp))
    }

    private static func refreshList(_ c: Credentials, _ start: Index) async throws -> Index {
        var i = start
        while true {
            let d = try await Account.send(c.request("v1/list?prefix=transcript.&since=\(i.since)"))
            let items = (try? JSONSerialization.jsonObject(with: d) as? [String: Any])?["items"] as? [[String: Any]] ?? []
            for it in items {
                let updated = (it["updated"] as? NSNumber)?.int64Value ?? 0
                i.since = max(i.since, updated)
                // 服务器没按前缀筛时这里再筛一遍
                guard let dev = it["device"] as? String, let seg = it["segment"] as? String, seg.hasPrefix("transcript.") else { continue }
                i.remote["\(dev)/\(seg)"] = updated
            }
            if items.count < 5000 { return i }
        }
    }

    private static func fetch(_ c: Credentials, _ key: String) async -> Result<Data, Error> {
        let p = key.split(separator: "/", maxSplits: 1).map(String.init)
        guard p.count == 2 else { return .failure(ServiceError(tr("块名不对：{key}", ["key": key]))) }
        do {
            let blob = try await Account.send(c.request("v1/blob/\(p[0])/\(p[1])"))
            return .success(try SyncCrypto.open(key: c.key, device: p[0], name: p[1], blob: blob))
        } catch {
            return .failure(error)
        }
    }
}
