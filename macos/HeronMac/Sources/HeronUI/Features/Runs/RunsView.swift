import SwiftUI
import HeronCore

/// The Runs screen: one list of the runs in progress and the ones that finished (heron-v1-features.md, F3).
/// A row opens the run's conversation, or its approval when it is waiting. Colour is never the only signal:
/// a waiting run carries a badge and the word "Waiting".
struct RunsView: View {
    @Bindable var model: AppModel

    private var runs: RunsStore { model.runs }

    var body: some View {
        ScreenScaffold(loading: runs.loadState.isLoading, failure: runs.loadState.failureText, isEmpty: runs.runs.isEmpty,
                       empty: EmptyStateView(title: "No runs yet",
                                             text: "Each time Heron works on something, it is listed here, with whether it is still going, waiting for you or done.")) {
            VStack(alignment: .leading, spacing: Spacing.s5) {
                if !runs.inProgress.isEmpty { group("In progress", runs.inProgress) }
                if !runs.finished.isEmpty { group("Finished", runs.finished) }
            }
        }
        .task { await runs.load() }
    }

    private func group(_ title: String, _ items: [RunRecord]) -> some View {
        VStack(alignment: .leading, spacing: Spacing.s2) {
            SectionLabel(title)
            RowGroup {
                ForEach(Array(items.enumerated()), id: \.element.id) { i, run in
                    if i > 0 { RowDivider() }
                    RunRow(run: run) { model.open(run: run) }
                }
            }
        }
    }
}

private struct RunRow: View {
    let run: RunRecord
    let open: () -> Void
    @Environment(\.heronTheme) private var theme

    var body: some View {
        Button(action: open) {
            HStack(alignment: .top, spacing: Spacing.s3) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(run.title.isEmpty ? "New conversation" : run.title).foregroundStyle(theme.ink).lineLimit(2)
                    Text(RunsStore.detailLine(run)).font(HeronFont.sub).foregroundStyle(theme.ink2).lineLimit(1)
                }
                Spacer(minLength: Spacing.s3)
                tag
            }
            .padding(.horizontal, Spacing.s4).padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
        .accessibilityHint(run.isWaiting ? "Opens the approval" : "Opens the conversation")
    }

    @ViewBuilder private var tag: some View {
        switch run.status {
        case "waiting": HTag(RunsStore.statusLabel(run.status), tone: .waiting, symbol: "hand.raised")
        case "running": HTag(RunsStore.statusLabel(run.status), tone: .accent, symbol: "circle.dotted")
        case "done": HTag(RunsStore.statusLabel(run.status), tone: .ok, symbol: "checkmark")
        case "error": HTag(RunsStore.statusLabel(run.status), tone: .danger, symbol: "exclamationmark.triangle")
        default: HTag(RunsStore.statusLabel(run.status), tone: .neutral)
        }
    }
}
