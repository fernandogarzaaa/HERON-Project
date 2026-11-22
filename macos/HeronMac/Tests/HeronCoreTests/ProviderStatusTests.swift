import XCTest
@testable import HeronCore

final class ProviderStatusTests: XCTestCase {
    private func provider(version: String?, testedRange: String?, experimental: Bool, authKind: String = "API key") -> ProviderStatus {
        ProviderStatus(id: "p", displayName: "P", installed: true, version: version, testedRange: testedRange,
                       inRange: true, loggedIn: true, experimental: experimental, versionAccepted: false,
                       loginCommand: "p login", authKind: authKind, isActive: false)
    }

    func testExperimentalProviderIsLabelledAndHidesTestedRange() {
        let p = provider(version: "2.1.4", testedRange: ">=2.1.4 <3.0.0", experimental: true)
        XCTAssertEqual(p.signedInLine, "Version 2.1.4 · experimental, not tested yet · API key")
        XCTAssertFalse(p.signedInLine.contains("tested range"))
    }

    func testExperimentalProviderWithoutRangeIsStillLabelled() {
        let p = provider(version: "2.1.4", testedRange: nil, experimental: true)
        XCTAssertEqual(p.signedInLine, "Version 2.1.4 · experimental, not tested yet · API key")
    }

    func testNonExperimentalProviderKeepsTestedRangeWording() {
        let p = provider(version: "2.1.4", testedRange: "2.0 to 2.1", experimental: false, authKind: "subscription login")
        XCTAssertEqual(p.signedInLine, "Version 2.1.4 · tested range 2.0 to 2.1 · subscription login")
    }

    func testNonExperimentalProviderWithoutRangeOmitsMiddlePart() {
        let p = provider(version: "2.1.4", testedRange: nil, experimental: false)
        XCTAssertEqual(p.signedInLine, "Version 2.1.4 · API key")
    }

    func testNilVersionReadsUnknown() {
        XCTAssertEqual(provider(version: nil, testedRange: "2.0 to 2.1", experimental: false).signedInLine,
                       "Version unknown · tested range 2.0 to 2.1 · API key")
        XCTAssertEqual(provider(version: nil, testedRange: nil, experimental: true).signedInLine,
                       "Version unknown · experimental, not tested yet · API key")
    }
}
