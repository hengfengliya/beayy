import SwiftUI

/// 墨线「原生 app」一节的共用件，iPhone 和手表两边用同一份：分组卡片（Card）、卡里的一行（InfoRow）、
/// 状态（Status）、出问题时的提示条（Banner）、按钮（InkButtonStyle、RoundRecordButton）、一天的地点色带（PlaceStrip）。
/// 图标用 SF Symbols。

/// 一张卡：grouped 底上的 surface 白卡，圆角 18、内边距 16，不加阴影和描边
struct Card<Content: View>: View {
    var title: String? = nil
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let title { Text(title).font(Typo.label).foregroundStyle(Theme.ink4) }
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Theme.surface))
    }
}

/// 卡里的一行：左边名称（可带一句说明），右边值或状态；行之间一条细线（最后一行传 last: true 不画）
struct InfoRow<Trailing: View>: View {
    var name: String
    var note: String? = nil
    var last = false
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name).font(Typo.body).foregroundStyle(Theme.ink)
                if let note { Text(note).font(Typo.label).foregroundStyle(Theme.ink3).fixedSize(horizontal: false, vertical: true) }
            }
            Spacer(minLength: 8)
            trailing
        }
        .padding(.vertical, 8)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(Theme.line).frame(height: 1) } }
    }
}

extension InfoRow where Trailing == Text {
    init(_ name: String, _ value: String, note: String? = nil, last: Bool = false) {
        self.init(name: name, note: note, last: last) { Text(value).font(Typo.note).foregroundStyle(Theme.ink3) }
    }
}

/// 状态：一个圆点加一个词。只有等人处理（warn）和出错（risk）用彩色字
enum Level { case none, ok, warn, risk }

struct Status: View {
    var level: Level
    var text: String
    var dot: Color? = nil

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(dot ?? dotColor).frame(width: 8, height: 8)
            Text(text).font(Typo.note).foregroundStyle(fg)
        }
    }

    private var dotColor: Color {
        switch level {
        case .ok: return Theme.ok
        case .warn: return Theme.warn
        case .risk: return Theme.risk
        case .none: return Theme.ink4
        }
    }

    private var fg: Color {
        switch level {
        case .warn: return Theme.warnInk
        case .risk: return Theme.riskInk
        default: return Theme.ink2
        }
    }
}

/// 出问题时首页顶上冒的一条：图标、说错在哪，右边箭头（点了去处理）
struct Banner: View {
    var icon: String
    var text: String
    var risk = false

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: icon).font(.system(size: 15, weight: .medium))
            Text(text).font(Typo.note).multilineTextAlignment(.leading)
            Spacer(minLength: 4)
            Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).opacity(0.6)
        }
        .foregroundStyle(risk ? Theme.riskInk : Theme.warnInk)
        .padding(14)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(risk ? Theme.riskWash : Theme.warnWash))
    }
}

/// 主按钮（一页一个：墨黑底，深色反成白底）和次按钮（透明底、细边）
struct InkButtonStyle: ButtonStyle {
    var primary = true

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 16, weight: .medium))
            .foregroundStyle(primary ? Theme.onPrimary : Theme.ink)
            .frame(maxWidth: .infinity, minHeight: 44)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(primary ? Theme.primary : Color.clear))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(primary ? Color.clear : Theme.lineStrong))
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}

/// 录音的圆形按钮：没在录是实心圆点（开始），在录是方块（停止）。和系统录音 app 一样
struct RoundRecordButton: View {
    var recording: Bool
    var size: CGFloat = 56
    var action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle().stroke(Theme.lineStrong, lineWidth: 2)
                if recording {
                    RoundedRectangle(cornerRadius: size * 0.08).fill(Theme.ink).frame(width: size * 0.34, height: size * 0.34)
                } else {
                    Circle().fill(Theme.ink).frame(width: size * 0.42, height: size * 0.42)
                }
            }
            .frame(width: size, height: size)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(recording ? tr("停止录音") : tr("开始录音"))
    }
}

