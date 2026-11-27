import SwiftUI
import HeronCore

/// The words around a "never" rule (heron-v1.md item 35h). Public so tests check them directly.
public enum NeverCopy {
    /// "fetch web pages", "run commands": what a tool does, for "Heron will be allowed to …".
    static func verb(forTool tool: String) -> String {
        switch tool {
        case "web_fetch", "fetch_url": "fetch web pages"
        case "web_search": "search the web"
        case "shell", "run_command": "run commands"
        case "send_email": "send email"
        case "write_file", "edit_file": "change files"
        case "read_file", "list_dir", "search_files", "find_files": "read files"
        default: "use \(tool)"
        }
    }

    /// The pattern part of `tool:pattern`; the whole rule when it doesn't start with the tool.
    static func pattern(of rule: NeverRule) -> String {
        let prefix = rule.tool + ":"
        return rule.rule.hasPrefix(prefix) ? String(rule.rule.dropFirst(prefix.count)) : rule.rule
    }

    /// What the user lets Heron do by removing the rule. The confirm sheet leads with it.
    public static func allowedSentence(_ rule: NeverRule) -> String {
        if rule.rule == "*" { return "Heron will be allowed to use any tool again." }
        if rule.tool.isEmpty || rule.tool == "*" { return "Heron will be allowed to do what the rule “\(rule.rule)” refused." }
        let verb = verb(forTool: rule.tool)
        let pattern = pattern(of: rule)
        return pattern == "*" ? "Heron will be allowed to \(verb) again." : "Heron will be allowed to \(verb) where it matches “\(pattern)”."
    }

    public static let afterRemoval = "Whether it asks first then depends on your permission mode."

    /// Where the rule lives. A rule that `config.toml` doesn't list is held by Heron only.
    public static func whereItLives(_ rule: NeverRule) -> String {
        rule.inFile ? "In config.toml" : "Kept by Heron only. It isn't in config.toml."
    }
}

/// Settings › Permissions › Never: every "never" rule with a Remove button. It has no scroll view of its own:
/// the page's one scroll container carries it (heron-v1.md item 41b).
struct NeverSection: View {
    let store: SettingsStore
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Never")
            Text("Heron refuses these in every mode. Remove one to let Heron do it again.")
                .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            if let message = store.neverError {
                HStack(alignment: .firstTextBaseline, spacing: Spacing.s2) {
                    Image(systemName: "exclamationmark.triangle").foregroundStyle(theme.danger)
                    Text(message).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                }
                .padding(Spacing.s3).frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.dangerSoft))
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Couldn't change the Never list. \(message)")
            }
            if store.nevers.isEmpty {
                Text("No “never” rules.").foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(store.nevers.enumerated()), id: \.element.id) { i, rule in
                        if i > 0 { RowDivider() }
                        HStack(alignment: .center, spacing: Spacing.s3) {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(rule.rule).font(HeronFont.mono).foregroundStyle(theme.ink).fixedSize(horizontal: false, vertical: true)
                                Text(NeverCopy.whereItLives(rule)).font(HeronFont.callout).foregroundStyle(theme.ink2)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: Spacing.s3)
                            Button("Remove") { store.askToRemoveNever(rule) }
                                .buttonStyle(HButtonStyle(.secondary)).disabled(store.isRemovingNever)
                                .accessibilityLabel("Remove the never rule \(rule.rule)")
                        }
                        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                    }
                }
            }
        }
        .sheet(item: Binding(get: { store.pendingNeverRemoval }, set: { if $0 == nil { store.cancelNeverRemoval() } })) { rule in
            NeverRemoveConfirmView(rule: rule, onCancel: { store.cancelNeverRemoval() },
                                   onConfirm: { Task { await store.confirmNeverRemoval() } })
        }
    }
}

/// The sheet Remove opens: it says what Heron will be allowed to do, and changes nothing until the user confirms.
/// Esc cancels, so a stray key never lifts a "never".
struct NeverRemoveConfirmView: View {
    let rule: NeverRule
    let onCancel: () -> Void
    let onConfirm: () -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text("Remove this “never”?").font(HeronFont.title2).foregroundStyle(theme.ink).accessibilityAddTraits(.isHeader)
            Text(rule.rule).font(HeronFont.mono).foregroundStyle(theme.ink)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(Spacing.s3)
                .background(RoundedRectangle(cornerRadius: Radius.control, style: .continuous).fill(theme.raised))
                .textSelection(.enabled)
            VStack(alignment: .leading, spacing: Spacing.s2) {
                Text(NeverCopy.allowedSentence(rule)).foregroundStyle(theme.ink)
                Text(NeverCopy.afterRemoval).foregroundStyle(theme.ink2)
                if !rule.inFile { Text(NeverCopy.whereItLives(rule)).foregroundStyle(theme.ink2) }
            }
            .fixedSize(horizontal: false, vertical: true)
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).buttonStyle(HButtonStyle(.secondary)).keyboardShortcut(.cancelAction)
                Button("Remove rule", action: onConfirm).buttonStyle(HButtonStyle(.danger))
            }
        }
        .padding(Spacing.s6)
        .frame(width: 480, alignment: .leading)
        .background(theme.surface)
        .overlay(RoundedRectangle(cornerRadius: Radius.sheet, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Remove the never rule \(rule.rule)?")
    }
}
