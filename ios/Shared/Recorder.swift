import AVFoundation
#if os(watchOS)
import WatchKit
#else
import UIKit
#endif

/// 全天录音（手表和 iPhone 共用）：AVAudioEngine 一直开着麦克风，每 10 分钟换一个 m4a 文件（中间不停麦，不丢声音），
/// 换下来的文件交给 deliver：手表交给 Transfer 送到 iPhone，iPhone 直接放进待上传。
///
/// 后台：Info.plist 开了 UIBackgroundModes=audio，录音只要是在前台开始的，抬腕放下（锁屏）后会继续录。
/// 被电话、Siri 打断后系统会停麦；打断结束能恢复就自动恢复，恢复不了就等下次打开 app 时接着录（wantsRecording）。
final class Recorder: ObservableObject {
    static let shared = Recorder()

    @Published private(set) var isRecording = false {
        didSet { if oldValue != isRecording { onRecordingChange?(isRecording) } }
    }
    @Published private(set) var since: Date?
    @Published private(set) var segmentsDone = 0
    @Published var message: String?

    /// 录完一段交给谁（App 启动时设好）
    var deliver: ((URL, Segment) -> Void)?
    /// 每段的设备名：分段 id 的前缀（watch-…／iphone-…），和段元数据里的 device（手表的 watch 在 iPhone 上传时换成 watch-<编号>）
    var idPrefix = "watch"
    var device = "watch"
    /// 开始、停止录音时调（手表借它开关取位置点、告诉 iPhone）
    var onRecordingChange: ((Bool) -> Void)?
    /// 每换一段调一次（手表借它顺手取一个位置点）
    var onSegment: (() -> Void)?

    let segmentLength: TimeInterval = 600
    let stopBattery: Float = 0.05  // 低于 5% 先停，免得没电关机时正在写的那段坏掉

    private let engine = AVAudioEngine()
    private let q = DispatchQueue(label: "beayy.recorder")
    private var file: AVAudioFile?                 // 只在 q 上碰
    private var current: (url: URL, start: Date)?  // 只在 q 上碰
    private var observers: [NSObjectProtocol] = []

