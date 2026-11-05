import SwiftUI
import UIKit

final class AppDelegate: NSObject, UIApplicationDelegate {
    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        Receiver.shared.activate()  // 系统可能是为了送手表的文件在后台拉起的，尽早激活
        let r = Recorder.shared       // 这台 iPhone 自己录（没戴手表时）：分段 iphone-…，直接进待上传
        r.idPrefix = "iphone"
        r.device = Config.device
        r.deliver = deliverOwnSegment
        Places.shared.start()        // 也可能是为了送到访／位置变化拉起的，manager 要在这里就建好
        Health.shared.start()        // 也可能是苹果送新的健康数据拉起的，观察者要在这里就挂上
        ICloud.start()               // iCloud 容器路径交给 Rust（不要账号的 iCloud 同步），盯着别的设备传来的文件下载
        Uploader.shared.uploadPending()
        Account.shared.startConfigTimer()  // 高德 Key、转写服务地址：启动时取一次，之后每小时
        beayy_start()                      // Rust 核心：账号同步（只下载）、录音和停留点，和电脑同一份代码
        return true
    }

    func application(_ application: UIApplication, handleEventsForBackgroundURLSession identifier: String,
                     completionHandler: @escaping () -> Void) {
        if identifier == Uploader.sessionID {
            Uploader.shared.backgroundCompletion = completionHandler
            Uploader.shared.uploadPending()  // 重建 session，接上后台事件
        }
    }
}

@main
struct BeayyPhoneApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            RootWeb()
        }
        .onChange(of: phase) { _, p in
            if p == .active {
                ICloud.start()  // 中途在设置里开了 iCloud 云盘
                Uploader.shared.uploadPending()
                Task { await Account.shared.refreshConfig() }
                Places.shared.retryNames()  // 当时没网、高德出错没取到名的再取一次
                if Health.shared.asked { Health.shared.kick(nil) }  // 锁屏时没读成的接着读
                Recorder.shared.resumeIfWanted()
                WatchState.shared.load()
            }
        }
    }
}

/// 时刻和日期的写法
enum Fmt {
    static func md(_ d: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "MM-dd"
        return f.string(from: d)
    }

    /// 今天的只写时刻，别的天带上日期
    static func when(_ d: Date) -> String {
        Calendar.current.isDateInToday(d) ? hm(d) : md(d) + " " + hm(d)
    }

    static func parse(_ iso: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f.date(from: iso) ?? ISO8601DateFormatter().date(from: iso)
    }

    /// 说话多久：不到一小时写分钟
    static func talk(_ sec: TimeInterval) -> String { spanText(sec) }
}
