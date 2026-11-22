import AppKit
import Carbon.HIToolbox
import SwiftUI

/// A global hotkey the owner recorded: a key and its modifiers. There is no default (F1): ⌥⌘Space opens a
/// Finder search window and ⌃Space switches input sources, so the owner picks one.
public struct HotKeyCombo: Codable, Equatable, Sendable {
    /// A Carbon virtual key code (`kVK_…`).
    public var keyCode: UInt32
    /// Carbon modifier flags (`cmdKey | optionKey | controlKey | shiftKey`).
    public var modifiers: UInt32

    public init(keyCode: UInt32, modifiers: UInt32) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    /// Carbon's modifier mask for an `NSEvent`'s flags.
    public static func carbonModifiers(from flags: NSEvent.ModifierFlags) -> UInt32 {
        var mask: UInt32 = 0
        if flags.contains(.command) { mask |= UInt32(cmdKey) }
        if flags.contains(.option) { mask |= UInt32(optionKey) }
        if flags.contains(.control) { mask |= UInt32(controlKey) }
        if flags.contains(.shift) { mask |= UInt32(shiftKey) }
        return mask
    }

    /// A hotkey needs ⌘, ⌃ or ⌥. Shift alone, or no modifier, would take over ordinary typing.
    public var isUsable: Bool { modifiers & UInt32(cmdKey | controlKey | optionKey) != 0 }

    /// The combo for a key press, or `nil` when it isn't usable as a hotkey.
    public init?(event: NSEvent) {
        let combo = HotKeyCombo(keyCode: UInt32(event.keyCode), modifiers: Self.carbonModifiers(from: event.modifierFlags))
        guard combo.isUsable else { return nil }
        self = combo
    }

    /// "⌃⌥⌘H", the way menus write it.
    public var display: String {
        var out = ""
        if modifiers & UInt32(controlKey) != 0 { out += "⌃" }
        if modifiers & UInt32(optionKey) != 0 { out += "⌥" }
        if modifiers & UInt32(shiftKey) != 0 { out += "⇧" }
        if modifiers & UInt32(cmdKey) != 0 { out += "⌘" }
        return out + (Self.keyNames[keyCode] ?? "Key \(keyCode)")
    }

    private static let keyNames: [UInt32: String] = {
        let table: [(Int, String)] = [
            (kVK_ANSI_A, "A"), (kVK_ANSI_B, "B"), (kVK_ANSI_C, "C"), (kVK_ANSI_D, "D"), (kVK_ANSI_E, "E"),
            (kVK_ANSI_F, "F"), (kVK_ANSI_G, "G"), (kVK_ANSI_H, "H"), (kVK_ANSI_I, "I"), (kVK_ANSI_J, "J"),
            (kVK_ANSI_K, "K"), (kVK_ANSI_L, "L"), (kVK_ANSI_M, "M"), (kVK_ANSI_N, "N"), (kVK_ANSI_O, "O"),
            (kVK_ANSI_P, "P"), (kVK_ANSI_Q, "Q"), (kVK_ANSI_R, "R"), (kVK_ANSI_S, "S"), (kVK_ANSI_T, "T"),
            (kVK_ANSI_U, "U"), (kVK_ANSI_V, "V"), (kVK_ANSI_W, "W"), (kVK_ANSI_X, "X"), (kVK_ANSI_Y, "Y"),
            (kVK_ANSI_Z, "Z"), (kVK_ANSI_0, "0"), (kVK_ANSI_1, "1"), (kVK_ANSI_2, "2"), (kVK_ANSI_3, "3"),
            (kVK_ANSI_4, "4"), (kVK_ANSI_5, "5"), (kVK_ANSI_6, "6"), (kVK_ANSI_7, "7"), (kVK_ANSI_8, "8"),
            (kVK_ANSI_9, "9"), (kVK_Space, "Space"), (kVK_Return, "Return"), (kVK_Tab, "Tab"),
            (kVK_ANSI_Minus, "-"), (kVK_ANSI_Equal, "="), (kVK_ANSI_Comma, ","), (kVK_ANSI_Period, "."),
            (kVK_ANSI_Slash, "/"), (kVK_ANSI_Semicolon, ";"), (kVK_ANSI_Quote, "'"),
            (kVK_ANSI_LeftBracket, "["), (kVK_ANSI_RightBracket, "]"), (kVK_ANSI_Backslash, "\\"),
            (kVK_ANSI_Grave, "`"), (kVK_LeftArrow, "←"), (kVK_RightArrow, "→"), (kVK_UpArrow, "↑"),
            (kVK_DownArrow, "↓"), (kVK_F1, "F1"), (kVK_F2, "F2"), (kVK_F3, "F3"), (kVK_F4, "F4"),
            (kVK_F5, "F5"), (kVK_F6, "F6"), (kVK_F7, "F7"), (kVK_F8, "F8"), (kVK_F9, "F9"),
            (kVK_F10, "F10"), (kVK_F11, "F11"), (kVK_F12, "F12"),
        ]
        return Dictionary(uniqueKeysWithValues: table.map { (UInt32($0.0), $0.1) })
    }()
}

/// Where the recorded hotkey is kept. `load` is `nil` until the owner records one.
public enum HotKeyStore {
    public static let defaultsKey = "heron.quickEntry.hotKey"
    /// Posted after the stored hotkey changed, so the registered one follows.
    public static let didChange = Notification.Name("heron.quickEntry.hotKeyDidChange")

