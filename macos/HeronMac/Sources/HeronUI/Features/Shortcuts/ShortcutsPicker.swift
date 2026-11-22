import SwiftUI
import Observation
import HeronCore

/// What the picker shows, in the order the spec names the states.
public enum ShortcutsPickerPhase: Equatable, Sendable {
    case idle, loading
    /// The list is on screen and can be ticked.
    case ready
    /// `409 feature_off`: the core key `shortcuts` is off.
    case featureOff
    /// `available: false`: the core can't run `/usr/bin/shortcuts` on this Mac.
    case unavailable
    case failed(String)
}

/// Settings › Features › Run your Shortcuts: the owner's shortcuts, and the ones Heron may run.
///
/// Every tick saves the whole allowlist through `POST /api/shortcuts/allowed` and shows what the core saved.
/// Ticks are ignored while a save is in flight, so two saves can never land out of order.
@MainActor
@Observable
public final class ShortcutsPickerStore {
    public let client: any ShortcutsClientProtocol
    public private(set) var phase: ShortcutsPickerPhase = .idle
    /// Every shortcut `shortcuts list` gave, in the core's order.
    public private(set) var shortcuts: [String] = []
    /// The names Heron may run, as the core last reported them.
    public private(set) var allowed: Set<String> = []
    /// Names on the allowlist that Shortcuts no longer lists (renamed or deleted). Heron can't run them,
    /// and the next save drops them, because the core refuses a name it can't find.
    public private(set) var missing: [String] = []
    public private(set) var isSaving = false
    /// The last save that failed, in words the owner can read. Cleared by the next success or load.
    public private(set) var errorMessage: String?

    public init(client: any ShortcutsClientProtocol) {
        self.client = client
    }

    public func isAllowed(_ name: String) -> Bool { allowed.contains(name) }

    // MARK: Loading

    public func load() async {
        phase = .loading
        errorMessage = nil
        do {
            apply(try await client.shortcuts())
        } catch ShortcutsError.featureOff {
            clear()
            phase = .featureOff
        } catch {
            clear()
            phase = .failed(describe(error))
        }
    }

    private func apply(_ list: ShortcutsList) {
        shortcuts = list.shortcuts
        allowed = Set(list.allowed)
        missing = list.allowed.filter { !list.shortcuts.contains($0) }
        phase = list.available ? .ready : .unavailable
    }

    private func clear() {
        shortcuts = []
        allowed = []
        missing = []
    }

    // MARK: Ticking

    /// Ticks or unticks one shortcut. Returns whether the core saved the change.
    @discardableResult
    public func set(_ name: String, allowed on: Bool) async -> Bool {
        guard phase == .ready, !isSaving, shortcuts.contains(name), allowed.contains(name) != on else { return false }
        var next = allowed
        if on { next.insert(name) } else { next.remove(name) }
        // Only names Shortcuts lists are sent, in its order, so a stale name can't make the core refuse the save.
        let names = shortcuts.filter { next.contains($0) }
        isSaving = true
        defer { isSaving = false }
        do {
            let saved = try await client.setAllowed(names)
            allowed = Set(saved)
            missing = saved.filter { !shortcuts.contains($0) }
            errorMessage = nil
            return allowed.contains(name) == on
        } catch ShortcutsError.featureOff {
            clear()
            phase = .featureOff
            return false
        } catch ShortcutsError.unknownShortcut(let message) {
            // The list is out of date: say so, then read it again so the owner ticks from what Shortcuts has now.
            await load()
            errorMessage = message
            return false
        } catch {
            errorMessage = describe(error)
            return false
        }
    }
}

/// Kept out of `ShortcutsPickerStore` so reading it needs no main actor: `FeatureRows.all` is a plain static.
public enum ShortcutsFeature {
    /// F5's row in Settings › Features: the core key `shortcuts`. It needs no macOS permission for Heron
    /// (heron-v1-features.md, F5: "TCC: none"); each shortcut asks for its own inside Shortcuts.
    public static let row = FeatureRow(key: "shortcuts", title: "Run your Shortcuts", kind: .core(key: "shortcuts"))
}

/// The picker under the Features list: a checkbox for each shortcut, or a line saying why there is none.
struct ShortcutsPickerView: View {
    let store: ShortcutsPickerStore
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel("Shortcuts Heron may run")
            content
            if let message = store.errorMessage {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
            }
        }
        .task { await store.load() }
    }

    @ViewBuilder private var content: some View {
        switch store.phase {
        case .idle, .loading:
            Text("Loading your shortcuts…").font(HeronFont.callout).foregroundStyle(theme.ink2)
        case .featureOff:
            note("Run your Shortcuts is switched off. Switch it on above to choose shortcuts.")
        case .unavailable:
            note("Shortcuts isn't available on this Mac, so Heron can't run any.")
        case .failed(let message):
            note("Heron's core didn't answer, so the list is unavailable. \(message)", color: theme.danger)
        case .ready:
            if store.shortcuts.isEmpty {
                note("You have no shortcuts yet. Make one in the Shortcuts app, then come back.")
            } else {
                Text("Heron runs only the ones you tick, and asks you each time.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
                RowGroup {
                    ForEach(Array(store.shortcuts.enumerated()), id: \.element) { i, name in
                        if i > 0 { RowDivider() }
                        HCheckbox(isOn: Binding(get: { store.isAllowed(name) }, set: { v in Task { await store.set(name, allowed: v) } }),
                                  label: name)
                            .disabled(store.isSaving)
                            .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            if !store.missing.isEmpty {
                note("No longer in Shortcuts: \(store.missing.joined(separator: ", ")). Heron can't run \(store.missing.count == 1 ? "it" : "them"), and your next change removes \(store.missing.count == 1 ? "it" : "them").")
            }
        }
    }

    private func note(_ text: String, color: Color? = nil) -> some View {
        Text(text).font(HeronFont.callout).foregroundStyle(color ?? theme.ink2).fixedSize(horizontal: false, vertical: true)
    }
}
