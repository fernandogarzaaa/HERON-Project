import XCTest
import AppKit
import HeronCore
@testable import HeronUI

/// Stands in for `AVSpeechSynthesizer`: records each sentence and finishes only when the test says so, which is
/// what makes "one sentence at a time" observable.
@MainActor
private final class FakeSpeaker: SpeechSynthesizing {
    var onFinish: (() -> Void)?
    private(set) var spoken: [String] = []
    private(set) var stops = 0

    func speak(_ sentence: String) { spoken.append(sentence) }
    func stopSpeaking() { stops += 1 }
    /// The synthesizer reached the end of the current sentence.
    func finishCurrent() { onFinish?() }
}

@MainActor
final class VoiceTests: XCTestCase {
    private func make(enabled: Bool = true, onScreen: @escaping (String) -> Bool = { _ in true }) -> (SpokenReplies, FakeSpeaker) {
        let speaker = FakeSpeaker()
        return (SpokenReplies(speaker: speaker, isEnabled: { enabled }, isOnScreen: onScreen), speaker)
    }

    private func text(_ chunk: String, session: String = "s1", seq: Int = 0) -> CoreEvent {
        CoreEvent(seq: seq, sessionID: session, runID: "r1", kind: .text(chunk))
    }

    private func done(_ text: String = "", session: String = "s1") -> CoreEvent {
        CoreEvent(sessionID: session, runID: "r1", kind: .done(text: text, title: nil))
    }

