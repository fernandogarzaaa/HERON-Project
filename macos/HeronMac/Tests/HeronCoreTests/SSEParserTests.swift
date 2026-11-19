import XCTest
@testable import HeronCore

final class SSEParserTests: XCTestCase {
    func testSingleEventWithIDAndData() {
        var parser = SSEParser()
        let out = parser.feed("id: 7\ndata: {\"seq\":7}\n\n")
        XCTAssertEqual(out, [SSEMessage(data: "{\"seq\":7}", id: "7")])
    }

    func testMultiLineDataIsJoinedWithNewlines() {
        var parser = SSEParser()
        let out = parser.feed("data: first\ndata: second\ndata:\ndata: fourth\n\n")
        XCTAssertEqual(out.count, 1)
        XCTAssertEqual(out.first?.data, "first\nsecond\n\nfourth")
    }

    func testKeepAliveCommentsEmitNothing() {
        var parser = SSEParser()
        // The core's keep-alive is a comment line and a blank line.
        XCTAssertEqual(parser.feed(":\n\n"), [])
        XCTAssertEqual(parser.feed(": keep-alive\n\n"), [])
        // A comment inside an event doesn't break it.
        let out = parser.feed("data: a\n: ping\ndata: b\n\n")
        XCTAssertEqual(out.map(\.data), ["a\nb"])
    }

    func testIDPersistsUntilReplaced() {
        var parser = SSEParser()
        let out = parser.feed("id: 1\ndata: a\n\ndata: b\n\nid: 3\ndata: c\n\n")
        XCTAssertEqual(out.map(\.id), ["1", "1", "3"])
    }

    func testAnEventSplitAcrossChunksIsAssembledOnce() {
        var parser = SSEParser()
        let text = "id: 12\ndata: {\"type\":\"text\",\"text\":\"héllo\"}\n\n"
        let bytes = Array(text.utf8)
        var out: [SSEMessage] = []
        // One byte at a time, which also splits the two-byte "é".
        for byte in bytes { out += parser.feed(bytes: [byte]) }
        XCTAssertEqual(out, [SSEMessage(data: "{\"type\":\"text\",\"text\":\"héllo\"}", id: "12")])
    }

    func testCRLFAndCRLineEndings() {
        var parser = SSEParser()
        XCTAssertEqual(parser.feed("data: a\r\n\r\n").map(\.data), ["a"])
        XCTAssertEqual(parser.feed("data: b\r\r").map(\.data), ["b"])
        // A CRLF split between chunks is one line ending, not two (which would dispatch early).
        var split = SSEParser()
        XCTAssertEqual(split.feed("data: c\r"), [])
        XCTAssertEqual(split.feed("\ndata: d\r\n\r\n").map(\.data), ["c\nd"])
    }

    func testOnlyOneLeadingSpaceIsDropped() {
        var parser = SSEParser()
        XCTAssertEqual(parser.feed("data:  two spaces\n\n").first?.data, " two spaces")
        XCTAssertEqual(parser.feed("data:none\n\n").first?.data, "none")
    }

    func testBlankLineWithoutDataDispatchesNothing() {
        var parser = SSEParser()
        XCTAssertEqual(parser.feed("id: 5\n\n"), [])
        XCTAssertEqual(parser.feed("event: x\n\n"), [])
    }

    func testEventNameAndRetry() {
        var parser = SSEParser()
        let out = parser.feed("event: tick\nretry: 3000\ndata: x\n\nretry: nope\ndata: y\n\n")
        XCTAssertEqual(out.first?.event, "tick")
        XCTAssertEqual(out.first?.retry, 3000)
        XCTAssertNil(out.last?.event, "the event name resets after each event")
        XCTAssertNil(out.last?.retry)
    }

    func testAnUnfinishedEventAtTheEndIsNotEmitted() {
        var parser = SSEParser()
        XCTAssertEqual(parser.feed("data: never finished\n"), [])
    }

    func testManyEventsInOneChunk() {
        var parser = SSEParser()
        let out = parser.feed("id: 1\ndata: a\n\nid: 2\ndata: b\n\nid: 3\ndata: c\n\n")
        XCTAssertEqual(out.map(\.data), ["a", "b", "c"])
    }
}
