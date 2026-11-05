import SwiftUI
import UIKit
import WebKit

/// 手机的界面就是电脑那套网页（app/ui，打进 app 包的 ui/ 目录），窄屏下自动变成底部页签。
/// 网页调 `window.__TAURI__.core.invoke(命令, 参数)`：普通命令交给 Rust 核心（beayy_call，和电脑同一份代码）；
/// native_ 开头的是只有手机有的（定位、手表、这台 iPhone 录、声纹、上传进度），在这里用原生做。
struct WebScreen: UIViewRepresentable {
    @ObservedObject var native = NativeUI.shared

    func makeCoordinator() -> Bridge { Bridge() }

    func makeUIView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        // 和 Tauri 一样的调用方式，网页代码不用改
        let shim = """
        (function () {
          let n = 0; const pending = {};
          window.__beayyResolve = function (id, res) {
            const p = pending[id]; if (!p) return; delete pending[id];
            if (res && Object.prototype.hasOwnProperty.call(res, "err")) p.reject(res.err); else p.resolve(res ? res.ok : null);
          };
          window.__TAURI__ = { core: { invoke: function (cmd, args) {
            return new Promise(function (resolve, reject) {
              const id = ++n; pending[id] = { resolve: resolve, reject: reject };
              window.webkit.messageHandlers.beayy.postMessage({ id: id, cmd: cmd, args: args || {} });
            });
          } } };
        })();
        """
        cfg.userContentController.addUserScript(WKUserScript(source: shim, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        // 界面语言和外壳同一个判断（Shared/I18n.swift）：i18n.js 先看它，不按网页视图报的 navigator.language
        // （那个跟 app 本地化走，系统是日文这类没有的语言时退回 zh-Hans，外壳却是英文）
        cfg.userContentController.addUserScript(WKUserScript(source: "window.__BEAYY_LANG = \"\(I18n.lang)\";", injectionTime: .atDocumentStart, forMainFrameOnly: true))
        cfg.userContentController.add(context.coordinator, name: "beayy")
        let web = WKWebView(frame: .zero, configuration: cfg)
        web.scrollView.contentInsetAdjustmentBehavior = .never  // 刘海、底部横条由网页按 env(safe-area-inset-*) 留
        web.isInspectable = true
        web.allowsBackForwardNavigationGestures = false
        context.coordinator.web = web
        if let dir = Bundle.main.url(forResource: "ui", withExtension: nil) {
            web.loadFileURL(dir.appendingPathComponent("index.html"), allowingReadAccessTo: dir)
        }
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {}

    final class Bridge: NSObject, WKScriptMessageHandler {
        weak var web: WKWebView?
        private let queue = DispatchQueue(label: "com.fengli.beayy.core", qos: .userInitiated, attributes: .concurrent)

        func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
            guard let body = message.body as? [String: Any], let id = body["id"] as? Int, let cmd = body["cmd"] as? String else { return }
            let args = body["args"] as? [String: Any] ?? [:]
            if cmd.hasPrefix("native_") {
                Task { @MainActor in self.reply(id, NativeUI.shared.handle(cmd, args)) }
                return
            }
            let json = (try? JSONSerialization.data(withJSONObject: args)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
            queue.async {
                let out: String = cmd.withCString { c in
                    json.withCString { a in
                        guard let p = beayy_call(c, a) else { return Self.err(tr("Rust 核心没返回")) }
                        defer { beayy_free(p) }
                        return String(cString: p)
                    }
                }
                DispatchQueue.main.async { self.web?.evaluateJavaScript("window.__beayyResolve(\(id), \(out))") }
            }
        }

        /// {"err": 一句话}，转义交给 JSONSerialization
        private static func err(_ s: String) -> String {
            (try? JSONSerialization.data(withJSONObject: ["err": s])).flatMap { String(data: $0, encoding: .utf8) } ?? #"{"err":"?"}"#
        }

        private func reply(_ id: Int, _ res: [String: Any]) {
            let out = (try? JSONSerialization.data(withJSONObject: res)).flatMap { String(data: $0, encoding: .utf8) } ?? Self.err(tr("返回值不对"))
            web?.evaluateJavaScript("window.__beayyResolve(\(id), \(out))")
        }
    }
}

/// 只有手机有的功能，网页设置里「这台手机」一节用
@MainActor
final class NativeUI: ObservableObject {
    static let shared = NativeUI()
    @Published var showVoiceprint = false

    func handle(_ cmd: String, _ args: [String: Any]) -> [String: Any] {
        switch cmd {
        case "native_status":
            let p = Places.shared, u = Uploader.shared, w = WatchState.shared, r = Recorder.shared
            let auth: String = switch p.auth {
            case .authorizedAlways: "always"
            case .authorizedWhenInUse: "when_in_use"
            case .denied, .restricted: "denied"
            default: "not_determined"
            }
            var s: [String: Any] = ["location": auth, "precise": p.precise, "watch_recording": w.recording,
                                    "phone_recording": r.isRecording, "pending": u.pending, "pending_place_days": u.pendingPlaceDays,
                                    "logged_in": Account.shared.credentials != nil, "device": Config.device]
            if let d = w.updated { s["watch_updated"] = d.iso8601UTC }
            if let d = u.lastUpload { s["last_upload"] = d.iso8601UTC }
            if let e = u.lastError { s["last_error"] = e }
            if let d = r.since { s["phone_since"] = d.iso8601UTC }
            let h = Health.shared
            var hs: [String: Any] = ["available": h.available, "asked": h.asked, "running": h.running, "pending_days": h.pendingDays]
            if let d = h.lastRun { hs["last_run"] = d.iso8601UTC }
            if let e = h.lastError { hs["error"] = e }
            #if targetEnvironment(simulator)
            hs["simulator"] = true
            #endif
            s["health"] = hs
            return ["ok": s]
        case "native_action":
            switch args["name"] as? String ?? "" {
            case "voiceprint": showVoiceprint = true
            case "record_start": Recorder.shared.start()
            case "record_stop": Recorder.shared.stop()
            case "open_settings":
                if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
            case "upload_now": Uploader.shared.uploadPending()
            case "health_connect": Task { await Health.shared.connect() }
            case "health_read": Health.shared.kick(nil)
            #if targetEnvironment(simulator)
            case "health_demo": Task { await Health.shared.seedDemo() }
            #endif
            default: return ["err": tr("手机没有这个操作")]
            }
            return ["ok": NSNull()]
        default:
            return ["err": tr("手机没有这个功能：{cmd}", ["cmd": cmd])]
        }
    }
}

/// 根视图：整屏网页；录声纹这类原生页从下面弹出
struct RootWeb: View {
    @ObservedObject private var native = NativeUI.shared

    var body: some View {
        WebScreen()
            .ignoresSafeArea()
            .sheet(isPresented: $native.showVoiceprint) { VoiceprintView() }
    }
}
