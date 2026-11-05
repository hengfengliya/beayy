import Foundation
import WatchConnectivity

enum Config {
    /// 这台 iPhone 的设备名：xcconfig 的 BEAYY_DEVICE_ID 有值就用它（和桌面版 devices/ 同名，老手机沿用），
    /// 没有就第一次启动时生成一个 iphone-<8 位十六进制> 存下来。停留点、这台录的音都归在它名下
    static let device: String = {
        let d = Bundle.main.object(forInfoDictionaryKey: "BeayyDeviceID") as? String ?? ""
        if !d.isEmpty { return d }
        if let s = UserDefaults.standard.string(forKey: "deviceID") { return s }
        let s = "iphone-" + String(format: "%08X", UInt32.random(in: .min ... .max))
        UserDefaults.standard.set(s, forKey: "deviceID")
        return s
    }()

    /// 配对的手表的设备名：手表送来的段只写 watch，在这台手机上第一次用时生成 watch-<8 位小写十六进制> 存下来
    static let watchDevice: String = {
        if let s = UserDefaults.standard.string(forKey: "watchDeviceID") { return s }
        let s = "watch-" + String(format: "%08x", UInt32.random(in: .min ... .max))
        UserDefaults.standard.set(s, forKey: "watchDeviceID")
        return s
    }()

    /// 段上写的设备（watch、iphone-…）→ 同步服务上的设备名
    static func uploadDevice(_ d: String) -> String {
        if d.contains("-") { return d }
        return d == "watch" ? watchDevice : device
    }

    /// 等着上传的段：Application Support/pending/<id>.m4a + <id>.json
    static var pendingDir: URL { appSupport("pending") }

    /// 封好（压缩、加密）等着后台上传的块：Application Support/outbox/，传完就删
    static var outboxDir: URL { appSupport("outbox") }

    private static func appSupport(_ name: String) -> URL {
        let d = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent(name, isDirectory: true)
        try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }
}

/// 手表 → iPhone：收下手表发来的段，放进 pending/，交给 Uploader。
/// 系统会在后台拉起 app 来送文件，所以 WCSession 要在启动时就激活（AppDelegate 里）。
final class Receiver: NSObject, WCSessionDelegate {
    static let shared = Receiver()

    func activate() {
        guard WCSession.isSupported() else { return }
        WCSession.default.delegate = self
        WCSession.default.activate()
    }

    func session(_ session: WCSession, didReceive file: WCSessionFile) {
        // 回调返回后系统就删这个临时文件，必须在这里同步挪走
        guard let meta = file.metadata, let seg = Segment(metadata: meta) else { return }
        let dir = Config.pendingDir
        let audio = dir.appendingPathComponent("\(seg.id).m4a")
        try? FileManager.default.removeItem(at: audio)
        do {
            try FileManager.default.moveItem(at: file.fileURL, to: audio)
            try JSONEncoder().encode(seg).write(to: dir.appendingPathComponent("\(seg.id).json"), options: .atomic)
        } catch {
            return
        }
        Uploader.shared.uploadPending()
    }

    /// 手表记的停留点：transferUserInfo 送来 {"visit": JSON}
    func session(_ session: WCSession, didReceiveUserInfo info: [String: Any]) {
        guard let data = info["visit"] as? Data, let v = try? JSONDecoder().decode(Visit.self, from: data) else { return }
        DispatchQueue.main.async { Places.shared.ingest(v) }
    }

    /// 手表的录音状态（applicationContext）：在不在录、最近录完一段的时刻
    func session(_ session: WCSession, didReceiveApplicationContext ctx: [String: Any]) {
        DispatchQueue.main.async { WatchState.shared.update(ctx) }
    }

    func session(_ session: WCSession, activationDidCompleteWith state: WCSessionActivationState, error: Error?) {}
    func sessionDidBecomeInactive(_ session: WCSession) {}
    func sessionDidDeactivate(_ session: WCSession) { WCSession.default.activate() }  // 换了配对的手表
}

/// 这台 iPhone 自己录的一段：和手表送来的一样放进 pending/，交给 Uploader
func deliverOwnSegment(_ url: URL, _ seg: Segment) {
    let dir = Config.pendingDir
    let audio = dir.appendingPathComponent("\(seg.id).m4a")
    try? FileManager.default.removeItem(at: audio)
    do {
        try FileManager.default.moveItem(at: url, to: audio)
        try JSONEncoder().encode(seg).write(to: dir.appendingPathComponent("\(seg.id).json"), options: .atomic)
    } catch {
        return
    }
    Uploader.shared.uploadPending()
}