    private func keyDown() -> NSEvent {
        NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
                         characters: "a", charactersIgnoringModifiers: "a", isARepeat: false, keyCode: 0)!
    }

    private func mouse(_ type: NSEvent.EventType) -> NSEvent {
        NSEvent.mouseEvent(with: type, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
                           eventNumber: 0, clickCount: 1, pressure: 1)!
    }

    // MARK: One sentence at a time

    func test_speaksOneSentenceAtATime() {
        let (replies, speaker) = make()
        replies.handle(text("Hello the"))
        replies.handle(text("re. How are you? I a"))
        XCTAssertEqual(speaker.spoken, ["Hello there."], "the second sentence waits for the first to be spoken")
        speaker.finishCurrent()
        XCTAssertEqual(speaker.spoken, ["Hello there.", "How are you?"], "the unfinished third sentence is not spoken yet")
        replies.handle(text("m fine."))
        replies.handle(done())
        XCTAssertEqual(speaker.spoken.count, 2, "still speaking the second sentence")
        speaker.finishCurrent()
        XCTAssertEqual(speaker.spoken, ["Hello there.", "How are you?", "I am fine."], "the last sentence needs no terminator to be flushed")
    }

    func test_sentenceIsSpokenOnlyOnceItIsComplete() {
        let (replies, speaker) = make()
        replies.handle(text("Almost there"))
        XCTAssertEqual(speaker.spoken, [])
        replies.handle(text("."))
        XCTAssertEqual(speaker.spoken, [], "a full stop at the end of the buffer might still be followed by digits")
        replies.handle(text(" Next"))
        XCTAssertEqual(speaker.spoken, ["Almost there."])
    }

    func test_aReplyThatArrivesWholeInDoneIsSpoken() {
        let (replies, speaker) = make()
        replies.handle(done("All done. Bye."))
        XCTAssertEqual(speaker.spoken, ["All done."])
        speaker.finishCurrent()
        XCTAssertEqual(speaker.spoken, ["All done.", "Bye."])
    }

    func test_theSameEventFromTwoSurfacesIsSpokenOnce() {
        let (replies, speaker) = make()
        replies.handle(text("Once. ", seq: 7))
        replies.handle(text("Once. ", seq: 7))
        replies.handle(done())
        XCTAssertEqual(speaker.spoken, ["Once."])
        speaker.finishCurrent()
        XCTAssertEqual(speaker.spoken, ["Once."])
    }

    func test_splitterKeepsDecimalsAbbreviationsAndListNumbersInside() {
        var splitter = SentenceSplitter()
        let out = splitter.append("It costs 3.5 dollars. Dr. Smith agrees.\n1. First item\n")
        XCTAssertEqual(out, ["It costs 3.5 dollars.", "Dr. Smith agrees.", "First item"])
    }

    func test_markdownIsStrippedAndCodeBlocksAreSkipped() {
        var splitter = SentenceSplitter()
        let out = splitter.append("## Title\nUse **bold** and `code`.\n```swift\nlet x = 1.\n```\nSee [docs](http://x.y).\n")
        XCTAssertEqual(out, ["Title", "Use bold and code.", "See docs."])
    }

    // MARK: Stops on any key or click

    func test_speechStopsOnKey() {
        let (replies, speaker) = make()
        replies.handle(text("One. Two. Three. "))
        XCTAssertEqual(speaker.spoken, ["One."])
        replies.interrupter.handle(keyDown())
        XCTAssertEqual(speaker.stops, 1)
        XCTAssertFalse(replies.isSpeaking, "the waiting sentences are dropped too")
        replies.handle(text("Four. Five. "))
        replies.handle(done())
        XCTAssertEqual(speaker.spoken, ["One."], "the rest of the reply that was stopped stays unspoken")
        // The next reply is spoken again.
        replies.handle(CoreEvent(sessionID: "s1", runID: "r2", kind: .session(id: "s1", runID: "r2")))
        replies.handle(text("Six. "))
        XCTAssertEqual(speaker.spoken, ["One.", "Six."])
    }

    func test_speechStopsOnClick() {
        for type in [NSEvent.EventType.leftMouseDown, .rightMouseDown, .otherMouseDown] {
            let (replies, speaker) = make()
            replies.handle(text("One. Two. "))
            replies.interrupter.handle(mouse(type))
            XCTAssertEqual(speaker.stops, 1, "\(type)")
            XCTAssertFalse(replies.isSpeaking)
        }
    }

    func test_pointerMovementAndMouseUpDoNotStopSpeech() {
        let (replies, speaker) = make()
        replies.handle(text("One. Two. "))
        replies.interrupter.handle(mouse(.mouseMoved))
        replies.interrupter.handle(mouse(.leftMouseUp))
        XCTAssertEqual(speaker.stops, 0)
        XCTAssertTrue(replies.isSpeaking)
    }

    func test_aKeyWhileNothingIsBeingSpokenDoesNotSilenceTheReply() {
        let (replies, speaker) = make()
        replies.handle(text("Still thinking"))
        replies.interrupter.handle(keyDown())
        replies.handle(text(". Done. "))
        XCTAssertEqual(speaker.spoken, ["Still thinking."])
    }

    func test_cancellingTheRunStopsSpeech() {
        let (replies, speaker) = make()
        replies.handle(text("One. Two. "))
        replies.handle(CoreEvent(sessionID: "s1", runID: "r1", kind: .cancelled))
        XCTAssertEqual(speaker.stops, 1)
        XCTAssertFalse(replies.isSpeaking)
    }

    // MARK: Only the conversation on screen

    func test_backgroundRunsAreNotSpoken() {
        let (replies, speaker) = make(onScreen: { $0 == "open" })
        replies.handle(text("A schedule ran. It finished. ", session: "schedule-run"))
        replies.handle(done("A schedule ran.", session: "schedule-run"))
        XCTAssertEqual(speaker.spoken, [], "a run nobody is looking at is silent")
        replies.handle(text("You are looking at me. ", session: "open"))
        XCTAssertEqual(speaker.spoken, ["You are looking at me."])
    }

    func test_aReplyGoesSilentOnceItsConversationLeavesTheScreen() {
        var onScreen = true
        let (replies, speaker) = make(onScreen: { _ in onScreen })
        replies.handle(text("First. "))
        speaker.finishCurrent()
        onScreen = false
        replies.handle(text("Second. Third. "))
        replies.handle(done())
        XCTAssertEqual(speaker.spoken, ["First."])
    }

    func test_conversationOnScreenIsSpokenAndAnotherRunIsNot() async throws {
        let speaker = FakeSpeaker()
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        // The stub refuses to chat until the risk notice is accepted, as in ScreenStoreTests.makeClient.
        let client = PreviewCoreClient()
        try await client.acceptRiskNotice(version: 3)
        let model = AppModel(client: client, preferences: DisplayPreferences(defaults: defaults),
                             userFirstName: "Sam", speaker: speaker)
        model.isMainWindowVisible = { true }
        model.selection = .chat
        await model.chat.send("Good morning")
        let session = try XCTUnwrap(model.chat.currentSessionID)

        model.route(CoreEvent(sessionID: "morning-brief-run", runID: "r9", kind: .text("Your brief is ready. ")))
        XCTAssertEqual(speaker.spoken, [], "a schedule's run is not the conversation on screen")

        model.isMainWindowVisible = { false }
        model.route(CoreEvent(sessionID: session, runID: "r1", kind: .text("Hidden window. ")))
        XCTAssertEqual(speaker.spoken, [], "a closed or covered window shows nothing")

        model.isMainWindowVisible = { true }
        model.selection = .today
        model.route(CoreEvent(sessionID: session, runID: "r1", kind: .text("Another screen. ")))
        XCTAssertEqual(speaker.spoken, [], "the conversation isn't showing while Today is")

        model.selection = .chat
        model.route(CoreEvent(sessionID: session, runID: "r1", kind: .text("Good morning, Sam. ")))
        XCTAssertEqual(speaker.spoken, ["Good morning, Sam."])
    }

    func test_turningTheSettingOffSpeaksNothing() {
        let (replies, speaker) = make(enabled: false)
        replies.handle(text("Quiet please. "))
        replies.handle(done())
        XCTAssertEqual(speaker.spoken, [])
    }

    // MARK: Settings row

    func test_speakRepliesRowIsAnAppSettingOnByDefault() throws {
        let row = try XCTUnwrap(FeatureRows.all.first { $0.key == "voice" })
        XCTAssertEqual(row.title, "Speak replies")
        XCTAssertEqual(row.kind, .app(defaultsKey: VoiceSettings.defaultsKey))
        XCTAssertNil(row.permission, "dictation and speech belong to macOS: F7 asks for no permission")
        let defaults = UserDefaults(suiteName: "heron.tests.\(UUID().uuidString)")!
        XCTAssertTrue(VoiceSettings.isEnabled(defaults))
        defaults.set(false, forKey: VoiceSettings.defaultsKey)
        XCTAssertFalse(VoiceSettings.isEnabled(defaults))
    }

    // MARK: Dictation button

    func test_dictationButtonFallsBackToShortcutHint() {
        let model = DictationButtonModel(send: { false }, shortcut: { "fn twice" })
        XCTAssertNil(model.hint)
        model.start()
        XCTAssertEqual(model.hint, "Press fn twice to dictate.")
    }

    func test_dictationButtonSendsStartDictationAndShowsNoHint() {
        var sends = 0
        var accepts = false
        let model = DictationButtonModel(send: { sends += 1; return accepts }, shortcut: { "fn twice" })
        model.start()
        XCTAssertNotNil(model.hint)
        accepts = true
        model.start()
        XCTAssertEqual(sends, 2)
        XCTAssertNil(model.hint, "a working button clears the earlier hint")
    }

    func test_dictationNeedsAFocusedTextField() {
        // No window, so no field editor to send `startDictation:` to: the button falls back instead of sending to nothing.
        XCTAssertFalse(DictationButtonModel.sendStartDictation())
    }
}
