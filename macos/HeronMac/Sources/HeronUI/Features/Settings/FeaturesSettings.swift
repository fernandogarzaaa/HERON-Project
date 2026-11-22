import SwiftUI
import AppKit
import Observation
import HeronCore

/// One line of the list: a row, and what it shows now.
public struct FeatureItem: Identifiable, Equatable, Sendable {
    public var row: FeatureRow
    public var on: Bool
    /// False while a core row's state is unknown (the core hasn't answered), so its switch can't be flipped blind.
    public var available: Bool
    /// True after macOS refused the permission this row needs; the row names it and links to its pane.
    public var denied: Bool
    public var id: String { row.id }
}

/// Settings › Features: the app's own switches and the core's keys in one list.
///
/// App rows read and write `UserDefaults`. Core rows read `GET /api/features` and write `POST /api/features/{key}`.
/// A core key with no row in `FeatureRows.all` still shows, under the title the core gave it.
/// Turning a row on first asks for the macOS permission it names; a refusal leaves it off.
@MainActor
@Observable
public final class FeaturesSettingsStore {
    public let client: any FeaturesClientProtocol
    public let rows: [FeatureRow]
    public private(set) var coreFeatures: [CoreFeature] = []
    public private(set) var loadState: LoadState = .idle
    /// The last switch that failed, in words the owner can read. Cleared by the next success.
    public private(set) var errorMessage: String?
    /// Keys of rows whose permission macOS refused.
    public private(set) var deniedKeys: Set<String> = []

    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let poster: any NotificationPoster
    @ObservationIgnored private let permissionRequests: [String: @Sendable () async -> Bool]
    private var appState: [String: Bool] = [:]

    /// - Parameters:
    ///   - permissionRequests: how to ask for each non-notification permission, by the name a row gives. A feature's
    ///     `a` unit registers its own. A permission with no entry here is not asked for.
    public init(client: any FeaturesClientProtocol,
                rows: [FeatureRow] = FeatureRows.all,
                defaults: UserDefaults = .standard,
                poster: any NotificationPoster = SystemNotificationPoster(),
                permissionRequests: [String: @Sendable () async -> Bool] = [:]) {
        self.client = client
        self.rows = rows
        self.defaults = defaults
        self.poster = poster
        self.permissionRequests = permissionRequests
        for row in rows { if case .app(let defaultsKey) = row.kind { appState[row.key] = defaults.bool(forKey: defaultsKey) } }
    }

    // MARK: Reading

    /// The list in order: the rows from `FeatureRows`, then core keys that have no row of their own.
    public var items: [FeatureItem] {
        var out = rows.map { item(for: $0) }
        var known: Set<String> = []
        for row in rows { if case .core(let key) = row.kind { known.insert(key) } }
        for feature in coreFeatures where !known.contains(feature.key) {
            out.append(FeatureItem(row: FeatureRow(key: feature.key, title: feature.title, kind: .core(key: feature.key)),
                                   on: feature.on, available: true, denied: deniedKeys.contains(feature.key)))
        }
        return out
    }

    private func item(for row: FeatureRow) -> FeatureItem {
        switch row.kind {
        case .app:
            return FeatureItem(row: row, on: appState[row.key] ?? false, available: true, denied: deniedKeys.contains(row.key))
        case .core(let key):
            let known = coreFeatures.first { $0.key == key }
            return FeatureItem(row: row, on: known?.on ?? false, available: known != nil, denied: deniedKeys.contains(row.key))
        }
    }

    /// Whether the core says `key` is on. False while the core hasn't answered or doesn't list it.
    public func isOn(coreKey key: String) -> Bool {
        coreFeatures.first { $0.key == key }?.on ?? false
    }

    // MARK: Loading and switching

    public func load() async {
        loadState = .loading
        do {
            coreFeatures = try await client.features()
            loadState = .loaded
        } catch {
            coreFeatures = []
            loadState = .failed(describe(error))
        }
    }

    /// Switches a row. Turning it on asks for its permission first; turning it off never does.
    /// Returns whether the row now has the state asked for.
    @discardableResult
    public func set(_ row: FeatureRow, on: Bool) async -> Bool {
        let current = items.first { $0.row.key == row.key }
        guard current?.available ?? true else { return false }
        if on {
            guard await permissionGranted(for: row) else { deniedKeys.insert(row.key); return false }
        }
        deniedKeys.remove(row.key)
        switch row.kind {
        case .app(let defaultsKey):
            defaults.set(on, forKey: defaultsKey)
            appState[row.key] = on
            errorMessage = nil
            return true
        case .core(let key):
            do {
                let saved = try await client.setFeature(key: key, on: on)
                if let i = coreFeatures.firstIndex(where: { $0.key == key }) { coreFeatures[i].on = saved }
                errorMessage = nil
                return saved == on
            } catch {
                errorMessage = describe(error)
                return false
            }
        }
    }

    private func permissionGranted(for row: FeatureRow) async -> Bool {
        guard let permission = row.permission else { return true }
        if permission == FeatureRow.notificationsPermission { return await poster.requestAuthorization() }
        guard let request = permissionRequests[permission] else { return true }
        return await request()
    }
}

/// The Features tab: one `RowGroup` of switches. A denied row names its permission and opens its System Settings pane.
struct FeaturesSettingsView: View {
    let store: FeaturesSettingsStore
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s4) {
            Text("Turn on only what you want. Each one that needs a macOS permission asks when you switch it on.")
                .font(HeronFont.callout).foregroundStyle(theme.ink2).fixedSize(horizontal: false, vertical: true)
            if case .failed(let message) = store.loadState {
                Text("Heron's core didn't answer, so its switches are unavailable. \(message)")
                    .font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
            }
            let items = store.items
            if items.isEmpty {
                Text(store.loadState == .loading ? "Loading…" : "No optional features yet.")
                    .font(HeronFont.callout).foregroundStyle(theme.ink2)
            } else {
                RowGroup {
                    ForEach(Array(items.enumerated()), id: \.element.id) { i, item in
                        if i > 0 { RowDivider() }
                        FeatureRowView(store: store, item: item)
                    }
                }
            }
            if let message = store.errorMessage {
                Text(message).font(HeronFont.callout).foregroundStyle(theme.danger).fixedSize(horizontal: false, vertical: true)
            }
        }
        .task { await store.load() }
    }
}

private struct FeatureRowView: View {
    let store: FeaturesSettingsStore
    let item: FeatureItem
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(alignment: .top, spacing: Spacing.s3) {
            VStack(alignment: .leading, spacing: 2) {
                Text(item.row.title).font(HeronFont.body.weight(.semibold)).foregroundStyle(theme.ink)
                if let permission = item.row.permission {
                    Text(item.denied ? "macOS refused \(permission). Allow it in System Settings, then switch this on again."
                                     : "Needs \(permission).")
                        .font(HeronFont.callout).foregroundStyle(item.denied ? theme.danger : theme.ink2)
                        .fixedSize(horizontal: false, vertical: true)
                    if item.denied, let pane = item.row.settingsPane {
                        Button("Open System Settings") { NSWorkspace.shared.open(pane) }.buttonStyle(HButtonStyle(.secondary))
                    }
                }
            }
            Spacer()
            Toggle(item.row.title, isOn: Binding(get: { item.on }, set: { v in Task { await store.set(item.row, on: v) } }))
                .labelsHidden().disabled(!item.available)
        }
        .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
        .accessibilityElement(children: .contain)
    }
}
