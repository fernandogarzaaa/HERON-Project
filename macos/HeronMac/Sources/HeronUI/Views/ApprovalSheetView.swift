import SwiftUI
import HeronCore

/// The words on the approval sheet, built from the event. Public so tests check them directly.
public enum ApprovalCopy {
    /// A question built from the action: "Create a file in your working folder?".
    public static func title(_ r: ApprovalRequest) -> String {
        switch r.tool {
        case "write_file", "edit_file": return "Create a file in your working folder?"
        case "shell", "run_command": return "Run a command on this Mac?"
        case "send_email": return "Send an email?"
        case "read_file", "list_dir", "search_files", "find_files": return "Read outside your working folder?"
        case "web_search", "fetch_url": return "Look something up on the web?"
        default:
            let s = r.summary.trimmingCharacters(in: .whitespaces)
            return s.hasSuffix("?") ? s : (s.isEmpty ? "Allow \(r.tool)?" : s + "?")
        }
    }

    /// High-risk actions name the action ("Send", "Delete"), never just "Allow".
    public static func primaryTitle(_ r: ApprovalRequest) -> String {
        guard r.isHighRisk else { return "Allow" }
        let t = r.tool.lowercased()
        if t.contains("send") || t.contains("email") || t.contains("post") { return "Send" }
        if t.contains("delete") || t.contains("remove") { return "Delete" }
        if t.contains("pay") || t.contains("purchase") || t.contains("spend") { return "Pay" }
        return "Do it"
    }

    public struct Impact: Equatable, Sendable { public var symbol: String; public var text: String }

    /// What changes, and whether it can be undone.
    public static func impact(_ r: ApprovalRequest) -> [Impact] {
        switch r.risk {
        case "write", "state":
            return [Impact(symbol: "doc.text", text: "Changes a file in your working folder."),
                    Impact(symbol: "arrow.uturn.backward", text: "You can undo it afterwards.")]
        case "exec":
            return [Impact(symbol: "terminal", text: "Runs a command inside the macOS sandbox."),
                    Impact(symbol: "exclamationmark.triangle", text: "Files it changes can't always be restored.")]
        case "external", "egress":
            return [Impact(symbol: "paperplane", text: "Sends something outside this Mac."),
                    Impact(symbol: "exclamationmark.triangle", text: "It can't be undone.")]
        case "network":
            return [Impact(symbol: "globe", text: "Reads from the internet.")]
        case "outside":
            return [Impact(symbol: "folder", text: "Reads a file outside your working folder.")]
        default:
            return [Impact(symbol: "exclamationmark.triangle", text: r.isHighRisk ? "It can't be undone." : "Changes something on this Mac.")]
        }
    }

    /// The violet line for a conversation that has read outside content.
    public static func taintedLine(_ r: ApprovalRequest) -> String {
        let what = r.outsideSource.map { ": \($0)" } ?? ""
        return "This conversation has read outside content\(what). Check that this is what you asked for."
    }

    /// "Always in ~/Work/drafts", from the rule the core offers.
    public static func alwaysTitle(_ r: ApprovalRequest) -> String {
        guard let rule = r.alwaysRule, let colon = rule.firstIndex(of: ":") else { return "Always allow this" }
        var target = String(rule[rule.index(after: colon)...])
        for suffix in ["/**", "/*", "**", "*"] where target.hasSuffix(suffix) { target = String(target.dropLast(suffix.count)); break }
        return target.isEmpty ? "Always allow this" : "Always in \(target)"
    }

    public static let untaintedNoScope = "Because this conversation has read outside content, Heron asks every time. This permission can't be saved."
}

/// The window-modal sheet for a live approval: what will happen, what it changes, whether outside content
/// is involved, and how long the permission lasts. 520 pt wide.
public struct ApprovalSheetView: View {
    public enum Scope: Hashable, Sendable { case once, session, always }

    let request: ApprovalRequest
    let avatarStyle: AvatarStyle
    let agentName: String
    let onAnswer: (ApprovalDecision) -> Void

    @Environment(\.heronTheme) private var theme
    @State private var scope: Scope = .once
    @State private var showDetails = false

    public init(request: ApprovalRequest, avatarStyle: AvatarStyle = .ripple, agentName: String = "Heron",
                onAnswer: @escaping (ApprovalDecision) -> Void) {
        self.request = request
        self.avatarStyle = avatarStyle
        self.agentName = agentName
        self.onAnswer = onAnswer
    }

