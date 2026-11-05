import SwiftUI

@main
struct BeayyWatchApp: App {
    @StateObject private var recorder = Recorder.shared
    @StateObject private var transfer = Transfer.shared
    @StateObject private var places = WatchPlaces.shared
    @Environment(\.scenePhase) private var phase

    init() {
        Transfer.shared.activate()
        WatchPlaces.shared.start()
        let r = Recorder.shared
        r.idPrefix = "watch"
        r.device = "watch"
        r.deliver = { url, seg in Transfer.shared.send(url: url, segment: seg) }
        r.onSegment = { WatchPlaces.shared.sample() }
        r.onRecordingChange = { on in
            WatchPlaces.shared.setActive(on)
            Transfer.shared.sendState(recording: on)
        }
    }

    var body: some Scene {
        WindowGroup {
            WatchHome()
                .environmentObject(recorder)
                .environmentObject(transfer)
                .environmentObject(places)
        }
        .onChange(of: phase) { _, p in
            if p == .active {
                recorder.resumeIfWanted()
                transfer.requeue()
            }
        }
    }
}

/// 手表首页：照设计系统「墨线」的「原生 app」一节，和系统录音 app 一样——上面状态，中间大计时器，
/// 下面一个圆形按钮（开始／停止），最下面一行写现在在哪
struct WatchHome: View {
    @EnvironmentObject var recorder: Recorder
    @EnvironmentObject var transfer: Transfer
    @EnvironmentObject var places: WatchPlaces

    var body: some View {
        ScrollView {
            VStack(spacing: 8) {
                Status(level: recorder.isRecording ? .ok : .none, text: recorder.isRecording ? tr("在录") : tr("没在录"),
                       dot: recorder.isRecording ? Theme.watch : nil)
                if recorder.isRecording, let since = recorder.since {
                    TimelineView(.periodic(from: .now, by: 1)) { ctx in
                        Text(clock(ctx.date.timeIntervalSince(since)))
                            .font(.system(size: 34, weight: .medium, design: .monospaced))
                            .minimumScaleFactor(0.7).lineLimit(1)
                            .foregroundStyle(Theme.ink)
                    }
                } else {
                    Text(tr("全天录音")).font(.system(size: 22, weight: .medium)).foregroundStyle(Theme.ink)
                }
                RoundRecordButton(recording: recorder.isRecording, size: 60) {
                    recorder.isRecording ? recorder.stop() : recorder.start()
                }
                .padding(.vertical, 2)
                location
                if transfer.pending > 0 {
                    Text(tr("{n} 段等着送到 iPhone", ["n": "\(transfer.pending)"])).font(.system(size: 13)).foregroundStyle(Theme.ink3)
                }
                if let m = recorder.message {
                    Text(m).font(.system(size: 13)).foregroundStyle(Theme.warnInk).multilineTextAlignment(.center)
                }
            }
            .frame(maxWidth: .infinity)
        }
    }

    @ViewBuilder private var location: some View {
        if !places.ready {
            Button { places.requestPermission() } label: {
                Label(tr("位置没开"), systemImage: "location.slash").font(.system(size: 13)).foregroundStyle(Theme.warnInk)
            }
            .buttonStyle(.plain)
        } else if recorder.isRecording {
            Label(places.stayed.map { $0 >= StayDetector.minStay ? tr("在这里 {t}", ["t": spanText($0)]) : tr("刚到") } ?? tr("在取位置"),
                  systemImage: "location").font(.system(size: 13)).foregroundStyle(Theme.ink3)
        } else {
            Text(tr("今天记了 {n} 处", ["n": "\(places.today)"])).font(.system(size: 13)).foregroundStyle(Theme.ink3)
        }
    }
}
