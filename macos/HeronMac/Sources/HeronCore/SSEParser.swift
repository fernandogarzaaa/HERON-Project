import Foundation

/// One server-sent event: the lines between two blank lines, per the WHATWG EventSource rules.
public struct SSEMessage: Sendable, Equatable {
    /// `event:` field, or nil. The core sends none: the event type lives in the JSON.
    public var event: String?
    /// All `data:` lines joined by `\n`.
    public var data: String
    /// `id:` field. The core sets it to the event's `seq`; it persists across messages until replaced.
    public var id: String?
    /// `retry:` field in milliseconds, when valid.
    public var retry: Int?

    public init(event: String? = nil, data: String, id: String? = nil, retry: Int? = nil) {
        self.event = event
        self.data = data
        self.id = id
        self.retry = retry
    }
}

/// Incremental parser for `text/event-stream`.
///
/// Feed it raw bytes in chunks of any size: a line, an event or a single byte may arrive split across
/// calls. It deliberately works on bytes, not on `URLSession.AsyncBytes.lines`, because that sequence
/// drops empty lines and the empty line is what ends an event.
///
/// Rules (HTML Living Standard, "Server-sent events"):
/// * lines end in `\n`, `\r\n` or `\r`;
/// * a line starting with `:` is a comment (the core's 15-second keep-alive) and changes nothing;
/// * `field: value` drops one leading space from the value; a line with no colon is a field with an empty value;
/// * several `data:` lines are joined with `\n`;
/// * a blank line dispatches the event, unless no `data:` line came since the last dispatch;
/// * an event still open when the stream ends is discarded.
public struct SSEParser: Sendable {
    private var line: [UInt8] = []
    /// A `\r` was the last byte, so a following `\n` belongs to the same line ending.
    private var skipLineFeed = false
    private var dataLines: [String] = []
    private var eventName: String?
    private var lastID: String?
    private var retry: Int?

    public init() {}

    /// Feeds a chunk and returns every event it completed, in order.
    public mutating func feed(_ chunk: Data) -> [SSEMessage] {
        feed(bytes: chunk)
    }

    public mutating func feed(_ text: String) -> [SSEMessage] {
        feed(bytes: Array(text.utf8))
    }

    public mutating func feed<S: Sequence>(bytes: S) -> [SSEMessage] where S.Element == UInt8 {
        var out: [SSEMessage] = []
        for byte in bytes {
            if skipLineFeed {
                skipLineFeed = false
                if byte == 0x0A { continue }
            }
            switch byte {
            case 0x0A:
                if let message = endOfLine() { out.append(message) }
            case 0x0D:
                skipLineFeed = true
                if let message = endOfLine() { out.append(message) }
            default:
                line.append(byte)
            }
        }
        return out
    }

    /// Handles one complete line. Returns the event a blank line dispatched, if any.
    private mutating func endOfLine() -> SSEMessage? {
        defer { line.removeAll(keepingCapacity: true) }
        if line.isEmpty { return dispatch() }
        if line[0] == 0x3A { return nil } // ":" comment
        let field: String
        var value: String
        if let colon = line.firstIndex(of: 0x3A) {
            field = String(decoding: line[..<colon], as: UTF8.self)
            var valueBytes = line[(colon + 1)...]
            if valueBytes.first == 0x20 { valueBytes = valueBytes.dropFirst() }
            value = String(decoding: valueBytes, as: UTF8.self)
        } else {
            field = String(decoding: line, as: UTF8.self)
            value = ""
        }
        switch field {
        case "data": dataLines.append(value)
        case "event": eventName = value
        case "id": if !value.contains("\0") { lastID = value }
        case "retry": if !value.isEmpty, value.allSatisfy(\.isASCII), let ms = Int(value) { retry = ms }
        default: break
        }
        return nil
    }

    private mutating func dispatch() -> SSEMessage? {
        defer {
            dataLines.removeAll()
            eventName = nil
            retry = nil
        }
        guard !dataLines.isEmpty else { return nil }
        return SSEMessage(event: eventName, data: dataLines.joined(separator: "\n"), id: lastID, retry: retry)
    }
}
