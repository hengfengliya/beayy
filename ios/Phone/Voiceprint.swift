import AVFoundation
import SwiftUI

/// 录一段本人朗读（30 秒左右），转写服务算出声纹，存到账号里，转写时用它认出哪些话是自己说的。换了麦克风环境效果不好时可以重录。
final class VoiceprintRecorder: NSObject, ObservableObject {
    enum Phase: Equatable { case idle, recording, uploading, done, failed(String) }

    @Published var phase: Phase = .idle
    @Published var elapsed: TimeInterval = 0
    let length: TimeInterval = 30

    private var recorder: AVAudioRecorder?
    private var timer: Timer?
    private let url = FileManager.default.temporaryDirectory.appendingPathComponent("voiceprint.m4a")

    func start() {
        AVAudioApplication.requestRecordPermission { granted in
            DispatchQueue.main.async {
                guard granted else { self.phase = .failed(tr("没有麦克风权限，到「设置 › Beayy」打开")); return }
                do {
                    let s = AVAudioSession.sharedInstance()
                    try s.setCategory(.record, mode: .measurement)
                    try s.setActive(true)
                    let settings: [String: Any] = [AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 16000,
                                                   AVNumberOfChannelsKey: 1, AVEncoderBitRateKey: 48000]
                    self.recorder = try AVAudioRecorder(url: self.url, settings: settings)
                    self.recorder?.record(forDuration: self.length)
                    self.phase = .recording
                    self.elapsed = 0
                    self.timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
                        guard let self, let r = self.recorder else { return }
                        if r.isRecording { self.elapsed = r.currentTime } else { self.finish() }
                    }
                } catch {
                    self.phase = .failed(tr("录音失败：{e}", ["e": error.localizedDescription]))
                }
            }
        }
    }

    func finish() {
        timer?.invalidate()
        recorder?.stop()
        recorder = nil
        try? AVAudioSession.sharedInstance().setActive(false)
        phase = .uploading
        Task {
            do {
                try await Self.upload(url)
                await MainActor.run { self.phase = .done }
            } catch {
                await MainActor.run { self.phase = .failed(error.localizedDescription) }
            }
        }
    }

    /// 转写服务算声纹（取票据 → POST /v2/voiceprint，服务不留音频和声纹）→ 加密传到同步服务 account/voiceprint.json，电脑转写时带上。
    /// 50 秒内算完直接给结果；没算完回 202 和任务号，每 3 秒问一次 GET /v2/jobs/<任务>，取走后 DELETE。
    /// 转写服务在 Cloudflare 后面，单个请求不能超过 100 秒
    private static func upload(_ url: URL) async throws {
        let audio = try Data(contentsOf: url)
        let ticket = try await Account.shared.ticket()
        let base = Account.shared.transcribeServer
        func request(_ path: String, method: String = "GET") -> URLRequest {
            var r = URLRequest(url: URL(string: "\(base)/\(path)")!)
            r.httpMethod = method
            r.timeoutInterval = 90
            r.setValue("Ticket \(ticket)", forHTTPHeaderField: "Authorization")
            return r
        }
        func object(_ d: Data) -> [String: Any] { (try? JSONSerialization.jsonObject(with: d) as? [String: Any]) ?? [:] }

        var r = request("v2/voiceprint", method: "POST")
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try JSONSerialization.data(withJSONObject: ["audio": audio.base64EncodedString()])
        let (d, status) = try await Account.call(r, service: "转写服务", accept: [200, 202])
        var result = object(d)
        if status == 202 {
            guard let job = result["job"] as? String, !job.isEmpty else { throw ServiceError(tr("转写服务没给任务号，重录一次试试")) }
            let deadline = Date().addingTimeInterval(10 * 60)
            while true {
                guard Date() < deadline else { throw ServiceError(tr("转写服务 10 分钟还没算完，可能在排队转写，过一会儿重录")) }
                try await Task.sleep(nanoseconds: 3_000_000_000)
                let o = object(try await Account.send(request("v2/jobs/\(job)"), service: "转写服务"))
                let s = o["status"] as? String
                if s == "failed" { throw ServiceError(tr("转写服务：{msg}", ["msg": o["error"] as? String ?? tr("没算出声纹")])) }
                if s == "done" {
                    result = o["result"] as? [String: Any] ?? [:]
                    _ = try? await Account.send(request("v2/jobs/\(job)", method: "DELETE"), service: "转写服务")
                    break
                }
            }
        }
        guard let embedding = result["embedding"] as? String, !embedding.isEmpty else {
            throw ServiceError(tr("转写服务没算出声纹，重录一次试试"))
        }
        let speech = (result["speech_sec"] as? NSNumber)?.doubleValue ?? 0
        let plain = try JSONSerialization.data(withJSONObject: ["v": 1, "embedding": embedding, "speech_sec": speech,
                                                                "updated": Int64(Date().timeIntervalSince1970 * 1000)], options: [.sortedKeys])
        try await Account.shared.put(device: "account", name: "voiceprint.json", plain: plain)
    }
}

struct VoiceprintView: View {
    @StateObject private var rec = VoiceprintRecorder()
    @Environment(\.dismiss) private var dismiss

    private let script = """
    我每天早上起床以后，先喝一杯温水，然后看一眼今天要做的三件事。上午一般处理沟通和会议，下午留给需要专心的工作，比如写方案、做产品、看数据。晚上九点以后不再工作，留出时间读书、散步和休息。周末会把这一周做过的事情回顾一遍，想清楚下周最重要的是什么。
    """

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 20) {
                Text(tr("用平时说话的音量，把下面这段念完（约 30 秒）。安静的地方效果最好。"))
                    .foregroundStyle(.secondary)
                Text(tr(script)).font(.title3).lineSpacing(6)
                Spacer()
                switch rec.phase {
                case .idle:
                    Button(tr("开始录")) { rec.start() }.buttonStyle(.borderedProminent).tint(.primary)
                case .recording:
                    HStack {
                        ProgressView(value: min(rec.elapsed, rec.length), total: rec.length)
                        Button(tr("念完了")) { rec.finish() }.buttonStyle(.bordered)
                    }
                case .uploading:
                    ProgressView(tr("上传中…"))
                case .done:
                    Text(tr("已存到账号，电脑下一轮转写（10 分钟内）生效。")).foregroundStyle(.green)
                    Button(tr("完成")) { dismiss() }.buttonStyle(.borderedProminent).tint(.primary)
                case .failed(let m):
                    Text(m).foregroundStyle(.red)
                    Button(tr("重录")) { rec.start() }.buttonStyle(.bordered)
                }
            }
            .padding()
            .navigationTitle(tr("录声纹"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button(tr("取消")) { dismiss() } } }
        }
    }
}
