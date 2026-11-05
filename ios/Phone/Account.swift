import Foundation
import Security
import UIKit

/// 钥匙串：账号的 user、token、数据密钥、邮箱，和解开的 account/config.json。
/// 只存这台设备（不随 iCloud 钥匙串走），开机解锁过一次以后后台也能读（手表送来录音时 app 可能在锁屏下被拉起来上传）
enum Keychain {
    static let service = "com.fengli.beayy.ios.account"

    static func get(_ account: String) -> Data? {
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                                kSecAttrAccount as String: account, kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
        var out: AnyObject?
        return SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? out as? Data : nil
    }

    static func string(_ account: String) -> String? { get(account).map { String(decoding: $0, as: UTF8.self) } }

    /// 值为空＝删掉
    static func set(_ account: String, _ value: Data?) {
        let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account]
        SecItemDelete(q as CFDictionary)
        guard let value else { return }
        var add = q
        add[kSecValueData as String] = value
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(add as CFDictionary, nil)
    }
}

/// 同步服务、转写服务的错：message 是给人看的，status 是 HTTP 状态（没连上为 0）
struct ServiceError: LocalizedError {
    var message: String
    var status = 0
    var errorDescription: String? { message }
    init(_ message: String, status: Int = 0) { self.message = message; self.status = status }
}

/// 请求出错写成人话：错在哪、会不会自己好。upload 为真时说会自动重传。
/// service 是中文服务名（「同步服务」「转写服务」「WebDAV」），下面拿它比对，显示时才 tr
func netErrorText(_ error: Error?, status: Int, service: String = "同步服务", upload: Bool = false) -> String {
    let v = ["service": tr(service), "later": upload ? tr("，稍后自动重传") : tr("，稍后再试")]
    if let e = error as? URLError {
        switch e.code {
        case .notConnectedToInternet, .networkConnectionLost: return upload ? tr("没有网络，连上后自动重传") : tr("没有网络")
        case .timedOut, .cannotConnectToHost, .cannotFindHost: return tr("连不上{service}{later}", v)
        default: return tr("{service}没连上（{code}）{later}", v.merging(["code": "\(e.code.rawValue)"]) { a, _ in a })
        }
    }
    if error != nil { return tr("{service}没连上{later}", v) }
    switch status {
    case 401, 403:
        if service == "WebDAV" { return tr("WebDAV 不认用户名或密码（改过密码？）：到「我」退出再登录，不会自己好") }
        return service == "同步服务" ? tr("同步服务不认这台手机的登录（在别处移除了，或改过主密码）：到「我」退出再登录，不会自己好")
            : tr("{service}不认这台手机的票据{later}", v)
    case 400: return tr("{service}说数据格式不对，这一条不会自己好，告诉凤梨", v)
    case 413: return tr("这一块太大，{service}不收，不会自己好，告诉凤梨", v)
    default: return tr("{service}出错（{status}）{later}", v.merging(["status": "\(status)"]) { a, _ in a })
    }
}

/// 后台上传的错（录音、停留点）
func uploadErrorText(_ error: Error?, status: Int) -> String { netErrorText(error, status: status, upload: true) }

/// 登录着的账号：发请求用的一份快照
struct Credentials {
    var server: String
    var user: String
    var token: String
    var key: Data

    /// 同步方式：Beayy 服务器；同步文件夹（手机上就是 iCloud，路径每次重新解析）；WebDAV（token 是 Basic 认证）。见 ICloud.swift
    enum Kind {
        case server
        case folder(URL)
        case webdav(base: String, auth: String)
    }

    var kind: Kind {
        if server.hasPrefix("webdav:") { return .webdav(base: String(server.dropFirst("webdav:".count)), auth: "Basic \(token)") }
        if server.hasPrefix("icloud:") { return .folder(ICloud.syncDir ?? URL(fileURLWithPath: String(server.dropFirst("icloud:".count)))) }
        if server.hasPrefix("folder:") { return .folder(URL(fileURLWithPath: String(server.dropFirst("folder:".count)))) }
        return .server
    }

