import XCTest
import HeronCore
@testable import HeronUI

@MainActor
final class SandboxSettingsTests: XCTestCase {
    private let changed = ToolchainStatus(state: .changed, toolchains: ["stable-x"], files: ["bin/rustc", "bin/cargo"], since: 100,
                                          during: "cargo install --path .")

    private func loadedStore(toolchain: ToolchainStatus? = nil) async -> (SettingsStore, PreviewCoreClient) {
        let client = PreviewCoreClient()
        if let toolchain { client.setToolchain(toolchain) }
        let store = SettingsStore(client: client)
        await store.load()
        return (store, client)
    }

    func test_sandboxSettingsShowsChangedToolchain() async throws {
        let (store, _) = await loadedStore(toolchain: changed)
        let status = try XCTUnwrap(store.toolchain)
        XCTAssertEqual(status.state, .changed)
        XCTAssertEqual(SandboxCopy.toolchainLine(status),
                       "The Rust toolchain changed while “cargo install --path .” was running: bin/rustc, bin/cargo. Until you trust it again, sandboxed commands that run it ask first.")
        let (trustedStore, _) = await loadedStore()
        XCTAssertEqual(trustedStore.toolchain?.state, .trusted)
        XCTAssertEqual(SandboxCopy.toolchainLine(try XCTUnwrap(trustedStore.toolchain)), "The Rust toolchain is the one you trusted.")
    }

    func test_trustToolchainCallsCore() async throws {
        let (store, client) = await loadedStore(toolchain: changed)
        await store.trustToolchain()
        XCTAssertEqual(client.toolchainTrustRequests, [100], "the since the screen showed goes to the core")
        XCTAssertEqual(store.toolchain?.state, .trusted)
        XCTAssertNil(store.toolchainError)
        await store.trustToolchain()
        XCTAssertEqual(client.toolchainTrustRequests, [100], "a trusted toolchain has nothing to trust")
    }

    func test_trustRefusedAfterAnotherChangeShowsTheNewerChange() async throws {
        let (store, client) = await loadedStore(toolchain: changed)
        var newer = changed
        newer.since = 200
        newer.files = ["bin/rustdoc"]
        client.setToolchain(newer)
        await store.trustToolchain()
        XCTAssertEqual(store.toolchainError, "The toolchain changed again since you looked. Check it again before you trust it.")
        XCTAssertEqual(store.toolchain, newer, "the page now shows what changed last")
        await store.trustToolchain()
        XCTAssertEqual(client.toolchainTrustRequests, [100, 200])
        XCTAssertEqual(store.toolchain?.state, .trusted)
    }

    func test_egressHostListRoundTrips() async throws {
        let (store, client) = await loadedStore()
        let before = try XCTUnwrap(store.settings?.egressAllow)
        let added = await store.addEgressHost("  *.example.com ")
        XCTAssertTrue(added)
        XCTAssertEqual(store.settings?.egressAllow, before + ["*.example.com"])
        XCTAssertEqual(client.sandboxRequests.last?.hosts, before + ["*.example.com"])
        let reloaded = try await client.settings()
        XCTAssertEqual(reloaded.egressAllow, before + ["*.example.com"], "the core holds it")
        await store.removeEgressHost("*.example.com")
        XCTAssertEqual(store.settings?.egressAllow, before)
        let afterRemoval = try await client.settings().egressAllow
        XCTAssertEqual(afterRemoval, before)
    }

    func test_sandboxChangesKeepTheOtherValues() async throws {
        let (store, client) = await loadedStore()
        let hosts = try XCTUnwrap(store.settings?.egressAllow)
        await store.addDeniedPort("5432")
        XCTAssertEqual(client.sandboxRequests.last, PreviewSandboxRequest(egress: "allowlist", hosts: hosts, denyPorts: [5432], maxRepeats: 4))
        await store.setEgress("open")
        XCTAssertEqual(client.sandboxRequests.last, PreviewSandboxRequest(egress: "open", hosts: hosts, denyPorts: [5432], maxRepeats: 4))
        XCTAssertEqual(store.settings?.egress, "open")
        await store.removeDeniedPort(5432)
        XCTAssertEqual(store.settings?.denyLocalPorts, [])
    }

    func test_refusedHostShowsTheCoresSentenceAndChangesNothing() async throws {
        let (store, _) = await loadedStore()
        let before = store.settings?.egressAllow
        let added = await store.addEgressHost("not a host")
        XCTAssertFalse(added)
        XCTAssertEqual(store.sandboxError, "Not a host name: not a host.")
        XCTAssertEqual(store.settings?.egressAllow, before)
        _ = await store.addEgressHost("github.com")
        XCTAssertEqual(store.sandboxError, "github.com is already on the list.")
    }

    func test_badPortIsRefusedBeforeAnyRequest() async throws {
        let (store, client) = await loadedStore()
        for text in ["0", "65536", "http", ""] {
            let added = await store.addDeniedPort(text)
            XCTAssertFalse(added, text)
        }
        XCTAssertEqual(store.sandboxError, "Enter a port from 1 to 65535.")
        XCTAssertTrue(client.sandboxRequests.isEmpty)
    }

    func test_toolchainRouteFailureLeavesTheRestOfSettingsLoaded() async throws {
        let client = PreviewCoreClient()
        client.failNextToolchainRead(with: .failed("The toolchain check stopped."))
        let store = SettingsStore(client: client)
        await store.load()
        XCTAssertEqual(store.loadState, .loaded)
        XCTAssertNil(store.toolchain)
        XCTAssertEqual(store.toolchainError, "The toolchain check stopped.")
    }
}