/// 手表那边的状态（手表经 applicationContext 发来）：首页上写「手表在录」
final class WatchState: ObservableObject {
    static let shared = WatchState()
    @Published private(set) var recording = false
    @Published private(set) var updated: Date?

    func update(_ ctx: [String: Any]) {
        recording = ctx["recording"] as? Bool ?? false
        updated = (ctx["at"] as? Double).map(Date.init(timeIntervalSince1970:))
    }

    func load() {
        guard WCSession.isSupported(), WCSession.default.activationState == .activated else { return }
        update(WCSession.default.receivedApplicationContext)
    }
}

/// iPhone → 同步服务：录音一段一块（audio.<段 id>），停留点、苹果健康一天一块（places.<日>、health.<日>），都封好再用后台 URLSession 传，
/// app 被挂起也会传完；收到 200 才删本地文件（停留点是标记这一版已传），失败的下次再传。没登录不传，留在本地排队。
/// 不要账号的同步（ICloud.swift）：iCloud 直接写进容器里的同步文件夹，WebDAV 同样后台 PUT 到 <地址>/blobs/<设备>/<块名>；
/// 录音只传 Beayy 服务器（转写要它发的票据），别的方式下录音留在手机上排队
final class Uploader: NSObject, ObservableObject, URLSessionTaskDelegate {
    static let shared = Uploader()
    static let sessionID = "com.fengli.beayy.ios.upload"

    /// 等着传的录音段
    @Published private(set) var pending = 0
    /// 停留点有改动、还没传的天数
    @Published private(set) var pendingPlaceDays = 0
    @Published private(set) var lastUpload: Date? = UserDefaults.standard.object(forKey: "lastUpload") as? Date
    @Published private(set) var lastError: String?
    var backgroundCompletion: (() -> Void)?

    /// 已经建了任务、还没回来的（getAllTasks 是异步的，连着调两次会重复建）
    private let lock = NSLock()
    private var queued: Set<String> = []

    private lazy var session: URLSession = {
        let c = URLSessionConfiguration.background(withIdentifier: Self.sessionID)
        c.sessionSendsLaunchEvents = true
        c.isDiscretionary = false
        return URLSession(configuration: c, delegate: self, delegateQueue: nil)
    }()

    func uploadPending() {
        refresh()
        #if !targetEnvironment(simulator)  // 模拟器绝不上传（见 Account.send）
        guard let acct = Account.shared.credentials else { return }
        session.getAllTasks { tasks in
            // 旧版（传 hk2 录音服务器）留在后台会话里的任务不要了
            for t in tasks where !(t.taskDescription.map { $0.hasPrefix("audio:") || $0.hasPrefix("places:") || $0.hasPrefix("health:") } ?? false) { t.cancel() }
            let inflight = Set(tasks.compactMap(\.taskDescription))
            if acct.isServer { self.queueAudio(acct, skip: inflight) }
            self.queuePlaces(acct, skip: inflight)
            self.queueHealth(acct, skip: inflight)
            self.refresh()
        }
        #endif
    }

    /// 退出登录：停掉在传的（本地文件都留着，重新登录后再传）
    func cancelAll() {
        session.getAllTasks { $0.forEach { $0.cancel() } }
    }

    private func claim(_ desc: String) -> Bool { lock.withLock { queued.insert(desc).inserted } }
    private func release(_ desc: String) { lock.withLock { _ = queued.remove(desc) } }

