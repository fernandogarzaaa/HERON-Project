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
        XCTAssertEqual(model.settings.settings?.connections.map(\.id), ["email", "calendar", "documents", "computer"])
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-light", size: WindowMinimum.settingsPage, dark: false)
    }

    func testConnectionsTabListsEverySourceWithItsActionDark() async throws {
        let (model, _) = await model()
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .connections), name: "connections-tab-dark", size: WindowMinimum.settingsPage, dark: true)
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
