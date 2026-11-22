import XCTest
import SwiftUI
import HeronCore
@testable import HeronUI

/// Snapshots of Settings › Permissions › Never (heron-v1.md item 35h), light and dark Dusk: a short list, a long
/// list at the Settings window's fixed size, the confirm sheet, and an inline error.
///
/// The short list, the sheet and the error render through `ImageRenderer` like the other component snapshots.
/// The long list renders through an `NSHostingView` (item 41b), because it has to show the page's one scroll view.
@MainActor
final class NeverSnapshotTests: XCTestCase {
    private func loadedStore(seed: (PreviewCoreClient) -> Void = { _ in }) async -> (SettingsStore, PreviewCoreClient) {
        let client = PreviewCoreClient()
        seed(client)
        let store = SettingsStore(client: client)
        await store.load()
        return (store, client)
    }

    /// The section as it sits on the Permissions page: the page's padding, 720 pt wide.
    private func page(_ store: SettingsStore) -> some View {
        NeverSection(store: store).padding(Spacing.s6)
    }

    private let sectionSize = CGSize(width: 720, height: 0)

    // MARK: A short list

    func testShortListShowsEveryRuleWithRemoveLight() async throws {
        let (store, _) = await loadedStore()
        XCTAssertEqual(store.nevers.count, 3)
        try assertSnapshot(page(store), name: "never-short-light", size: sectionSize, dark: false)
    }

    func testShortListShowsEveryRuleWithRemoveDark() async throws {
        let (store, _) = await loadedStore()
        try assertSnapshot(page(store), name: "never-short-dark", size: sectionSize, dark: true)
    }

    // MARK: A long list at the smallest window

    private func permissionsPage(_ client: PreviewCoreClient) async -> AppModel {
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults), userFirstName: "Sam")
        await model.settings.load()
        return model
    }

    func testLongListScrollsWithThePageAtTheSettingsWindowSizeLight() async throws {
        let client = PreviewCoreClient()
        client.seedLongNevers()
        let model = await permissionsPage(client)
        XCTAssertEqual(model.settings.nevers.count, PreviewDemo.longNevers.count)
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .permissions), name: "never-long-light", size: WindowMinimum.settingsPage, dark: false)
    }

    func testLongListScrollsWithThePageAtTheSettingsWindowSizeDark() async throws {
        let client = PreviewCoreClient()
        client.seedLongNevers()
        let model = await permissionsPage(client)
        try assertScrollingSnapshot(SettingsTabContent(model: model, tab: .permissions), name: "never-long-dark", size: WindowMinimum.settingsPage, dark: true)
    }

    /// The same long list inside the page's scroll view alone, so the picture shows the rows themselves running
    /// past the scroll edge with nothing else above them.
    func testLongListRowsRunPastTheScrollEdgeLight() async throws {
        let (store, _) = await loadedStore { $0.seedLongNevers() }
        try assertScrollingSnapshot(HScroll { page(store) }, name: "never-long-rows-light", size: WindowMinimum.settingsPage, dark: false)
    }

    // MARK: The confirm sheet

    private let sheetRule = NeverRule(rule: "shell:*sudo*", tool: "shell", inFile: true)

    func testConfirmSheetSaysWhatHeronWillBeAllowedToDoLight() throws {
        try assertSnapshot(NeverRemoveConfirmView(rule: sheetRule, onCancel: {}, onConfirm: {}), name: "never-confirm-light",
                           size: CGSize(width: 480, height: 0), dark: false)
    }

    func testConfirmSheetSaysWhatHeronWillBeAllowedToDoDark() throws {
        try assertSnapshot(NeverRemoveConfirmView(rule: sheetRule, onCancel: {}, onConfirm: {}), name: "never-confirm-dark",
                           size: CGSize(width: 480, height: 0), dark: true)
    }

    // MARK: A failed removal

    private func failedRemoval() async -> SettingsStore {
        let (store, client) = await loadedStore()
        client.failNextNeverRemoval(with: .failed("Heron couldn't update config.toml, so the rule is still in force."))
        store.askToRemoveNever(store.nevers[1])
        await store.confirmNeverRemoval()
        XCTAssertNotNil(store.neverError)
        XCTAssertEqual(store.nevers.count, 3, "the rule is still listed")
        return store
    }

    func testFailedRemovalShowsTheErrorInlineLight() async throws {
        try assertSnapshot(page(await failedRemoval()), name: "never-error-light", size: sectionSize, dark: false)
    }

    func testFailedRemovalShowsTheErrorInlineDark() async throws {
        try assertSnapshot(page(await failedRemoval()), name: "never-error-dark", size: sectionSize, dark: true)
    }
}