    private func start(_ acct: Credentials, device: String, name: String, file: URL, desc: String) {
        switch acct.kind {
        case .server:
            var r = acct.request("v1/blob/\(device)/\(name)", method: "PUT")
            r.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            let task = session.uploadTask(with: r, fromFile: file)
            task.taskDescription = desc
            task.resume()
        case .folder(let dir):
            // iCloud：写进容器里的同步文件夹就算传完，iCloud 自己同步到电脑
            DispatchQueue.global(qos: .utility).async {
                do {
                    try BlobFile.write(Data(contentsOf: file), to: dir.appendingPathComponent("blobs/\(device)/\(name)"))
                    self.finish(desc, ok: true, error: nil)
                } catch {
                    self.finish(desc, ok: false, error: "写不进 iCloud（\(error.localizedDescription)），稍后自动重传")
                }
            }
        case .webdav(let base, let auth):
            Task {
                do {
                    try await WebDAV.ensureDir(base, auth, device: device)
                    var r = try WebDAV.request(base, auth, "blobs/\(device)/\(name)", method: "PUT")
                    r.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
                    let task = self.session.uploadTask(with: r, fromFile: file)
                    task.taskDescription = desc
                    task.resume()
                } catch {
                    self.finish(desc, ok: false, error: "\(error.localizedDescription)，稍后自动重传")
                }
            }
        }
    }

    /// 录音：明文＝一行元数据 JSON＋换行＋m4a 原始字节
    private func queueAudio(_ acct: Credentials, skip: Set<String>) {
        let files = (try? FileManager.default.contentsOfDirectory(at: Config.pendingDir, includingPropertiesForKeys: nil)) ?? []
        for sidecar in files where sidecar.pathExtension == "json" {
            let audio = sidecar.deletingPathExtension().appendingPathExtension("m4a")
            guard let data = try? Data(contentsOf: sidecar),
                  let seg = try? JSONDecoder().decode(Segment.self, from: data),
                  FileManager.default.fileExists(atPath: audio.path) else { continue }
            let desc = "audio:\(seg.id)"
            guard !skip.contains(desc), claim(desc) else { continue }
            let device = Config.uploadDevice(seg.device), name = "audio.\(seg.id)"
            let out = Config.outboxDir.appendingPathComponent("\(name).blob")
            do {
                let meta: [String: Any] = ["id": seg.id, "start": seg.start.iso8601UTC, "end": seg.end.iso8601UTC, "device": device,
                                           "battery": seg.battery.map { ($0 * 100).rounded() / 100 } ?? NSNull()]
                var plain = try JSONSerialization.data(withJSONObject: meta, options: [.sortedKeys])
                plain.append(0x0a)
                plain.append(try Data(contentsOf: audio))
                try SyncCrypto.seal(key: acct.key, device: device, name: name, plain: plain).write(to: out, options: .atomic)
            } catch {
                release(desc)
                DispatchQueue.main.async { self.lastError = "录音：\(seg.id) 封装没成功（\(error.localizedDescription)），先留在手机上，告诉凤梨" }
                continue
            }
            start(acct, device: device, name: name, file: out, desc: desc)
        }
    }

    /// 停留点：有改动的那几天，各传一整天（这天每个停留点一行，最新一版）
    private func queuePlaces(_ acct: Credentials, skip: Set<String>) {
        for day in Set(Places.shared.pendingVisits().compactMap(\.dayKey)) {
            let desc = "places:\(day)"
            guard !skip.contains(desc), claim(desc) else { continue }
            let name = "places.\(day)"
            let out = Config.outboxDir.appendingPathComponent("\(name).blob")
            let revs = Config.outboxDir.appendingPathComponent("\(name).revs")
            do {
                let snap = Places.shared.daySnapshot(day, device: Config.device)
                try JSONEncoder().encode(snap.revs).write(to: revs, options: .atomic)
                try SyncCrypto.seal(key: acct.key, device: Config.device, name: name, plain: snap.rows).write(to: out, options: .atomic)
            } catch {
                release(desc)
                DispatchQueue.main.async { self.lastError = "位置：\(day) 封装没成功（\(error.localizedDescription)），先留在手机上，告诉凤梨" }
                continue
            }
            start(acct, device: Config.device, name: name, file: out, desc: desc)
        }
    }

    /// 苹果健康：有改动的那几天，各传一整天（补历史时一次排 30 天，传完一天接着排）
    private func queueHealth(_ acct: Credentials, skip: Set<String>) {
        for (day, seq) in Health.shared.pendingUploads(30) {
            let desc = "health:\(day)"
            guard !skip.contains(desc), claim(desc) else { continue }
            let name = "health.\(day)"
            let out = Config.outboxDir.appendingPathComponent("\(name).blob")
            do {
                try String(seq).write(to: Config.outboxDir.appendingPathComponent("\(name).seq"), atomically: true, encoding: .utf8)
                try SyncCrypto.seal(key: acct.key, device: Config.device, name: name, plain: Health.shared.dayRows(day)).write(to: out, options: .atomic)
            } catch {
                release(desc)
                DispatchQueue.main.async { self.lastError = "健康：\(day) 封装没成功（\(error.localizedDescription)），先留在手机上，告诉凤梨" }
                continue
            }
            start(acct, device: Config.device, name: name, file: out, desc: desc)
        }
    }