    private var offersScope: Bool { request.canSession || request.canAlways }

    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            header
            detail
            impact
            scopeSection
            if showDetails { rawDetails }
            footer
        }
        .padding(Spacing.s6)
        .frame(width: 520, alignment: .leading)
        .background(theme.surface)
        .overlay(RoundedRectangle(cornerRadius: Radius.sheet, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(ApprovalCopy.title(request))
    }

    private var header: some View {
        HStack(alignment: .top, spacing: Spacing.s3) {
            AvatarView(style: avatarStyle, state: .awaitingApproval, name: agentName, size: 40)
            VStack(alignment: .leading, spacing: Spacing.s1) {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
                    Text(ApprovalCopy.title(request)).font(HeronFont.title2).foregroundStyle(theme.ink)
                        .accessibilityAddTraits(.isHeader)
                    if request.isHighRisk { HTag("Can't be undone", tone: .danger) }
                }
                if let reason = request.reason {
                    Text(reason).font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    private var detail: some View {
        Text(request.detail.isEmpty ? request.summary : request.detail)
            .font(HeronFont.mono).foregroundStyle(theme.ink)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Spacing.s3)
            .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.raised))
            .textSelection(.enabled)
    }

    private var impact: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            ForEach(Array(ApprovalCopy.impact(request).enumerated()), id: \.offset) { _, line in
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
                    Image(systemName: line.symbol).foregroundStyle(theme.ink2).frame(width: 18)
                    Text(line.text).foregroundStyle(theme.ink)
                }
            }
            if request.tainted {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
                    Image(systemName: "globe").foregroundStyle(theme.outside).frame(width: 18)
                    Text(ApprovalCopy.taintedLine(request)).foregroundStyle(theme.outside).fixedSize(horizontal: false, vertical: true)
                }
                .padding(Spacing.s3)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.outsideSoft))
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Outside content. \(ApprovalCopy.taintedLine(request))")
            }
        }
    }

    @ViewBuilder private var scopeSection: some View {
        if offersScope {
            VStack(alignment: .leading, spacing: Spacing.s2) {
                SectionLabel("How long")
                HRadioRow(value: Scope.once, selection: $scope, title: "Only this time")
                if request.canSession {
                    HRadioRow(value: Scope.session, selection: $scope, title: "For the rest of this conversation")
                }
                if request.canAlways {
                    HRadioRow(value: Scope.always, selection: $scope, title: ApprovalCopy.alwaysTitle(request),
                              subtitle: "You can remove this in Settings › Permissions.")
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel("How long to allow it")
        } else if request.tainted {
            Text(ApprovalCopy.untaintedNoScope).font(HeronFont.callout).foregroundStyle(theme.ink2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var rawDetails: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            Text("Tool: \(request.tool)").font(HeronFont.mono).foregroundStyle(theme.ink2)
            if let rule = request.alwaysRule { Text("Rule: \(rule)").font(HeronFont.mono).foregroundStyle(theme.ink2) }
            Text("Risk: \(request.risk)").font(HeronFont.mono).foregroundStyle(theme.ink2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .textSelection(.enabled)
    }

    private var footer: some View {
        HStack {
            Button(showDetails ? "Hide details" : "Show details") { withAnimation(Motion.fast) { showDetails.toggle() } }
                .buttonStyle(HButtonStyle(.link))
            Spacer()
            // High risk: "Don't allow" is the default button, so Return doesn't approve.
            Button("Don't allow") { onAnswer(.deny) }
                .buttonStyle(HButtonStyle(.secondary))
                .keyboardShortcut(request.isHighRisk ? .defaultAction : .cancelAction)
            Button(ApprovalCopy.primaryTitle(request)) { onAnswer(decision) }
                .buttonStyle(HButtonStyle(request.isHighRisk ? .danger : .primary))
                .modifier(DefaultActionIf(!request.isHighRisk))
        }
    }

    private var decision: ApprovalDecision {
        switch scope {
        case .once: .once
        case .session: request.canSession ? .session : .once
        case .always: request.canAlways ? .always : .once
        }
    }
}

private struct DefaultActionIf: ViewModifier {
    let on: Bool
    init(_ on: Bool) { self.on = on }
    @ViewBuilder func body(content: Content) -> some View {
        if on { content.keyboardShortcut(.defaultAction) } else { content }
    }
}
