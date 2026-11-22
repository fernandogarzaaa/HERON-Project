import SwiftUI
import HeronCore

/// F2, "Why is it asking?": the words and the row for the feature. The core builds `approval.why` itself;
/// the chat's chip says the same second sentence from the `tainted` event's source, so the two never disagree.
public enum AskReasonsFeature {
    /// The core key behind the switch (default on). Off: the core leaves `why` out and the chip hides.
    public static let key = "ask_reasons"

    /// Settings › Features. The title is the one the core gives the key (`src/features.rs`).
    public static let row = FeatureRow(key: key, title: "Explain approvals", kind: .core(key: key))
}

/// The sentence the chip shows, from the taint source. It matches `approval_text::taint_sentence` in the core.
public enum TaintChipCopy {
    /// The sentence for a conversation whose source is unknown: the core's text from before sources were recorded.
    public static let noSource = "Heron has read content from outside (a web page, an email or a connected app) in this conversation. Check that this is something you asked for."

    /// What Heron read, for "Heron has read … in this conversation". Nil for a kind this version doesn't know.
    public static func what(_ source: TaintSource) -> String? {
        switch source.kind {
        case "tool": return source.detail.isEmpty ? "the output of \(source.name)" : source.detail
        case "chat_context": return "text from \(source.detail)"
        case "mcp": return "a request from another app over MCP"
        case "telegram_forward": return "a forwarded Telegram message"
        case "trigger": return "the event that started \(source.name)"
        case "suggestion": return "a task Heron prepared from what it observed"
        default: return nil
        }
    }

    /// "Heron has read text from Preview in this conversation, so changes and sends ask first."
    public static func sentence(_ source: TaintSource?) -> String {
        guard let source, let what = what(source) else { return noSource }
        return "Heron has read \(what) in this conversation, so changes and sends ask first."
    }
}

/// The violet chip at the top of a chat that has read outside content. It names what, in one sentence, and
/// the words "Outside content" label it, so colour is never the only signal.
public struct TaintChip: View {
    let source: TaintSource?
    @Environment(\.heronTheme) private var theme

    public init(source: TaintSource?) { self.source = source }

    public var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
            Image(systemName: "globe").imageScale(.small).foregroundStyle(theme.outside)
            Text(TaintChipCopy.sentence(source)).font(HeronFont.callout).foregroundStyle(theme.outside)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, Spacing.s3).padding(.vertical, Spacing.s2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.outsideSoft))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Outside content. \(TaintChipCopy.sentence(source))")
    }
}
