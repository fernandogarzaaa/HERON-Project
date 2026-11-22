import SwiftUI
import AppKit
import HeronCore

/// The floating panel: borderless, above other apps, and able to take typing without activating Heron.
final class QuickEntryPanel: NSPanel {
    var onCancel: (() -> Void)?

    init(contentViewController: NSViewController) {
        super.init(contentRect: NSRect(x: 0, y: 0, width: QuickEntryView.width, height: 120),
                   styleMask: [.borderless, .nonactivatingPanel, .fullSizeContentView],
                   backing: .buffered, defer: true)
        isFloatingPanel = true
        level = .floating
        hidesOnDeactivate = false
        isReleasedWhenClosed = false
        isMovableByWindowBackground = true
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        backgroundColor = .clear
        isOpaque = false
        hasShadow = true
        // A hosting controller sizes the window to its SwiftUI content, so the panel grows with the reply.
        self.contentViewController = contentViewController
    }

    override var canBecomeKey: Bool { true }

    /// Esc closes the panel.
    override func cancelOperation(_ sender: Any?) { onCancel?() }
}

/// Owns the panel, the global hotkey and the Services provider, and ties them to the model. One per app.
@MainActor
public final class QuickEntryController {
    public let model: QuickEntryModel
    private let preferences: DisplayPreferences
    private let defaults: UserDefaults
    /// Brings the main window forward on the given conversation ("Open in Heron").
    public var openMain: (String?) -> Void
    private var panel: QuickEntryPanel?
    private var hotKey: HotKey?
    private var observers: [NSObjectProtocol] = []
    public private(set) var servicesProvider: ServicesProvider?

    /// - Parameter openMain: brings the main window forward on the given conversation ("Open in Heron").
    public init(model: QuickEntryModel, preferences: DisplayPreferences, defaults: UserDefaults = .standard,
                openMain: @escaping (String?) -> Void) {
        self.model = model
        self.preferences = preferences
        self.defaults = defaults
        self.openMain = openMain
        model.onOpenInHeron = { [weak self] id in
            self?.hide()
            self?.openMain(id)
        }
    }

    public var isEnabled: Bool { QuickEntrySettings.isEnabled(defaults) }
    public var isVisible: Bool { panel?.isVisible ?? false }

    /// Registers the Services item and the hotkey (if the owner recorded one), and follows later changes to
    /// the hotkey and to the "Quick entry" switch.
    public func start() {
        let provider = ServicesProvider(isEnabled: { [weak self] in self?.isEnabled ?? false }) { [weak self] text, app in
            self?.show(attaching: text, from: .selection(app: app))
        }
        provider.register()
        servicesProvider = provider
        applyHotKey()
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: HotKeyStore.didChange, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.applyHotKey() }
        })
        observers.append(center.addObserver(forName: UserDefaults.didChangeNotification, object: defaults, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.applyHotKey() }
        })
    }

    /// (Re)registers the stored hotkey, or drops it when the feature is off or none is recorded.
    private func applyHotKey() {
        guard isEnabled, let combo = HotKeyStore.load(defaults) else {
            hotKey?.unregister()
            registeredCombo = nil
            return
        }
        let key = hotKey ?? HotKey { [weak self] in self?.toggle() }
        hotKey = key
        if !key.isRegistered || registeredCombo != combo {
            do {
                try key.register(combo)
                registeredCombo = combo
            } catch {
                registeredCombo = nil
            }
        }
    }

    private var registeredCombo: HotKeyCombo?

    // MARK: Showing

    /// Opens the panel, with outside text attached as a chip when there is some. Nothing is sent.
    public func show(attaching text: String? = nil, from source: QuickEntryModel.Source? = nil) {
        guard isEnabled else { return }
        model.open()
        if let text, let source { model.attach(text: text, from: source) }
        let panel = self.panel ?? makePanel()
        self.panel = panel
        panel.layoutIfNeeded()
        position(panel)
        panel.makeKeyAndOrderFront(nil)
    }

    public func toggle() { isVisible ? hide() : show() }

    public func hide() { panel?.orderOut(nil) }

    private func makePanel() -> QuickEntryPanel {
        let host = NSHostingController(rootView: HeronThemed(preferences: preferences) { QuickEntryView(model: model) })
        host.sizingOptions = [.preferredContentSize]
        let panel = QuickEntryPanel(contentViewController: host)
        panel.onCancel = { [weak self] in self?.hide() }
        return panel
    }

    /// Centred on the screen under the pointer, in its upper third.
    private func position(_ panel: NSPanel) {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { $0.frame.contains(mouse) } ?? NSScreen.main
        guard let frame = screen?.visibleFrame else { return }
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(x: frame.midX - size.width / 2, y: frame.maxY - frame.height / 3 - size.height))
    }
}

/// The panel's content: one text field, the chip, the reply below, and "Open in Heron".
struct QuickEntryView: View {
    static let width: CGFloat = 560

    @Bindable var model: QuickEntryModel
    @Environment(\.heronTheme) private var theme
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.s3) {
            TextField("Ask Heron anything", text: $model.field, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 18))
                .foregroundStyle(theme.ink)
                .lineLimit(1...4)
                .focused($focused)
                .onSubmit { Task { await model.submit() } }
                .accessibilityLabel("Ask Heron")
            if let chip = model.chip { ChipRow(chip: chip, remove: { model.removeChip() }) }
            if !model.reply.isEmpty {
                RowDivider()
                ScrollView { MarkdownText(markdown: model.reply).frame(maxWidth: .infinity, alignment: .leading) }
                    .frame(maxHeight: 260)
            }
            HStack(spacing: Spacing.s2) {
                statusText
                Spacer()
                Button("Open in Heron") { model.openInHeron() }
                    .buttonStyle(HButtonStyle(.secondary))
                    .accessibilityHint("Continues this conversation in the main window")
            }
        }
        .padding(Spacing.s4)
        .frame(width: Self.width)
        .background(RoundedRectangle(cornerRadius: Radius.sheet, style: .continuous).fill(theme.surface))
        .overlay(RoundedRectangle(cornerRadius: Radius.sheet, style: .continuous).stroke(theme.cardBorder, lineWidth: theme.cardBorderWidth))
        .onAppear { focused = true }
    }

    @ViewBuilder private var statusText: some View {
        switch model.status {
        case .idle:
            Text(model.chip == nil ? "Return to send" : "Return to send with the text above")
                .font(HeronFont.foot).foregroundStyle(theme.ink3)
        case .sending, .working:
            Text("Heron is working…").font(HeronFont.foot).foregroundStyle(theme.ink2)
        case .needsApproval:
            Text("Heron needs your OK. Open in Heron to answer.").font(HeronFont.foot).foregroundStyle(theme.waiting)
        case .failed(let message):
            Text(message).font(HeronFont.foot).foregroundStyle(theme.danger).lineLimit(2)
        }
    }
}

/// "From Safari" in the violet outside-content tone, with a button that removes it. The tooltip shows the text.
private struct ChipRow: View {
    let chip: QuickEntryModel.Chip
    let remove: () -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        HStack(spacing: Spacing.s2) {
            HTag(chip.label, tone: .outside, symbol: "text.quote")
            Text(chip.text.replacingOccurrences(of: "\n", with: " "))
                .font(HeronFont.sub).foregroundStyle(theme.ink2).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: 0)
            Button(action: remove) { Image(systemName: "xmark.circle.fill") }
                .buttonStyle(.plain).foregroundStyle(theme.ink3)
                .accessibilityLabel("Remove the attached text")
        }
        .help(chip.text)
    }
}
