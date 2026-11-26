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
| `--signed-out` | With `--fake`/`--ui-testing`: start signed out instead, so the sign-in flows can be driven against the fake accounts (`FakeAccounts`: `tim`/`ana`/`bruno`/`cvetka`, password `correct-horse`, invite code `WXYZ-2345`, 2FA on `ana`, suspended `cvetka`, `tooOld` for the update gate) |
| `--demo-terms`, `--demo-update`, `--demo-suspended` | Debug builds only: opens signed in straight into the terms-required / update-required / suspended gate, for screenshots |

Without arguments the app talks to `http://127.0.0.1:8080` (change `server_url` in `UserDefaults`).

## Auth and onboarding (phase 2)

`HideoutCore.AuthFlow` is the state machine (`AuthStep`: password, email code, second factor, passkey, reset, invite code/accept) driving every screen in `HideoutUI/Auth`; `AuthScreen` switches on its `step` inside the shared `AuthChrome` (one calm card, centred on regular width, the bare page on a phone), with the server line and a quiet "Terms · Privacy" line (`account::legal_links` on desktop) always in the footer. `Gates.swift` holds the full-screen states after a session is restored: `SplashScreen` (loading), `TermsGate`, `UpdateGate`, `SuspendedGate`, and `WelcomeSheet` (the "add a passkey after signing up" prompt, `AUTH-11`).

**Passkeys.** `SystemPasskeys` (`PasskeyPlatform`) uses `ASAuthorizationPlatformPublicKeyCredentialProvider` directly when the server's host can be a WebAuthn relying party (`ServerURL.isDomain`, true for a registered domain with the `webcredentials:` associated domain, false for `127.0.0.1`/a LAN IP): `Begin*LoginOptions`/`Begin*RegistrationOptions` returns `options_json`, which becomes an `ASAuthorizationRequest`, and the resulting credential's JSON goes to `Finish*`. When the host cannot be a relying party (local dev, or a self-hosted server before it serves the AASA file), it falls back to opening the server's own ceremony page in an `SFSafariViewController` and polling for completion — the same browser-page flow desktop uses, so the dev loop and self-hosted servers without the well-known file still work.

**Server requirement for native passkeys and universal invite links**: the server must serve `/.well-known/apple-app-site-association` (no file extension, `Content-Type: application/json`, no redirect):

```json
{
  "webcredentials": { "apps": ["<TEAMID>.si.zabit.thehideout"] },
  "applinks": { "details": [{ "appIDs": ["<TEAMID>.si.zabit.thehideout"], "components": [{ "/": "/invite/*" }] }] }
}
```

