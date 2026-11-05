import CryptoKit
import Foundation
import Sodium

/// 同步服务的加密，和电脑（app/crates/beayy-sync/src/lib.rs 的 derive、unwrap_key、seal、open）逐字节一致，
/// 约定见 报告/方案_20261021_录音与存储.md。对照测试向量的测试在 Tests/SyncCryptoTests.swift。
/// - 主密码 → Argon2id（m=64MB、t=3、p=1，盐 16 字节）→ 主钥 → HKDF-SHA256（无盐）：info「beayy auth」是登录校验值、「beayy wrap」是包数据密钥的钥匙
/// - 数据密钥由主密码那把包着存在服务器：base64url 无填充（nonce 24‖密文），AAD「beayy-data-key」
/// - 一块数据：明文 raw deflate（无 zlib 头）→ XChaCha20-Poly1305，块＝nonce 24‖密文，AAD＝「<设备>/<块名>」
enum SyncCrypto {
    struct Failure: LocalizedError {
        var errorDescription: String?
        init(_ s: String) { errorDescription = s }
    }

    private static let sodium = Sodium()

    /// 主密码＋盐（hex）→ (登录校验值 hex, 包钥)
    static func derive(password: String, saltHex: String) throws -> (auth: String, wrap: Data) {
        guard let salt = unhex(saltHex), salt.count == sodium.pwHash.SaltBytes else { throw Failure(tr("服务器给的盐不对")) }
        guard let master = sodium.pwHash.hash(outputLength: 32, passwd: Array(password.utf8), salt: Array(salt),
                                              opsLimit: 3, memLimit: 64 * 1024 * 1024, alg: .Argon2ID13)
        else { throw Failure(tr("主密码派生失败（内存不够？）")) }
        let ikm = SymmetricKey(data: master)
        let auth = HKDF<SHA256>.deriveKey(inputKeyMaterial: ikm, info: Data("beayy auth".utf8), outputByteCount: 32)
        let wrap = HKDF<SHA256>.deriveKey(inputKeyMaterial: ikm, info: Data("beayy wrap".utf8), outputByteCount: 32)
        return (hex(auth.withUnsafeBytes { Data($0) }), wrap.withUnsafeBytes { Data($0) })
    }

    /// 解开服务器存的数据密钥
    static func unwrapKey(_ wrapped: String, wrap: Data) throws -> Data {
        guard let b = b64url(wrapped), b.count > 24 else { throw Failure(tr("服务器给的数据密钥坏了")) }
        guard let key = sodium.aead.xchacha20poly1305ietf.decrypt(nonceAndAuthenticatedCipherText: Array(b), secretKey: Array(wrap),
                                                                   additionalData: Array("beayy-data-key".utf8))
        else { throw Failure(tr("主密码不对，解不开数据密钥")) }
        guard key.count == 32 else { throw Failure(tr("数据密钥长度不对")) }
        return Data(key)
    }

    static func seal(key: Data, device: String, name: String, plain: Data) throws -> Data {
        let packed = try deflate(plain)
        guard let out: Bytes = sodium.aead.xchacha20poly1305ietf.encrypt(message: Array(packed), secretKey: Array(key),
                                                                         additionalData: Array("\(device)/\(name)".utf8))
        else { throw Failure(tr("加密失败")) }
        return Data(out)
    }

    static func open(key: Data, device: String, name: String, blob: Data) throws -> Data {
        guard blob.count > 24 else { throw Failure(tr("块太短")) }
        guard let packed = sodium.aead.xchacha20poly1305ietf.decrypt(nonceAndAuthenticatedCipherText: Array(blob), secretKey: Array(key),
                                                                     additionalData: Array("\(device)/\(name)".utf8))
        else { throw Failure(tr("解密失败：密钥不对，或块被改过")) }
        return try inflate(Data(packed))
    }

    // MARK: 压缩（NSData 的 zlib 就是不带头的 raw deflate，同电脑的 miniz_oxide）

    static func deflate(_ d: Data) throws -> Data {
        do { return try (d as NSData).compressed(using: .zlib) as Data } catch { throw Failure(tr("压缩失败")) }
    }

    static func inflate(_ d: Data) throws -> Data {
        if d.isEmpty { return Data() }
        do { return try (d as NSData).decompressed(using: .zlib) as Data } catch { throw Failure(tr("解压失败")) }
    }

    // MARK: 编码

    static func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }

    static func unhex(_ s: String) -> Data? {
        let c = Array(s.utf8)
        guard c.count % 2 == 0 else { return nil }
        var out = Data(capacity: c.count / 2)
        for i in stride(from: 0, to: c.count, by: 2) {
            guard let b = UInt8(String(decoding: c[i..<i + 2], as: UTF8.self), radix: 16) else { return nil }
            out.append(b)
        }
        return out
    }

    /// base64url 无填充 → 字节
    static func b64url(_ s: String) -> Data? {
        var t = s.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while t.count % 4 != 0 { t += "=" }
        return Data(base64Encoded: t)
    }
}