    func refresh() {
        let n = ((try? FileManager.default.contentsOfDirectory(at: Config.pendingDir, includingPropertiesForKeys: nil)) ?? [])
            .filter { $0.pathExtension == "json" }.count
        let days = Set(Places.shared.pendingVisits().compactMap(\.dayKey)).count
        DispatchQueue.main.async {
            self.pending = n
            self.pendingPlaceDays = days
        }
    }

    private func succeeded() {
        let now = Date()
        UserDefaults.standard.set(now, forKey: "lastUpload")
        DispatchQueue.main.async { self.lastUpload = now; self.lastError = nil }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let desc = task.taskDescription else { return }
        let status = (task.response as? HTTPURLResponse)?.statusCode ?? 0
        let ok = error == nil && (200..<300).contains(status)  // WebDAV 新建回 201、覆盖回 204
        let cancelled = (error as? URLError)?.code == .cancelled  // 退出登录，或旧版的任务
        let webdav = Account.shared.credentials?.isServer == false
        if webdav && status == 409 { WebDAV.forgetDirs() }  // 目录被删了：下次重建
        finish(desc, ok: ok, error: ok || cancelled ? nil : netErrorText(error, status: status, service: webdav ? "WebDAV" : "同步服务", upload: true))
    }

    /// 一块传完：成功才删本地文件、记下传过的版本；失败的留着下次再传，error 是给人看的（取消的没有）
    private func finish(_ desc: String, ok: Bool, error msg: String?) {
        release(desc)
        let fm = FileManager.default
        if desc.hasPrefix("audio:") {
            let id = String(desc.dropFirst("audio:".count))
            try? fm.removeItem(at: Config.outboxDir.appendingPathComponent("audio.\(id).blob"))
            if ok {
                try? fm.removeItem(at: Config.pendingDir.appendingPathComponent("\(id).m4a"))
                try? fm.removeItem(at: Config.pendingDir.appendingPathComponent("\(id).json"))
                succeeded()
            } else if let msg {
                DispatchQueue.main.async { self.lastError = "录音：\(msg)" }
            }
        } else if desc.hasPrefix("places:") {
            let day = String(desc.dropFirst("places:".count))
            let revsURL = Config.outboxDir.appendingPathComponent("places.\(day).revs")
            let revs = (try? Data(contentsOf: revsURL)).flatMap { try? JSONDecoder().decode([String: Int].self, from: $0) } ?? [:]
            try? fm.removeItem(at: Config.outboxDir.appendingPathComponent("places.\(day).blob"))
            try? fm.removeItem(at: revsURL)
            if ok {
                for (id, rev) in revs { Places.shared.uploaded(id: id, rev: rev) }
                succeeded()
                // 传的时候这天又有了新的一版：接着传
                if Places.shared.pendingVisits().contains(where: { $0.dayKey == day }) { uploadPending() }
            } else if let msg {
                DispatchQueue.main.async { self.lastError = "位置：\(msg)" }
            }
        } else if desc.hasPrefix("health:") {
            let day = String(desc.dropFirst("health:".count))
            let seqURL = Config.outboxDir.appendingPathComponent("health.\(day).seq")
            let seq = (try? String(contentsOf: seqURL, encoding: .utf8)).flatMap { Int($0) }
            try? fm.removeItem(at: Config.outboxDir.appendingPathComponent("health.\(day).blob"))
            try? fm.removeItem(at: seqURL)
            if ok {
                if let seq { Health.shared.uploaded(day, seq: seq) }
                succeeded()
                if !Health.shared.pendingUploads(1).isEmpty { uploadPending() }
            } else if let msg {
                DispatchQueue.main.async { self.lastError = "健康：\(msg)" }
            }
        }
        refresh()
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        DispatchQueue.main.async {
            self.backgroundCompletion?()
            self.backgroundCompletion = nil
        }
    }
}
