import Foundation

/// Cuts a reply that arrives in chunks (the `text` events) into sentences, as each one completes.
///
/// A sentence ends at `.`, `!`, `?` or `…` followed by white space (closing quotes and brackets may sit between),
/// or at a line break. A terminator at the very end of the buffer is held back: the next chunk may continue the
/// word ("3" then ".5"). `flush()` hands over what is left when the reply ends.
///
/// What gets spoken is the sentence with its Markdown removed. Fenced code blocks are skipped, since code read
/// aloud is noise.
struct SentenceSplitter {
    private var buffer: [Character] = []
    private var inFence = false

    private static let terminators: Set<Character> = [".", "!", "?", "…"]
    private static let closers: Set<Character> = ["\"", "'", "”", "’", ")", "]", "*", "_"]
    private static let abbreviations: Set<String> = ["mr", "mrs", "ms", "dr", "vs", "e.g", "i.e"]

    mutating func append(_ chunk: String) -> [String] {
        buffer.append(contentsOf: chunk)
        var out: [String] = []
        var start = 0
        var i = 0
        while i < buffer.count {
            let c = buffer[i]
            if c == "\n" {
                emit(buffer[start..<i], into: &out)
                start = i + 1
                i = start
                continue
            }
            if Self.terminators.contains(c) {
                var j = i + 1
                while j < buffer.count, Self.closers.contains(buffer[j]) { j += 1 }
                if j == buffer.count { break }  // wait for the next character
                if buffer[j].isWhitespace, isBoundary(at: i, segmentStart: start) {
                    emit(buffer[start..<j], into: &out)
                    start = j
                    i = j
                    continue
                }
                i = j
                continue
            }
            i += 1
        }
        buffer.removeFirst(start)
        return out
    }

    /// The reply is over: whatever is left is the last sentence.
    mutating func flush() -> [String] {
        var out: [String] = []
        emit(buffer[...], into: &out)
        buffer = []
        inFence = false
        return out
    }

    /// "1." opening a list item and abbreviations such as "Dr." don't end a sentence.
    private func isBoundary(at index: Int, segmentStart: Int) -> Bool {
        guard buffer[index] == "." else { return true }
        let head = String(buffer[segmentStart..<index]).trimmingCharacters(in: .whitespaces)
        if !head.isEmpty, head.allSatisfy(\.isNumber) { return false }
        let lastWord = head.split(whereSeparator: { $0.isWhitespace }).last.map { $0.lowercased() } ?? ""
        return !Self.abbreviations.contains(lastWord)
    }

    private mutating func emit(_ raw: ArraySlice<Character>, into out: inout [String]) {
        let text = String(raw)
        if text.trimmingCharacters(in: .whitespaces).hasPrefix("```") { inFence.toggle(); return }
        if inFence { return }
        if let spoken = Self.spoken(text) { out.append(spoken) }
    }

    /// The text as it is read aloud, or nil when nothing speakable is left.
    static func spoken(_ markdown: String) -> String? {
        var s = markdown
        s = s.replacingOccurrences(of: #"\[([^\]]*)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
        s = s.replacingOccurrences(of: #"https?://\S+"#, with: "link", options: .regularExpression)
        s = s.replacingOccurrences(of: #"^\s*(#{1,6}|[-*•>]+|\d{1,3}[.)])\s+"#, with: "", options: .regularExpression)
        for mark in ["`", "**", "__", "*"] { s = s.replacingOccurrences(of: mark, with: "") }
        s = s.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return s.contains(where: { $0.isLetter || $0.isNumber }) ? s : nil
    }
}