/// 一天的形状（同桌面版 DayStrip，设计系统「人生记录」）：0–24 点，上面一条地点（停留交替 place／place-alt，
/// 两段之间的路上画一条细线），下面一条说话（对话的起止，watch 色）。今天的底色只画到现在，现在画一条墨色竖线
struct PlaceStrip: View {
    var day = Date()
    /// 停留：(开始, 结束)
    var stays: [(Date, Date)]
    /// 路上：(开始, 结束)
    var transits: [(Date, Date)] = []
    /// 说话：(开始, 结束)；为空就不画这一条
    var talks: [(Date, Date)]? = nil

    var body: some View {
        let today = Calendar.current.isDateInToday(day)
        let now = Date()
        VStack(alignment: .leading, spacing: 4) {
            row(tr("地点")) { x, w, h in
                ZStack(alignment: .leading) {
                    RoundedRectangle(cornerRadius: 2).fill(Theme.track).frame(width: today ? x(now) : w)
                    ForEach(Array(transits.enumerated()), id: \.offset) { _, t in
                        Rectangle().fill(Theme.ink4).frame(width: max(x(t.1) - x(t.0), 1), height: 1).offset(x: x(t.0))
                    }
                    ForEach(Array(stays.enumerated()), id: \.offset) { i, s in
                        Rectangle().fill(i % 2 == 0 ? Theme.place : Theme.placeAlt)
                            .frame(width: max(x(s.1) - x(s.0), 1.5)).offset(x: x(s.0))
                    }
                }
            }
            if let talks {
                row(tr("说话")) { x, w, _ in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 2).fill(Theme.track).frame(width: today ? x(now) : w)
                        ForEach(Array(talks.enumerated()), id: \.offset) { _, t in
                            Rectangle().fill(Theme.watch).frame(width: max(x(t.1) - x(t.0), 1.5)).offset(x: x(t.0))
                        }
                    }
                }
            }
            HStack(spacing: 8) {
                Color.clear.frame(width: 28, height: 1)
                HStack {
                    ForEach([0, 6, 12, 18, 24], id: \.self) { h in
                        Text("\(h)").font(.system(size: 10, design: .monospaced)).foregroundStyle(Theme.ink4)
                        if h < 24 { Spacer() }
                    }
                }
            }
        }
        .overlay(alignment: .topLeading) {
            if today {
                GeometryReader { g in
                    let w = g.size.width - 36
                    let f = now.timeIntervalSince(Calendar.current.startOfDay(for: now)) / 86400
                    Rectangle().fill(Theme.ink).frame(width: 1.5, height: talks == nil ? 12 : 26).offset(x: 36 + w * f, y: -2)
                }
            }
        }
    }

    /// 一排：行首写这一排是什么，右边 8 高的条
    private func row<C: View>(_ name: String, @ViewBuilder _ bar: @escaping (_ x: @escaping (Date) -> CGFloat, _ w: CGFloat, _ h: CGFloat) -> C) -> some View {
        HStack(spacing: 8) {
            Text(name).font(.system(size: 10)).foregroundStyle(Theme.ink4).frame(width: 28, alignment: .leading)
            GeometryReader { g in
                let d0 = Calendar.current.startOfDay(for: day)
                bar({ d in max(0, min(1, d.timeIntervalSince(d0) / 86400)) * g.size.width }, g.size.width, g.size.height)
            }
            .frame(height: 8)
        }
    }
}

/// 时长：不到一小时写分钟，否则写 6.2h（同桌面版）
func spanText(_ sec: TimeInterval) -> String {
    sec < 3600 ? tr("{n} 分钟", ["n": "\(max(1, Int((sec / 60).rounded())))"]) : String(format: "%.1fh", sec / 3600)
}

/// 时刻 09:04
func hm(_ d: Date) -> String {
    let f = DateFormatter()
    f.dateFormat = "HH:mm"
    return f.string(from: d)
}

/// 计时器 2:13:05（不到一小时 13:05）
func clock(_ sec: TimeInterval) -> String {
    let s = max(0, Int(sec))
    return s >= 3600 ? String(format: "%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60) : String(format: "%02d:%02d", s / 60, s % 60)
}
