import XCTest
import SwiftUI
import HeronCore
@testable import HeronUI

/// Snapshots of Settings › Connections, the disconnect confirm, and of the first-run sources step (Dusk, light and dark). The email and calendar sheets are not snapshotted: `ImageRenderer` draws a text field as a placeholder.
@MainActor
final class ConnectionsSnapshotTests: XCTestCase {
    private func model(seed: (PreviewCoreClient) -> Void = { _ in }) async -> (AppModel, PreviewCoreClient) {
        let client = PreviewCoreClient()
        seed(client)
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults), userFirstName: "Sam")
        await model.settings.load()
        return (model, client)
    }

    func testConnectionsTabListsEverySourceWithItsActionLight() async throws {
        let (model, _) = await model()
        XCTAssertEqual(model.settings.settings?.connections.map(\.id), ["email", "calendar", "documents", "computer", "calendar_local"])
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-light", size: WindowMinimum.settingsPage, dark: false)
    }

    func testConnectionsTabListsEverySourceWithItsActionDark() async throws {
        let (model, _) = await model()
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-dark", size: WindowMinimum.settingsPage, dark: true)
    }

    func testConnectionsTabWhenThisBuildHasNoGoogleClientLight() async throws {
        let (model, _) = await model { $0.setOAuthConfigured(.google, false) }
        XCTAssertFalse(model.settings.account(.google).clientConfigured)
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-not-configured-light", size: WindowMinimum.settingsPage, dark: false)
    }

    func testConnectionsTabWithAGoogleAccountConnectedLight() async throws {
        let (model, client) = await model()
        client.completeSignIn(afterPolls: 1, address: "sam@gmail.com")
        await model.settings.beginSignIn(.google) { _ in true }.value
        XCTAssertEqual(model.settings.account(.google).account, "sam@gmail.com")
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-google-connected-light", size: WindowMinimum.settingsPage, dark: false)
    }

    func testConnectionsTabWhileWaitingForTheBrowserLight() async throws {
        let (model, client) = await model()
        client.completeSignIn(afterPolls: nil)
        let store = SettingsStore(client: client, localCalendar: SystemCalendarAccess(), sleep: { _ in try await Task.sleep(for: .seconds(60)) })
        await store.load()
        let waiting = store.beginSignIn(.google) { _ in true }
        for _ in 0..<200 where store.signingIn == nil { await Task.yield() }
        XCTAssertEqual(store.signingIn, .google)
        let page = ConnectionsPanel(store: store).padding(Spacing.s6)
        try assertSnapshot(page, name: "connections-waiting-light", size: CGSize(width: 720, height: 0), dark: false)
        store.cancelSignIn()
        await waiting.value
        _ = model
    }

    func testDisconnectConfirmLight() throws {
        let row = ConnectionInfo(id: "email", name: "Email", connected: true, detail: "sam@northwind.example")
        try assertSnapshot(SourceDisconnectConfirmView(source: row, onCancel: {}, onConfirm: {}), name: "connections-disconnect-light",
                           size: CGSize(width: 480, height: 0), dark: false)
    }

    func testSourcesStepOfFirstRunLight() async throws {
        let (model, _) = await model()
        await model.runSetupAgain()
        model.firstRun.step = .sources
        try assertScrollingSnapshot(FirstRunView(model: model), name: "first-run-sources-light", size: WindowMinimum.firstRun, dark: false)
    }
}
