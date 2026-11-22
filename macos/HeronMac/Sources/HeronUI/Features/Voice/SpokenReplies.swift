import AppKit
import AVFoundation
import Foundation
import HeronCore

/// Speaks one sentence at a time. The real one is `AVSpeechSpeaker`; tests use a fake that finishes on command.
@MainActor
public protocol SpeechSynthesizing: AnyObject {
    /// Called after an utterance has been spoken to its end. Not called for one that `stopSpeaking()` cut off.
    var onFinish: (() -> Void)? { get set }
    func speak(_ sentence: String)
    func stopSpeaking()
}

/// `AVSpeechSynthesizer` with the system voice. It runs on the device and needs no permission.
@MainActor
final class AVSpeechSpeaker: NSObject, SpeechSynthesizing, AVSpeechSynthesizerDelegate {
    var onFinish: (() -> Void)?
    /// Created on first use, so an app that never speaks never starts the synthesizer.
    private lazy var synthesizer: AVSpeechSynthesizer = {
        let synthesizer = AVSpeechSynthesizer()
        synthesizer.delegate = self
        return synthesizer
    }()

    func speak(_ sentence: String) {
        synthesizer.speak(AVSpeechUtterance(string: sentence))
    }

    func stopSpeaking() {
        synthesizer.stopSpeaking(at: .immediate)
    }

    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        Task { @MainActor in self.onFinish?() }
    }
}

/// Stops speech on any key press or click. The local monitor sees Heron's own windows (the F1 panel included);
/// the global one sees clicks in other apps. Key presses in other apps are not visible without an Input
/// Monitoring permission, which F7 does not ask for.
@MainActor
public final class SpeechInterrupter {
    private let onInterrupt: () -> Void
    private var monitors: [Any] = []

    static let clickMask: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
    static let localMask: NSEvent.EventTypeMask = SpeechInterrupter.clickMask.union(.keyDown)

    init(onInterrupt: @escaping () -> Void) { self.onInterrupt = onInterrupt }

    func start() {
        guard monitors.isEmpty else { return }
        if let local = NSEvent.addLocalMonitorForEvents(matching: Self.localMask, handler: { [weak self] event in
            MainActor.assumeIsolated { _ = self?.handle(event) }
            return event
        }) { monitors.append(local) }
        if let global = NSEvent.addGlobalMonitorForEvents(matching: Self.clickMask, handler: { [weak self] event in
            MainActor.assumeIsolated { _ = self?.handle(event) }
        }) { monitors.append(global) }
    }

    func stop() {
        for monitor in monitors { NSEvent.removeMonitor(monitor) }
        monitors = []
    }

    /// Key down, or a mouse button going down, interrupts. Everything else (movement, scrolling, modifier keys) doesn't.
    @discardableResult
    func handle(_ event: NSEvent) -> NSEvent {
        switch event.type {
        case .keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown: onInterrupt()
        default: break
        }
        return event
    }
}

/// F7's spoken replies: reads a reply aloud, one sentence at a time, as each sentence completes in the `text`
/// events. Only a conversation that is on screen is spoken, so a schedule or a phone turn running in the
/// background is silent.
@MainActor
public final class SpokenReplies {
    private struct Run {
        var splitter = SentenceSplitter()
        var sawText = false
        /// A key or click cut this reply off: the rest of it stays unspoken.
        var muted = false
    }

    private let speaker: any SpeechSynthesizing
    private let isEnabled: () -> Bool
    /// Whether the conversation `sessionID` is on screen right now: the open conversation of the main window while it
    /// is showing, or the F1 panel's while it is showing. `AppModel` supplies it.
    public var isOnScreen: (String) -> Bool

    private var runs: [String: Run] = [:]
    private var lastSeq: [String: Int] = [:]
    private var queue: [String] = []
    private var speaking = false
    private(set) var interrupter: SpeechInterrupter!

    public convenience init(isEnabled: @escaping () -> Bool = { VoiceSettings.isEnabled() },
                            isOnScreen: @escaping (String) -> Bool = { _ in false }) {
        self.init(speaker: AVSpeechSpeaker(), isEnabled: isEnabled, isOnScreen: isOnScreen)
    }

    init(speaker: any SpeechSynthesizing, isEnabled: @escaping () -> Bool, isOnScreen: @escaping (String) -> Bool) {
        self.speaker = speaker
        self.isEnabled = isEnabled
        self.isOnScreen = isOnScreen
        interrupter = SpeechInterrupter { [weak self] in self?.interrupt() }
        speaker.onFinish = { [weak self] in
            self?.speaking = false
            self?.advance()
        }
    }

    /// Starts listening for the key or click that stops speech. Call once, at launch.
    public func start() { interrupter.start() }

    /// True while a sentence is being spoken or waits for its turn.
    public var isSpeaking: Bool { speaking || !queue.isEmpty }

    // MARK: Events

    /// Feeds one event of a conversation the app is showing (from `ChatStore` and `QuickEntryModel`, which hold events
    /// until their conversation exists).
    public func handle(_ event: CoreEvent) {
        if case .resync = event.kind { runs = [:]; lastSeq = [:]; return }
        guard let session = event.sessionID else { return }
        // The main window and the panel can both show one conversation and both pass the same event on.
        if event.seq > 0 {
            if let last = lastSeq[session], event.seq <= last { return }
            lastSeq[session] = event.seq
        }
        switch event.kind {
        case .session:
            runs[session] = nil
        case .text(let chunk):
            guard canSpeak(session) else { return }
            var run = runs[session] ?? Run()
            run.sawText = true
            let sentences = run.splitter.append(chunk)
            runs[session] = run
            enqueue(sentences)
        case .done(let text, _):
            finish(session, fallbackText: text)
        case .runEnd:
            finish(session, fallbackText: nil)
        case .cancelled, .error:
            runs[session] = nil
            silence()
        default:
            break
        }
    }

    private func canSpeak(_ session: String) -> Bool {
        guard isEnabled() else { silence(); return false }
        guard isOnScreen(session) else { return false }
        return !(runs[session]?.muted ?? false)
    }

    /// The reply is complete: its last sentence needs no terminator. A reply that came whole in `done` is spoken from there.
    private func finish(_ session: String, fallbackText: String?) {
        guard let run = runs.removeValue(forKey: session) else {
            if let fallbackText, canSpeak(session) {
                var splitter = SentenceSplitter()
                enqueue(splitter.append(fallbackText) + splitter.flush())
            }
            return
        }
        guard !run.muted, isEnabled(), isOnScreen(session) else { return }
        var splitter = run.splitter
        if run.sawText {
            enqueue(splitter.flush())
        } else if let fallbackText {
            enqueue(splitter.append(fallbackText) + splitter.flush())
        }
    }

    // MARK: Speaking

    private func enqueue(_ sentences: [String]) {
        queue.append(contentsOf: sentences)
        advance()
    }

    /// Hands the speaker the next sentence once the last one has been spoken.
    private func advance() {
        guard !speaking, !queue.isEmpty else { return }
        guard isEnabled() else { queue = []; return }
        speaking = true
        speaker.speak(queue.removeFirst())
    }

    private func silence() {
        queue = []
        if speaking { speaker.stopSpeaking() }
        speaking = false
    }

    /// A key or a click: speech stops now, and the rest of the replies being spoken stays unspoken.
    /// With nothing being spoken, typing or clicking changes nothing, so a reply that is still arriving is read later.
    func interrupt() {
        guard isSpeaking else { return }
        for session in Array(runs.keys) { runs[session]?.muted = true }
        silence()
    }
}