    /// 登录的是 Beayy 服务器（官方或自建）：只有这样才有转写、官方地图、设备登录
    var isServer: Bool {
        if case .server = kind { return true }
        return false
    }

    /// 只给服务器模式用（别的模式先看 kind）
    func request(_ path: String, method: String = "GET") -> URLRequest {
        var r = URLRequest(url: URL(string: "\(server)/\(path)")!)
        r.httpMethod = method
        r.setValue("Bearer \(user):\(token)", forHTTPHeaderField: "Authorization")
        return r
    }
}

/// account/config.json：电脑推来的转写服务地址和高德 Key
struct RemoteConfig: Codable, Equatable {
    var transcribe_server: String?
    /// 地图用官方服务（official，经同步服务中转、不用 Key）还是自己的（custom，amap_key）
    var map_mode: String?
    var amap_key: String?
    /// 地图服务：amap（高德）／osm（OpenStreetMap，手机不问高德）；旧电脑没推这一项＝amap
    var map_provider: String?
    var updated: Int64?
}

/// 同步账号（和电脑同一个：邮箱＋主密码）。令牌和数据密钥只在钥匙串，服务器地址在 UserDefaults。
/// 登录后每次启动和每小时取一次 account/config.json
final class Account: ObservableObject {
    static let shared = Account()
    static let defaultServer = "https://sync.beayy.com"
    static let defaultTranscribe = "https://audio.beayy.com"

    @Published private(set) var loggedIn = false
    @Published private(set) var email: String?
    @Published private(set) var config: RemoteConfig?
    @Published private(set) var configError: String?

    /// 后台上传的线程也要读，锁着
    private let lock = NSLock()
    private var storedCredentials: Credentials?
    private var storedConfig: RemoteConfig?
    private var configFetched: Date?
    private var timer: Timer?
    /// 官方服务的票据缓存：kind → (票据, 到期)
    private var tickets: [String: (String, Date)] = [:]

    /// 账号以 Rust 核心存的那份为准（数据目录 Beayy/sync/account，网页里登录、退出都改它）；文件变了就重读
    var credentials: Credentials? {
        reloadFromCore()
        return lock.withLock { storedCredentials }
    }

    // MARK: 和 Rust 核心共用的账号（配对码格式：beayy-sync-1:<base64url(JSON {s, u, t, k, e})>，同电脑 beayy-sync）