    static var dir: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("segments", isDirectory: true)
    }

    /// 用户想不想录：点了开始就是 true，点停止才 false。被打断、app 被杀都不改它，下次打开接着录。
    private(set) var wantsRecording: Bool {
        get { UserDefaults.standard.bool(forKey: "wantsRecording") }
        set { UserDefaults.standard.set(newValue, forKey: "wantsRecording") }
    }

    private init() {
        #if os(watchOS)
        WKInterfaceDevice.current().isBatteryMonitoringEnabled = true
        #else
        UIDevice.current.isBatteryMonitoringEnabled = true
        #endif
        let nc = NotificationCenter.default
        observers.append(nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            self?.handleInterruption(n)
        })
        observers.append(nc.addObserver(forName: AVAudioSession.mediaServicesWereResetNotification, object: nil, queue: .main) { [weak self] _ in
            self?.restart()
        })
        observers.append(nc.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) { [weak self] _ in
            self?.restart()
        })
    }

    // MARK: 开始 / 停止

    func start() {
        guard !isRecording else { return }
        AVAudioApplication.requestRecordPermission { granted in
            DispatchQueue.main.async {
                guard granted else {
                    #if os(watchOS)
                    self.message = tr("没有麦克风权限：到手表「设置 › 隐私与安全性 › 麦克风」打开 Beayy")
                    #else
                    self.message = tr("没有麦克风权限：到「设置 › Beayy」打开麦克风")
                    #endif
                    return
                }
                do {
                    try self.startEngine()
                    self.wantsRecording = true
                    self.isRecording = true
                    self.since = self.since ?? Date()
                    self.message = nil
                } catch {
                    self.message = tr("开始录音失败：{e}", ["e": error.localizedDescription])
                }
            }
        }
    }

    func stop() {
        wantsRecording = false
        halt()
        since = nil
    }

    /// 打开 app 时调用：上次是在录的（被打断或 app 被系统杀了），接着录。
    func resumeIfWanted() {
        if wantsRecording && !isRecording { start() }
    }

    private func startEngine() throws {
        let session = AVAudioSession.sharedInstance()
        #if os(watchOS)
        try session.setCategory(.record, mode: .default)
        #else
        // iPhone 上别把正在放的音乐、视频停掉：和别的声音混着录；耳机照常放（A2DP），不切到蓝牙耳机的通话麦克风
        try session.setCategory(.playAndRecord, mode: .default, options: [.mixWithOthers, .defaultToSpeaker, .allowBluetoothA2DP])
        #endif
        try session.setActive(true)
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw NSError(domain: "Beayy", code: 1, userInfo: [NSLocalizedDescriptionKey: tr("麦克风没有输入")])
        }
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 8192, format: format) { [weak self] buffer, _ in
            self?.q.async { self?.write(buffer) }
        }
        engine.prepare()
        try engine.start()
    }

    /// 停麦并收尾当前这一段，但不改 wantsRecording。
    private func halt() {
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        q.async { self.closeSegment() }
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        isRecording = false
    }

    private func restart() {
        guard isRecording else { return }
        halt()
        start()
    }

    private func handleInterruption(_ n: Notification) {
        guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: raw) else { return }
        switch type {
        case .began:
            if isRecording { halt() }
            message = tr("录音被打断（电话或 Siri），结束后自动接着录")
        case .ended:
            if wantsRecording { start() }
        @unknown default:
            break
        }
    }

    // MARK: 分段（都在 q 上）

    private func write(_ buffer: AVAudioPCMBuffer) {
        let now = Date()
        if let c = current, now.timeIntervalSince(c.start) >= segmentLength { closeSegment() }
        if file == nil { openSegment(format: buffer.format, start: now) }
        do {
            try file?.write(from: buffer)
        } catch {
            closeSegment()
            DispatchQueue.main.async { self.message = tr("写录音文件出错：{e}", ["e": error.localizedDescription]) }
        }
    }

    private func openSegment(format: AVAudioFormat, start: Date) {
        do {
            try FileManager.default.createDirectory(at: Self.dir, withIntermediateDirectories: true)
            let url = Self.dir.appendingPathComponent("\(Segment.makeID(start: start, device: idPrefix)).m4a")
            // 说话录音 32kbps AAC 足够，10 分钟约 2.4MB，一天 16 小时约 230MB
            let settings: [String: Any] = [AVFormatIDKey: kAudioFormatMPEG4AAC,
                                           AVSampleRateKey: format.sampleRate,
                                           AVNumberOfChannelsKey: format.channelCount,
                                           AVEncoderBitRateKey: 32000 * Int(format.channelCount)]
            file = try AVAudioFile(forWriting: url, settings: settings,
                                   commonFormat: format.commonFormat, interleaved: format.isInterleaved)
            current = (url, start)
        } catch {
            DispatchQueue.main.async { self.message = tr("建录音文件失败：{e}", ["e": error.localizedDescription]) }
        }
    }

    private func closeSegment() {
        guard let c = current else { return }
        file?.close()
        file = nil
        current = nil
        let end = Date()
        guard end.timeIntervalSince(c.start) >= 1 else {
            try? FileManager.default.removeItem(at: c.url)
            return
        }
        DispatchQueue.main.async {
            #if os(watchOS)
            let level = WKInterfaceDevice.current().batteryLevel
            #else
            let level = UIDevice.current.batteryLevel
            #endif
            let seg = Segment(id: c.url.deletingPathExtension().lastPathComponent, start: c.start, end: end,
                              device: self.device, battery: level >= 0 ? Double(level) : nil)
            self.deliver?(c.url, seg)
            self.onSegment?()
            self.segmentsDone += 1
            if level >= 0 && level < self.stopBattery && self.isRecording {
                self.stop()
                self.message = tr("电量低于 5%，已停止录音")
            }
        }
    }
}