    public static func load(_ defaults: UserDefaults = .standard) -> HotKeyCombo? {
        guard let data = defaults.data(forKey: defaultsKey), let combo = try? JSONDecoder().decode(HotKeyCombo.self, from: data),
              combo.isUsable else { return nil }
        return combo
    }

    /// Stores `combo`, or clears the hotkey when it is `nil`. An unusable combo is refused.
    @discardableResult
    public static func save(_ combo: HotKeyCombo?, _ defaults: UserDefaults = .standard) -> Bool {
        if let combo {
            guard combo.isUsable, let data = try? JSONEncoder().encode(combo) else { return false }
            defaults.set(data, forKey: defaultsKey)
        } else {
            defaults.removeObject(forKey: defaultsKey)
        }
        NotificationCenter.default.post(name: didChange, object: nil)
        return true
    }
}

/// The four-character code ('HRNQ') that marks Heron's hotkey events, so a handler ignores anyone else's.
private let hotKeySignature: OSType = 0x4852_4E51

/// The Carbon callback. A top-level function, so it is a plain C function pointer with no isolation of its own.
private func hotKeyEventHandler(_: EventHandlerCallRef?, _ event: EventRef?, _ userData: UnsafeMutableRawPointer?) -> OSStatus {
    guard let userData, let event else { return OSStatus(eventNotHandledErr) }
    var id = EventHotKeyID()
    let read = GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil,
                                 MemoryLayout<EventHotKeyID>.size, nil, &id)
    guard read == noErr, id.signature == hotKeySignature else { return OSStatus(eventNotHandledErr) }
    let hotKey = Unmanaged<HotKey>.fromOpaque(userData).takeUnretainedValue()
    // Application events are delivered on the main thread.
    MainActor.assumeIsolated { hotKey.fire() }
    return noErr
}

/// One registered global hotkey, through `RegisterEventHotKey`. That call needs no permission.
@MainActor
public final class HotKey {
    public enum RegistrationError: Error, Equatable {
        case notUsable
        /// macOS refused the key, usually because another app holds it.
        case refused(OSStatus)
    }

    private var hotKeyRef: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private let handler: @MainActor () -> Void

    public init(handler: @escaping @MainActor () -> Void) { self.handler = handler }

    public var isRegistered: Bool { hotKeyRef != nil }

    fileprivate func fire() { handler() }

    /// Registers `combo`, replacing whatever was registered before.
    public func register(_ combo: HotKeyCombo) throws {
        guard combo.isUsable else { throw RegistrationError.notUsable }
        unregister()
        if handlerRef == nil {
            var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
            let status = InstallEventHandler(GetApplicationEventTarget(), hotKeyEventHandler, 1, &spec,
                                             Unmanaged.passUnretained(self).toOpaque(), &handlerRef)
            guard status == noErr else { throw RegistrationError.refused(status) }
        }
        var ref: EventHotKeyRef?
        let status = RegisterEventHotKey(combo.keyCode, combo.modifiers, EventHotKeyID(signature: hotKeySignature, id: 1),
                                         GetApplicationEventTarget(), 0, &ref)
        guard status == noErr, let ref else { throw RegistrationError.refused(status) }
        hotKeyRef = ref
    }

    public func unregister() {
        if let hotKeyRef { UnregisterEventHotKey(hotKeyRef) }
        hotKeyRef = nil
    }

    deinit {
        // Both refs are plain handles; releasing them here needs no main-actor state.
        if let hotKeyRef { UnregisterEventHotKey(hotKeyRef) }
        if let handlerRef { RemoveEventHandler(handlerRef) }
    }
}

/// "Quick entry shortcut" in Settings › Features: shows the recorded hotkey, records a new one, clears it.
/// Esc cancels a recording. A key with no ⌘, ⌃ or ⌥ is not accepted.
struct HotKeyRecorderView: View {
    @State private var combo = HotKeyStore.load()
    @State private var recording = false
    @State private var monitor: Any?
    @State private var hint: String?
    @Environment(\.heronTheme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s1) {
            HStack(spacing: Spacing.s2) {
                Text("Shortcut").font(HeronFont.callout).foregroundStyle(theme.ink2)
                Button(recording ? "Press a key combination…" : (combo?.display ?? "Record a shortcut")) { toggle() }
                    .buttonStyle(HButtonStyle(.secondary))
                if combo != nil, !recording {
                    Button("Clear") { combo = nil; HotKeyStore.save(nil) }.buttonStyle(HButtonStyle(.link))
                }
            }
            Text(hint ?? "There is no default. Use ⌘, ⌃ or ⌥ with a key. Esc cancels.")
                .font(HeronFont.foot).foregroundStyle(theme.ink3)
        }
        .onDisappear { stop() }
    }

    private func toggle() { recording ? stop() : start() }

    private func start() {
        recording = true
        hint = nil
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == UInt16(kVK_Escape) { stop(); return nil }
            guard let picked = HotKeyCombo(event: event) else {
                hint = "Add ⌘, ⌃ or ⌥ so the shortcut doesn't take over typing."
                return nil
            }
            combo = picked
            HotKeyStore.save(picked)
            stop()
            return nil
        }
    }

    private func stop() {
        recording = false
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
    }
}
