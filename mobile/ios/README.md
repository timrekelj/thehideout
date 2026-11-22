# thehideout for iOS

SwiftUI, iOS 26+, Swift 6 language mode (strict concurrency, warnings as errors). Architecture and decisions: [`../README.md`](../README.md). Feature checklist: [`../PARITY.md`](../PARITY.md).

```
Hideout.xcodeproj       generated from project.yml (committed)
project.yml             XcodeGen spec: the single source of the project
Hideout/                the app target: @main, AppContainer, Info.plist (generated), asset catalog (icon, colours)
HideoutTests/           look persistence, token checks and the snapshot tests (+ __Snapshots__)
Shared/                 DesignTokens.generated.swift (written by ../tokens/generate.py)
Packages/
  HideoutAPI            generated protobuf messages + Connect clients (../api/generate.sh)
  HideoutDesign         the tokens as a module (a symlink to Shared/), so the app, HideoutUI and every extension share them
  HideoutCore           Foundation only, `swift test` on macOS: models, AppState + Reducer (apply.rs), AppStore,
                        GatewayStream, ConnectApiClient + AppError, Keychain TokenStore, Cache, FakeBackend
  HideoutUI             SwiftUI: theme injection, components, the design gallery, the tab shell
```

The fonts (Instrument Sans variable, JetBrains Mono) are referenced from `../tokens/fonts` by `project.yml`, not copied; their OFL licences go into the Acknowledgements screen (settings phase).

## Everyday commands

```sh
# regenerate the Xcode project after editing project.yml (brew install xcodegen; CI pins 2.48.0)
cd mobile/ios && xcodegen generate

# HideoutCore unit tests, no simulator
swift test --package-path mobile/ios/Packages/HideoutCore

# build, run on a simulator against the in-memory fake world
xcodebuild -project Hideout.xcodeproj -scheme Hideout -destination 'platform=iOS Simulator,name=iPhone 18 Pro' -derivedDataPath build build
xcrun simctl boot "iPhone 18 Pro"
xcrun simctl install booted build/Build/Products/Debug-iphonesimulator/Hideout.app
xcrun simctl launch booted si.zabit.thehideout --fake        # or --gallery (debug builds)
xcrun simctl io booted screenshot /tmp/shot.png
```

## Launch arguments

| Argument | Effect |
|---|---|
| `--fake` | The `FakeBackend` replaces the network: signed in as tim in desktop's fake world (ana, bruno, cvetka; The Hideout with sections, voice channels, DMs) |
| `--gallery` | Debug builds only: opens the design gallery (colours, type, components, look switcher). Also at You > Design gallery |
| `--tab servers\|dms\|mentions\|you` | Debug builds only: the tab to open on (for screenshots) |
| `--ui-testing` | Same as `--fake`; reserved for the XCUITest suite |

Without arguments the app talks to `http://127.0.0.1:8080` (change `server_url` in `UserDefaults`); sign-in arrives in phase 2.

## Snapshot tests

`HideoutTests/SnapshotTests.swift` snapshots the gallery (every section) and the shell (servers, channel list, transcript, DMs, signed-out) in dark and light on iPhone (402 x 874) and iPad (834 x 1210) layouts, plus the looks of PARITY SYS-11 (Ocean/Cool, Coral/Paper, High contrast, Compact, Text 125 %). Screens with Liquid Glass are rendered through the simulator's key window (`drawHierarchyInKeyWindow`), because glass is composited by the render server and comes out blank from an offscreen render. Time zone, locale and "now" are pinned, so the pictures do not drift.

```sh
# run (references are in HideoutTests/__Snapshots__)
xcodebuild -project Hideout.xcodeproj -scheme Hideout -destination 'platform=iOS Simulator,name=iPhone 18 Pro' -derivedDataPath build test -only-testing:HideoutTests
# re-record after an intended design change, then LOOK at the diff of the PNGs
TEST_RUNNER_RECORD_SNAPSHOTS=1 xcodebuild ... test -only-testing:HideoutTests   # xcodebuild forwards TEST_RUNNER_* to the tests as RECORD_SNAPSHOTS
```

References are recorded on the **iPhone 18 Pro, iOS 27.2** simulator. CI does not run them (Liquid Glass renders slightly differently across simulator runtimes, which would fail GitHub's images for the wrong reason); it builds the app and runs the unit tests. Run the snapshots locally before a design PR; the glass shots allow a small pixel tolerance for the render server's highlights.

## Theming rules in code

*Glass for chrome, tokens for content.* Bars, tab bar, sheets and menus are the system's; `.themed(_:)` at the root sets the tint to the accent, the window background to `bg` and the colour scheme. Content uses `theme.colors.*`, `typeStyle(_:)` (Instrument Sans at an exact variable weight, Dynamic Type on top of the in-app text size), `Radius`, `Space` and `Motion`. On a filled accent (prominent buttons) use `.onAccent()` for the label: the system's white fails contrast on the lighter accents in dark mode.
