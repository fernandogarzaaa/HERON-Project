import SwiftUI
import AppKit

/// The agent's avatar: `ripple`, `heron`, `monogram` or `image`, drawn with SwiftUI shapes (no image
/// assets). State shows through colour, a badge and motion; Reduce Motion stops the loops and leaves
/// colour and badge.
public struct AvatarView: View {
    let style: AvatarStyle
    let state: AvatarState
    let name: String
    let size: CGFloat
    /// The picture for the `image` style. Nil, or a missing file, falls back to `ripple`.
    let image: NSImage?

    @Environment(\.heronTheme) private var theme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.heronSnapshotMode) private var snapshotMode

    public init(style: AvatarStyle = .ripple, state: AvatarState, name: String = "Heron", size: CGFloat = 32, image: NSImage? = nil) {
        self.style = style
        self.state = state
        self.name = name
        self.size = size
        self.image = image
    }

    private var look: AvatarLook { AvatarLook.make(state: state, reduceMotion: reduceMotion || snapshotMode) }

    private var toneColor: Color {
        switch look.tone {
        case .accent: theme.accent
        case .waiting: theme.waitingMark
        case .danger: theme.danger
        }
    }

    public var body: some View {
        ZStack(alignment: .bottomTrailing) {
            avatar
                .frame(width: size, height: size)
            if let badge = look.badge {
                let d = max(10, size * 0.34)
                Image(systemName: badge)
                    .font(.system(size: d * 0.55, weight: .bold))
                    .foregroundStyle(theme.isDark ? Color.black : Color.white)
                    .frame(width: d, height: d)
                    .background(Circle().fill(toneColor))
                    .overlay(Circle().stroke(theme.sidebar, lineWidth: 1.5))
                    .offset(x: d * 0.15, y: d * 0.15)
            }
        }
        .frame(width: size, height: size)
        .animation(.easeOut(duration: look.transitionDuration), value: state)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(AvatarLook.accessibilityLabel(name: name, state: state))
    }

    @ViewBuilder private var avatar: some View {
        if look.animates {
            TimelineView(.animation) { context in
                canvas(t: context.date.timeIntervalSinceReferenceDate)
            }
        } else {
            canvas(t: 0)
        }
    }

    @ViewBuilder private func canvas(t: Double) -> some View {
        switch style {
        case .ripple:
            Canvas { ctx, sz in drawRipple(&ctx, sz, t: t) }
        case .heron:
            Canvas { ctx, sz in drawDisc(&ctx, sz, t: t) { c, r, center in drawHeron(&c, r, center) } }
        case .monogram:
            ZStack {
                Canvas { ctx, sz in drawDisc(&ctx, sz, t: t) { _, _, _ in } }
                Text(String(name.first ?? "H").uppercased())
                    .font(.system(size: size * 0.38, weight: .semibold, design: .serif))
                    .foregroundStyle(theme.onAccent)
            }
        case .image:
            if let image {
                ZStack {
                    Image(nsImage: image).resizable().scaledToFill()
                        .frame(width: size * 0.78, height: size * 0.78).clipShape(Circle())
                    Canvas { ctx, sz in drawRing(&ctx, sz) }
                }
            } else {
                Canvas { ctx, sz in drawRipple(&ctx, sz, t: t) }
            }
        }
    }

    // MARK: Drawing

    private func loopValues(_ t: Double) -> (breathe: Double, rippleRings: [(r: Double, a: Double)], core: Double) {
        switch look.loop {
        case .none:
            return (1, [], 1)
        case .breathe(let period, let scale):
            let p = 0.5 - 0.5 * cos(2 * .pi * t / period)
            return (1 + (scale - 1) * p, [], 1)
        case .ripple(let period, let rings):
            let list = (0..<rings).map { i -> (r: Double, a: Double) in
                let phase = (t / period + Double(i) / Double(rings)).truncatingRemainder(dividingBy: 1)
                return (0.3 + 0.7 * phase, 1 - phase)
            }
            return (1, list, 1)
        case .pulse(let period, let from, let to):
            let p = 0.5 - 0.5 * cos(2 * .pi * t / (period * 2))
            return (1, [], from + (to - from) * p)
        }
    }

    private func drawRipple(_ ctx: inout GraphicsContext, _ sz: CGSize, t: Double) {
        let R = min(sz.width, sz.height) / 2
        let c = CGPoint(x: sz.width / 2, y: sz.height / 2)
        let v = loopValues(t)
        let line = max(1.2, R * 0.09)
        func ring(_ f: Double, _ alpha: Double, width: Double? = nil) {
            let r = R * f * 0.94
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2)),
                       with: .color(toneColor.opacity(alpha)), lineWidth: width ?? line)
        }
        if case .ripple = look.loop {
            for r in v.rippleRings { ring(r.r, r.a) }
        } else {
            // Static rings (also the Reduce Motion state): outer, middle, and an inner ring that widens while thinking.
            ring(0.92 * v.breathe, 0.35)
            ring(0.62, 0.6)
            ring(0.38, 1, width: look.widenedInnerRing ? line * 2.4 : line)
        }
        let coreR = R * 0.17 * v.core
        ctx.fill(Path(ellipseIn: CGRect(x: c.x - coreR, y: c.y - coreR, width: coreR * 2, height: coreR * 2)), with: .color(toneColor))
    }

    /// Disc styles: an accent disc with a halo ring that carries the loop.
    private func drawDisc(_ ctx: inout GraphicsContext, _ sz: CGSize, t: Double,
                          mark: (inout GraphicsContext, CGFloat, CGPoint) -> Void) {
        let R = min(sz.width, sz.height) / 2
        let c = CGPoint(x: sz.width / 2, y: sz.height / 2)
        let v = loopValues(t)
        let discR = R * 0.72 * v.core
        let line = max(1.2, R * 0.07)
        if case .ripple = look.loop {
            for r in v.rippleRings {
                let rr = R * (0.72 + 0.28 * (r.r - 0.3) / 0.7) * 0.97
                ctx.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr, width: rr * 2, height: rr * 2)),
                           with: .color(toneColor.opacity(r.a)), lineWidth: line)
            }
        } else {
            let rr = R * 0.92 * v.breathe
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - rr, y: c.y - rr, width: rr * 2, height: rr * 2)),
                       with: .color(toneColor.opacity(0.4)), lineWidth: look.widenedInnerRing ? line * 2 : line)
        }
        ctx.fill(Path(ellipseIn: CGRect(x: c.x - discR, y: c.y - discR, width: discR * 2, height: discR * 2)), with: .color(toneColor))
        mark(&ctx, discR, c)
    }

    private func drawRing(_ ctx: inout GraphicsContext, _ sz: CGSize) {
        let R = min(sz.width, sz.height) / 2
        let r = R * 0.94
        ctx.stroke(Path(ellipseIn: CGRect(x: R - r, y: R - r, width: r * 2, height: r * 2)),
                   with: .color(toneColor), lineWidth: max(2, R * 0.12))
    }

    /// A heron's head and neck in one stroke.
    private func drawHeron(_ ctx: inout GraphicsContext, _ r: CGFloat, _ c: CGPoint) {
        func p(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: c.x + (x - 0.5) * r * 1.5, y: c.y + (y - 0.5) * r * 1.5) }
        var path = Path()
        path.move(to: p(0.38, 0.92))
        path.addCurve(to: p(0.5, 0.52), control1: p(0.7, 0.8), control2: p(0.28, 0.66))
        path.addCurve(to: p(0.46, 0.2), control1: p(0.66, 0.4), control2: p(0.4, 0.34))
        path.addLine(to: p(0.86, 0.3))
        ctx.stroke(path, with: .color(theme.onAccent), style: StrokeStyle(lineWidth: max(1.5, r * 0.14), lineCap: .round, lineJoin: .round))
        let eye = CGRect(x: p(0.5, 0.22).x - r * 0.05, y: p(0.5, 0.22).y - r * 0.05, width: r * 0.1, height: r * 0.1)
        ctx.fill(Path(ellipseIn: eye), with: .color(theme.onAccent))
    }
}
