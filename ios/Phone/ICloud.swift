import Foundation

/// 不要 Beayy 账号的同步（同电脑 beayy-sync 的 Store）：iCloud（iCloud 容器里的同步文件夹）、WebDAV（坚果云、NAS、Nextcloud）。
/// 布局和电脑一样：<根>/vault.json、<根>/blobs/<设备>/<块名>，块的加密同 SyncCrypto。
/// 账号存在 Rust 核心那份（Beayy/sync/account）：server 是 icloud:<路径>、folder:<路径> 或 webdav:<地址>，WebDAV 的 Basic 认证在 token。
enum ICloud {
    /// 和 Rust 的 beayy_sync::ICLOUD_CONTAINER、entitlements 里登记的是同一个
    static let container = "iCloud.com.fengli.beayy"

    /// 解析出的容器路径写在这里，Rust 核心（beayy_sync::icloud_dir）读它；没开 iCloud 就没有这个文件
    static var marker: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Beayy/icloud_container")
    }

    private static let lock = NSLock()
    private static var containerURL: URL?
    private static var query: NSMetadataQuery?

    /// 启动、回到前台时调：后台线程解析容器（第一次可能要几秒，不能在主线程），写进 marker；解析到了就盯着下载
    static func start() {
        DispatchQueue.global(qos: .utility).async {
            let url = FileManager.default.url(forUbiquityContainerIdentifier: container)
            lock.withLock { containerURL = url }
            if let url {
                try? FileManager.default.createDirectory(at: marker.deletingLastPathComponent(), withIntermediateDirectories: true)
                try? Data(url.path.utf8).write(to: marker, options: .atomic)
                DispatchQueue.main.async { watch() }
            } else {
                try? FileManager.default.removeItem(at: marker)
            }
        }
    }

    /// 同步目录 <容器>/Sync（同 Rust 的 ICLOUD_SUB）；没开 iCloud 是 nil
    static var syncDir: URL? { lock.withLock { containerURL }?.appendingPathComponent("Sync", isDirectory: true) }

    /// 别的设备传来的文件在 iPhone 上先只是占位（没下载）：盯着容器里的文件，没下载的叫 iCloud 下载，Rust 下一轮同步就读得到。
    /// NSMetadataQuery 要在主线程起；之后有新文件会再通知
    private static func watch() {
        guard query == nil else { return }
        let q = NSMetadataQuery()
        q.searchScopes = [NSMetadataQueryUbiquitousDataScope]  // Sync/ 不在 Documents 里
        q.predicate = NSPredicate(format: "%K LIKE %@", NSMetadataItemFSNameKey, "*")
        for name in [Notification.Name.NSMetadataQueryDidFinishGathering, .NSMetadataQueryDidUpdate] {
            NotificationCenter.default.addObserver(forName: name, object: q, queue: .main) { _ in download(q) }
        }
        q.start()
        query = q
    }

    private static func download(_ q: NSMetadataQuery) {
        q.disableUpdates()
        defer { q.enableUpdates() }
        for case let item as NSMetadataItem in q.results {
            guard let url = item.value(forAttribute: NSMetadataItemURLKey) as? URL,
                  item.value(forAttribute: NSMetadataUbiquitousItemDownloadingStatusKey) as? String != NSMetadataUbiquitousItemDownloadingStatusCurrent
            else { continue }
            try? FileManager.default.startDownloadingUbiquitousItem(at: url)
        }
    }
}

/// 同步文件夹（手机上就是 iCloud）里的文件：经 NSFileCoordinator 读写，iCloud 才知道有新文件、读的时候才会先下载
enum BlobFile {
    static func write(_ data: Data, to dst: URL) throws {
        try FileManager.default.createDirectory(at: dst.deletingLastPathComponent(), withIntermediateDirectories: true)
        var err: NSError?
        var inner: Error?
        NSFileCoordinator().coordinate(writingItemAt: dst, options: .forReplacing, error: &err) { u in
            do { try data.write(to: u, options: .atomic) } catch { inner = error }
        }
        if let e = err ?? inner { throw e }
    }

    /// 读一个块；没有抛 404（同服务器），还没下载下来叫 iCloud 下载、这次先算没有
    static func read(_ url: URL) throws -> Data {
        let fm = FileManager.default
        if !fm.fileExists(atPath: url.path) {
            let stub = url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent).icloud")
            if fm.fileExists(atPath: stub.path) {
                try? fm.startDownloadingUbiquitousItem(at: url)
                throw ServiceError(tr("iCloud 还没下载下来，稍后再试"))
            }
            throw ServiceError(tr("没有这个文件"), status: 404)
        }
        var err: NSError?
        var out: Result<Data, Error> = .failure(ServiceError(tr("读不了")))
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &err) { u in
            out = Result { try Data(contentsOf: u) }
        }
        if let err { throw err }
        return try out.get()
    }
}

/// WebDAV：地址已经由 Rust 转成合法的（中文、空格是 %xx），认证是 Basic
enum WebDAV {
    private static let lock = NSLock()
    /// 这次启动里建过的目录（坚果云免费版限请求次数，别每传一块都 MKCOL 一遍）
    private static var made: Set<String> = []

    static func request(_ base: String, _ auth: String, _ rel: String, method: String = "GET") throws -> URLRequest {
        guard let url = URL(string: "\(base)/\(rel)") else { throw ServiceError(tr("WebDAV 地址不对")) }
        var r = URLRequest(url: url)
        r.httpMethod = method
        r.setValue(auth, forHTTPHeaderField: "Authorization")
        return r
    }

    /// 建好 blobs/<设备>/（已经有了回 405，也算成）
    static func ensureDir(_ base: String, _ auth: String, device: String) async throws {
        for rel in ["blobs/", "blobs/\(device)/"] {
            let key = "\(base)/\(rel)"
            if lock.withLock({ made.contains(key) }) { continue }
            _ = try await Account.call(request(base, auth, rel, method: "MKCOL"), service: "WebDAV", accept: [200, 201, 204, 301, 405])
            lock.withLock { _ = made.insert(key) }
        }
    }

    /// 传的时候回 409（目录被删了）：下次重建
    static func forgetDirs() { lock.withLock { made.removeAll() } }
}
