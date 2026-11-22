# App Intents check: "Ask Heron" in Shortcuts

Status: deferred to v1.1

F1 (docs/designs/heron-v1-features.md, "F1. Quick entry and Ask Heron about this") says the "Ask Heron" App Intent ships only if `swift build` alone can produce the App Intents metadata that Shortcuts, Spotlight and Siri read: `Contents/Resources/Metadata.appintents/` in the built bundle. This unit could not show that it can, so `AskHeronIntent.swift` and its two tests (`intentTextIsContext`, `intentNeverSends`) were deleted, as the spec directs. F1 ships with the panel, the hotkey and Services.

## Why

*   **Nothing in the build path writes `Metadata.appintents`.** `packaging/macos/build-app.sh --unsigned` runs `swift build -c release`, copies the one executable into `dist/Heron.app/Contents/MacOS/`, writes `Info.plist` and `icon.icns` into the bundle, and signs it. It has no step that creates or copies a `Metadata.appintents` directory, so `dist/Heron.app/Contents/Resources/Metadata.appintents` doesn't exist after it runs.
*   **The metadata comes from a tool SwiftPM doesn't call.** Xcode's build system runs `appintentsmetadataprocessor` (it is in the Xcode toolchain, `usr/bin/appintentsmetadataprocessor`) over the compiler's extracted constant values for the module. heron-v1.md builds with SwiftPM and no Xcode project, and nothing in `Package.swift` or `build-app.sh` runs that tool or asks the compiler for those values.
*   **Confirmed by a real build.** The supervisor built a minimal macOS 14 executable package containing one `AppIntent` with `swift build` (Swift 6.4.2, swift-build) and searched `.build`: there is no `Metadata.appintents` and no `extract.actionsdata` anywhere in it. So `swift build` alone does not produce the App Intents metadata, and `build-app.sh` (which only wraps `swift build`) can't either. (This unit's own sandbox refuses `swift build`, so the evidence is the supervisor's run, not one made here.)

## What this changes

*   `ChatContext.Kind.intent` stays in `HeronCore` because the core's `POST /api/chat` accepts it (B2). Nothing in the app sends it in v1.
*   `QuickEntryModel.Source` has one case, `.selection`. v1.1 adds the intent's case and calls the same `attach(text:from:)`, which never sends.
*   The F1 docs outside this unit (`docs/features/quick_entry.md`, from F1d) should say that Siri and Shortcuts are v1.1 and that Quick entry has the panel, the hotkey and the Services item. QF3 lets F1 ship that way.

## To pick this up in v1.1

1.  Decide how the metadata gets built without an Xcode project: either run `appintentsmetadataprocessor` from `build-app.sh` after `swift build` (needs the module's `.swiftconstvalues`, which SwiftPM doesn't emit unless asked with `-Xswiftc -emit-const-values`), or move the app target to an Xcode project.
2.  Restore `AskHeronIntent.swift` (`AppIntent` with one `String` parameter, `openAppWhenRun = true`, a `perform()` that calls `QuickEntryController.show(attaching:from:)`) and its two tests: the text becomes a chip "From Shortcuts" of kind `intent`, and nothing is sent.
3.  Build the bundle and check that `Contents/Resources/Metadata.appintents` exists.
4.  On the owner's Mac, check that "Ask Heron" appears in Shortcuts, then replace the `Status:` line above with the result.