`<TEAMID>` is the Apple Developer Team ID (`project.yml`'s `DEVELOPMENT_TEAM` once a real team is set); `Hideout.entitlements` already declares `webcredentials:thehideout.zabit.si` and `applinks:thehideout.zabit.si` for the production domain. Follow-up for a self-hosted server that cannot add that well-known file: a custom-scheme redirect (`thehideout://invite/<code>`) from the ceremony page, so the `SFSafariViewController` fallback can close itself instead of waiting purely on polling.

## Snapshot tests

`HideoutTests/SnapshotTests.swift` snapshots the gallery (every section) and the shell (servers, channel list, transcript, DMs, signed-out) in dark and light on iPhone (402 x 874) and iPad (834 x 1210) layouts, plus the looks of PARITY SYS-11 (Ocean/Cool, Coral/Paper, High contrast, Compact, Text 125 %). Screens with Liquid Glass are rendered through the simulator's key window (`drawHierarchyInKeyWindow`), because glass is composited by the render server and comes out blank from an offscreen render. Time zone, locale and "now" are pinned, so the pictures do not drift.

```sh
# run (references are in HideoutTests/__Snapshots__)
xcodebuild -project Hideout.xcodeproj -scheme Hideout -destination 'platform=iOS Simulator,name=iPhone 18 Pro' -derivedDataPath build test -only-testing:HideoutTests
# re-record after an intended design change, then LOOK at the diff of the PNGs
TEST_RUNNER_RECORD_SNAPSHOTS=1 xcodebuild ... test -only-testing:HideoutTests   # xcodebuild forwards TEST_RUNNER_* to the tests as RECORD_SNAPSHOTS
```

References are recorded on the **iPhone 18 Pro, iOS 27.2** simulator. CI does not run them (Liquid Glass renders slightly differently across simulator runtimes, which would fail GitHub's images for the wrong reason); it builds the app and runs the unit tests. Run the snapshots locally before a design PR; the glass shots allow a small pixel tolerance for the render server's highlights.

A sheet (`WelcomeSheet`, `ServerSheet`) is snapshotted as its own content view, not through the `.sheet` that presents it: an offscreen `UIHostingController` cannot reliably capture a UIKit-presented sheet, so `AuthSnapshotTests.testWelcomePasskeySheet` hosts `WelcomeSheet()` directly, the same way `testServerSheet` already did for `ServerSheet()`.

## UI tests

`HideoutUITests/AuthUITests.swift` drives the sign-in, invite-accept and wrong-code flows against `--ui-testing` end to end (real `XCUIApplication` taps, not just view code), plus `testAuthTour` which walks every auth screen and gate and saves a screenshot of each under `TOUR_DIR` (default `/tmp/shots/tour`) for a design review:

```sh
xcodebuild -project Hideout.xcodeproj -scheme Hideout -destination 'platform=iOS Simulator,name=iPhone 18 Pro' -derivedDataPath build test -only-testing:HideoutUITests/AuthUITests
TOUR_DIR=/tmp/shots/tour xcodebuild ... -only-testing:HideoutUITests/AuthUITests/testAuthTour   # screenshots only
```

Two things to know when touching this file:

- `CheckRow`'s checkbox carries the `.isToggle` accessibility trait, so XCUITest exposes it as a `Switch`, not a `Button`; the invite accept form's Terms/Privacy row has markdown links in its label, which widens that one switch's accessible frame past its 44pt box, so tapping it needs a coordinate near the leading edge, not the element's center (see `AuthUITests.tap(_:_:)`).
- A `.textContentType(.newPassword)` field triggers the system's "Use Strong Password?" callout as soon as it is focused; typing into the field while it is up can lose characters, so `type(_:into:secure:_:)` dismisses it and refocuses before typing.

These tests are **not** run in CI (see `.github/workflows/ci.yml`, job `ios`): a headless CI runner's interaction with the keyboard and system callouts above is unreliable in a way a local run is not, and a flaky gate is worse than no gate. Run them locally before an auth-flow PR, and whenever this doc's launch arguments or `FakeAccounts` change.

## Maestro flows

`maestro/` drives the real app on a simulator through the accessibility tree, not XCUITest; it is how we check things XCUITest can't easily reach, like the gateway staying connected against a real server. [Install Maestro](https://maestro.mobile.dev) (needs a JVM; `brew install openjdk` and add it to `PATH` if `maestro --version` can't find a Java runtime).

- `fake-smoke.yaml`: launches with `--fake`, cheap enough to run anytime.
  ```sh
  maestro --device <udid> test mobile/ios/maestro/fake-smoke.yaml
  ```
- `invite-code.yaml`: launches with `--fake --signed-out`, opens "Join with a code", types a code into
  the 8-box field (the 8th character looks it up at once) and reaches the accept step, then opens an
  invite deep link (the same `AuthFlow.handle(url:)` path a pasted link's extracted code or a universal
  link takes) and reaches the accept step again.
  ```sh
  maestro --device <udid> test mobile/ios/maestro/invite-code.yaml
  ```
- `live-dm.yaml`: signs in as a real account on a real server, opens a DM and checks a message sent by the other side (via the Connect JSON API, not the UI) arrives live with no "Connecting…" banner, then sends a reply and confirms it too. Needs:
  1. The dev stack up (`docker compose up -d`) and the server running against a disposable keyspace, e.g.:
     ```sh
     THEHIDEOUT_LISTEN_ADDR=127.0.0.1:18350 THEHIDEOUT_PUBLIC_URL=http://127.0.0.1:18350 \
     THEHIDEOUT_KEYSPACE=live_check_$(date +%s) THEHIDEOUT_HEARTBEAT_SECS=5 \
     THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL=you@example.com \
     cd server && cargo run
     ```
     (`THEHIDEOUT_HEARTBEAT_SECS=5` makes the stream's liveness visible on a short run instead of the 30s default.)
  2. Two accounts on it: accept the bootstrap invite logged at startup (`InviteService.AcceptInvite`, the Connect JSON API, e.g. `curl -X POST $SERVER_URL/thehideout.v1.InviteService/AcceptInvite -H 'Content-Type: application/json' --data '{...}'`) for account A, then `CreateInvite` + `AcceptInvite` for account B, then `DmService.OpenDm` between them to get a channel ID. See `server/tests/invites.rs` for the request shapes; invite codes are `XXXX-XXXX`.
  3. The simulator pointed at that server (the app reads `server_url` from `UserDefaults.standard`, key `ServerSelection.defaultsKey`):
     ```sh
     xcrun simctl spawn <udid> defaults write si.zabit.thehideout server_url "http://127.0.0.1:18350"
     ```
  4. Run it, with account A's credentials and account B's session passed in, never committed:
     ```sh
     maestro --device <udid> test mobile/ios/maestro/live-dm.yaml \
       -e USERNAME=alice -e PASSWORD=... -e PEER_USERNAME=bob \
       -e SERVER_URL=http://127.0.0.1:18350 -e PEER_TOKEN=... -e DM_CHANNEL_ID=...
     ```

Screenshots land under the run's `takeScreenshot` artifact folder (pass `--test-output-dir` to control where); keep them out of version control.

## Theming rules in code

*Glass for chrome, tokens for content.* Bars, tab bar, sheets and menus are the system's; `.themed(_:)` at the root sets the tint to the accent, the window background to `bg` and the colour scheme. Content uses `theme.colors.*`, `typeStyle(_:)` (Instrument Sans at an exact variable weight, Dynamic Type on top of the in-app text size), `Radius`, `Space` and `Motion`. On a filled accent (prominent buttons) use `.onAccent()` for the label: the system's white fails contrast on the lighter accents in dark mode.