    static var coreAccountFile: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Beayy/sync/account")
    }
    private var coreStamp: Date?

    private static func b64url(_ d: Data) -> String {
        d.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }

    static func readCore() -> (Credentials, String?)? {
        guard let text = try? String(contentsOf: coreAccountFile, encoding: .utf8) else { return nil }
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard body.hasPrefix("beayy-sync-1:"), let raw = SyncCrypto.b64url(String(body.dropFirst("beayy-sync-1:".count))),
              let j = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
              let u = j["u"] as? String, let t = j["t"] as? String, let k = (j["k"] as? String).flatMap(SyncCrypto.b64url), k.count == 32 else { return nil }
        let s = j["s"] as? String ?? defaultServer
        // 同步文件夹、iCloud、WebDAV 的 server 不是网址，原样留着
        let server = ["folder:", "icloud:", "webdav:"].contains(where: s.hasPrefix) ? s : normalize(s)
        return (Credentials(server: server, user: u, token: t, key: k), j["e"] as? String)
    }

    static func writeCore(_ c: Credentials, email: String?) {
        var j: [String: Any] = ["s": c.server, "u": c.user, "t": c.token, "k": b64url(c.key)]
        if let email { j["e"] = email }
        guard let raw = try? JSONSerialization.data(withJSONObject: j) else { return }
        try? FileManager.default.createDirectory(at: coreAccountFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? Data(("beayy-sync-1:" + b64url(raw)).utf8).write(to: coreAccountFile, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
    }

    /// 文件改过（网页里登录、退出）就重读，并更新 loggedIn、email
    private func reloadFromCore() {
        let stamp = (try? FileManager.default.attributesOfItem(atPath: Self.coreAccountFile.path))?[.modificationDate] as? Date
        let changed: Bool = lock.withLock {
            if stamp == coreStamp { return false }
            coreStamp = stamp
            return true
        }
        guard changed else { return }
        let got = Self.readCore()
        lock.withLock { storedCredentials = got?.0 }
        DispatchQueue.main.async {
            self.loggedIn = got != nil
            self.email = got?.1
        }
        if got != nil { Uploader.shared.uploadPending() }
    }

    var server: String { UserDefaults.standard.string(forKey: "syncServer") ?? Self.defaultServer }

    /// 转写服务地址：电脑推来的，没有就用官方的
    var transcribeServer: String {
        let s = lock.withLock { storedConfig?.transcribe_server } ?? ""
        return s.isEmpty ? Self.defaultTranscribe : s.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }

    /// 高德 Key：电脑推来的；没有（没登录、电脑没配）就是空
    var amapKey: String? { lock.withLock { storedConfig?.amap_key }.flatMap { $0.isEmpty ? nil : $0 } }

    /// 地图服务：电脑选的，没推过＝高德
    var mapProvider: String { lock.withLock { storedConfig?.map_provider } ?? "amap" }

    /// 地图走官方服务：电脑选的官方；或者电脑没配、也没有编进去的 Key 时，登录着就走官方
    var amapOfficial: Bool {
        let mode = lock.withLock { storedConfig?.map_mode }
        if mode == "official" { return true }
        if mode == "custom" { return false }
        return amapKey == nil && (Bundle.main.object(forInfoDictionaryKey: "BeayyAmapKey") as? String ?? "").isEmpty
    }

    private init() {
        // 0.2 (12) 在原生界面里登录过的：迁进 Rust 核心那份（之后网页和原生共用），钥匙串里的删掉
        if Self.readCore() == nil, let user = Keychain.string("user"), let token = Keychain.string("token"), let key = Keychain.get("key"), key.count == 32 {
            Self.writeCore(Credentials(server: server, user: user, token: token, key: key), email: Keychain.string("email"))
            for k in ["user", "token", "key", "email"] { Keychain.set(k, nil) }
        }
        if let (c, e) = Self.readCore() {
            storedCredentials = c
            loggedIn = true
            email = e
            coreStamp = (try? FileManager.default.attributesOfItem(atPath: Self.coreAccountFile.path))?[.modificationDate] as? Date
        }
        storedConfig = Keychain.get("config").flatMap { try? JSONDecoder().decode(RemoteConfig.self, from: $0) }
        config = storedConfig
    }

    // MARK: 请求

    /// 发一个请求，200 才算成功，返回 body。模拟器里一律不发：模拟器里全是测试数据，不能碰真的服务器（10-19 出过一次：模拟器把假停留点传了上去）
    static func send(_ r: URLRequest, service: String = "同步服务") async throws -> Data {
        try await call(r, service: service).0
    }

    /// 同 send，另外收下 accept 里的状态（转写服务算得慢时回 202），返回 body 和状态
    static func call(_ r: URLRequest, service: String = "同步服务", accept: Set<Int> = [200]) async throws -> (Data, Int) {
        #if targetEnvironment(simulator)
        throw ServiceError(tr("模拟器不连服务器"))
        #else
        let data: Data, resp: URLResponse
        do {
            (data, resp) = try await URLSession.shared.data(for: r)
        } catch {
            throw ServiceError(netErrorText(error, status: 0, service: service))
        }
        let status = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard accept.contains(status) else {
            // 两个服务出错时都带一句中文（同步服务 {"error"}，转写服务 {"detail"}），有就用它
            let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            let said = (o?["error"] as? String) ?? (o?["detail"] as? String)
            throw ServiceError(said.map { tr("{service}：{msg}", ["service": tr(service), "msg": $0]) } ?? netErrorText(nil, status: status, service: service), status: status)
        }
        return (data, status)
        #endif
    }

    private static func json(_ d: Data) -> [String: Any] { (try? JSONSerialization.jsonObject(with: d) as? [String: Any]) ?? [:] }

    /// 取一块并解开；没有抛 status 404
    func get(device: String, name: String) async throws -> Data {
        guard let c = credentials else { throw ServiceError(tr("没登录账号")) }
        let blob: Data
        switch c.kind {
        case .server: blob = try await Self.send(c.request("v1/blob/\(device)/\(name)"))
        case .folder(let dir): blob = try BlobFile.read(dir.appendingPathComponent("blobs/\(device)/\(name)"))
        case .webdav(let base, let auth): blob = try await Self.send(WebDAV.request(base, auth, "blobs/\(device)/\(name)"), service: "WebDAV")
        }
        return try SyncCrypto.open(key: c.key, device: device, name: name, blob: blob)
    }

    /// 封好传一块（小的，前台传；录音和停留点走 Uploader 的后台上传）
    func put(device: String, name: String, plain: Data) async throws {
        guard let c = credentials else { throw ServiceError(tr("没登录账号")) }
        let sealed = try SyncCrypto.seal(key: c.key, device: device, name: name, plain: plain)
        switch c.kind {
        case .server:
            var r = c.request("v1/blob/\(device)/\(name)", method: "PUT")
            r.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            r.httpBody = sealed
            _ = try await Self.send(r)
        case .folder(let dir):
            try BlobFile.write(sealed, to: dir.appendingPathComponent("blobs/\(device)/\(name)"))
        case .webdav(let base, let auth):
            try await WebDAV.ensureDir(base, auth, device: device)
            var r = try WebDAV.request(base, auth, "blobs/\(device)/\(name)", method: "PUT")
            r.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
            r.httpBody = sealed
            _ = try await Self.call(r, service: "WebDAV", accept: [200, 201, 204])
        }
    }

    /// 同 Rust 的 NO_SERVER
    static var noServer: String { tr("用的是 iCloud 或 WebDAV，没有服务器：转写、官方地图要换成服务器同步") }

    /// 转写服务的票据（1 小时有效）：转写服务不拿同步令牌
    /// 官方服务的票据：kind＝asr（转写）或 map（地图），同步服务按那一项的开通和额度发
    func ticket(_ kind: String = "asr") async throws -> String {
        guard let c = credentials else { throw ServiceError(tr("没登录账号")) }
        guard c.isServer else { throw ServiceError(Self.noServer) }
        // 一小时有效：快到期（5 分钟内）再取新的
        if let (t, exp) = lock.withLock({ tickets[kind] }), exp.timeIntervalSinceNow > 300 { return t }
        let d = try await Self.send(c.request("v2/ticket?for=\(kind)"))
        let j = Self.json(d)
        if let t = j["ticket"] as? String, !t.isEmpty {
            let exp = (j["expires"] as? Double).map { Date(timeIntervalSince1970: $0 / 1000) } ?? Date().addingTimeInterval(3000)
            lock.withLock { tickets[kind] = (t, exp) }
            return t
        }
        let s = String(decoding: d, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !s.isEmpty, !s.hasPrefix("{") else { throw ServiceError(tr("同步服务没给转写票据")) }
        return s
    }

    // MARK: 登录、退出

    /// 地址补上 https://、去掉末尾的 /
    static func normalize(_ s: String) -> String {
        var t = s.trimmingCharacters(in: .whitespacesAndNewlines)
        while t.hasSuffix("/") { t.removeLast() }
        if t.isEmpty { return defaultServer }
        return t.contains("://") ? t : "https://" + t
    }

    /// 和电脑一样：取盐 → 派生 → 拿这台的令牌和包好的数据密钥 → 本机解开
    func login(email rawEmail: String, password: String, server rawServer: String) async throws {
        let server = Self.normalize(rawServer)
        let email = rawEmail.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !email.isEmpty, !password.isEmpty else { throw ServiceError(tr("邮箱和主密码都要填")) }
        guard let base = URL(string: server), base.host != nil else { throw ServiceError(tr("服务器地址不对")) }
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-_.~")
        let q = email.addingPercentEncoding(withAllowedCharacters: allowed) ?? email
        let pre = Self.json(try await Self.send(URLRequest(url: URL(string: "\(server)/v2/prelogin?email=\(q)")!)))
        let salt = pre["salt"] as? String ?? ""
        // Argon2id 要 64MB、几百毫秒，别在主线程
        let (auth, wrap) = try await Task.detached(priority: .userInitiated) { try SyncCrypto.derive(password: password, saltHex: salt) }.value
        var r = URLRequest(url: URL(string: "\(server)/v2/login")!)
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let name = await MainActor.run { UIDevice.current.name }
        r.httpBody = try JSONSerialization.data(withJSONObject: ["email": email, "auth": auth, "device": Config.device, "name": name])
        let v = Self.json(try await Self.send(r))
        guard let user = v["user"] as? String, let token = v["token"] as? String else { throw ServiceError(tr("同步服务的登录结果缺账号或令牌")) }
        let key = try SyncCrypto.unwrapKey(v["wrapped_key"] as? String ?? "", wrap: wrap)
        Keychain.set("user", Data(user.utf8))
        Keychain.set("token", Data(token.utf8))
        Keychain.set("key", key)
        Keychain.set("email", Data(email.utf8))
        UserDefaults.standard.set(server, forKey: "syncServer")
        lock.withLock { storedCredentials = Credentials(server: server, user: user, token: token, key: key) }
        await MainActor.run {
            self.loggedIn = true
            self.email = email
            Talk.shared.reset()  // 没登录时记下的「没有对话」不算数
        }
        Uploader.shared.uploadPending()
        await refreshConfig(force: true)
    }

    /// 退出：服务器上注销这台的令牌，本机忘掉账号和解开的对话；没传的录音、停留点留在手机上，重新登录后接着传
    func logout() async {
        if let c = credentials, c.isServer { _ = try? await Self.send(c.request("v2/sessions/self", method: "DELETE")) }
        for k in ["user", "token", "key", "email", "config"] { Keychain.set(k, nil) }
        lock.withLock {
            storedCredentials = nil
            storedConfig = nil
        }
        configFetched = nil
        Uploader.shared.cancelAll()
        await TranscriptStore.shared.reset()
        await MainActor.run {
            self.loggedIn = false
            self.email = nil
            self.config = nil
            self.configError = nil
            Talk.shared.reset()
        }
    }

    // MARK: 配置

    /// 启动时调一次：先取一次配置，之后每小时一次
    func startConfigTimer() {
        Task { await refreshConfig(force: true) }
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 3600, repeats: true) { [weak self] _ in Task { await self?.refreshConfig(force: true) } }
    }

    /// 取 account/config.json（不强制时一小时内只取一次）。电脑还没推（404）就用默认的：转写服务用官方的、高德 Key 用编进去的
    func refreshConfig(force: Bool = false) async {
        guard credentials != nil else { return }
        if !force, let t = configFetched, Date().timeIntervalSince(t) < 3600 { return }
        configFetched = Date()
        do {
            let plain = try await get(device: "account", name: "config.json")
            let cfg = try JSONDecoder().decode(RemoteConfig.self, from: plain)
            Keychain.set("config", plain)
            lock.withLock { storedConfig = cfg }
            await MainActor.run {
                self.config = cfg
                self.configError = nil
            }
        } catch let e as ServiceError where e.status == 404 {
            Keychain.set("config", nil)
            lock.withLock { storedConfig = nil }
            await MainActor.run {
                self.config = nil
                self.configError = nil
            }
        } catch {
            let msg = error is DecodingError ? tr("电脑推来的配置看不懂，告诉凤梨") : error.localizedDescription
            await MainActor.run { self.configError = msg }
        }
    }

    /// 本人声纹：传过就返回传的时刻
    func voiceprintUpdated() async throws -> Date? {
        do {
            let plain = try await get(device: "account", name: "voiceprint.json")
            let ms = Self.json(plain)["updated"] as? Double ?? 0
            return Date(timeIntervalSince1970: ms / 1000)
        } catch let e as ServiceError where e.status == 404 {
            return nil
        }
    }
}
