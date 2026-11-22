# thehideout mobile

Native iOS (SwiftUI) and Android (Jetpack Compose) apps with the same features as the desktop client, designed to feel native on each platform while keeping thehideout's warm look. This directory is the foundation both apps build on: design tokens, generated API clients, the feature checklist and the architecture they follow.

- [`PARITY.md`](PARITY.md): every desktop feature and screen, the mobile adaptation, the native component on each platform. The definition of done.
- [`tokens/`](tokens): design tokens exported from the Rust design crate, the generator, bundled fonts.
- [`api/`](api): `buf` generation of the Swift and Kotlin API clients from `proto/`.
- [`ios/`](ios): the Xcode project (generated from `project.yml`), the app, `Packages/{HideoutAPI,HideoutDesign,HideoutCore,HideoutUI}`, snapshot tests. See [`ios/README.md`](ios/README.md).
- [`android/`](android): the Gradle project: `:app`, `:core`, `:ui`, `:design` (generated tokens), `:api` (generated client).

## Decisions (settled)

| Topic | Decision |
|---|---|
| Networking | Each app talks to the server directly with code generated from the protos: **connect-swift 1.2.3** (Connect protocol over `URLSession`) and **connect-kotlin 0.10.0** (OkHttp). No shared Rust core. |
| Realtime | `GatewayService.Subscribe` is a server-streaming RPC: connect-swift `ServerOnlyAsyncStreamInterface` (`GatewayServiceClient.subscribe`), connect-kotlin `ServerOnlyStreamInterface`. |
| Voice and video | Official LiveKit Swift SDK and LiveKit Android SDK, joined with the `{url, token}` from `VoiceService.JoinVoice`. |
| Design tokens | One source of truth, `client/design`. Exported to JSON, Swift and Kotlin are generated from the JSON. |
| UI | Native controls and navigation, themed by the tokens. Liquid Glass (iOS 26+) and Material 3 Expressive on the system chrome; the warm palette on content. |
| API generation | `buf` remote plugins, pinned, output **committed** (see below). |
| Cache | Lightweight file cache of serialized protobufs, not SwiftData or Room (see "Offline cache"). |
| DI | Manual (`AppContainer`), no Hilt, no Swinject. |

## Layout

```
mobile/
  README.md  PARITY.md
  tokens/
    tokens.json            exported from client/design (committed)
    generate.py            tokens.json -> Swift + Kotlin (python3 -I, stdlib only; --check for CI)
    fonts/                 Instrument Sans (variable) + JetBrains Mono, with their OFL licences
  api/
    buf.gen.yaml           buf v2 template, pinned remote plugins
    generate.sh            runs buf at the repo root; --check for CI
  ios/
    Shared/DesignTokens.generated.swift     Theme, ThemeSpec, palette, type, metrics, constants
    Packages/HideoutAPI/                    SwiftPM: generated messages + Connect clients
    Hideout.xcodeproj (from project.yml), Hideout/, HideoutTests/, Packages/HideoutDesign, Packages/HideoutCore, Packages/HideoutUI
    (next) extensions
  android/
    design/src/main/kotlin/app/thehideout/design/DesignTokens.kt
    api/src/main/{java,kotlin}              generated protobuf-javalite + connect-kotlin clients
    settings.gradle.kts, gradle/libs.versions.toml, gradlew      the Gradle project (wrapper 9.9.2)
    app/   core/   ui/                                          application, JVM core, Compose UI
```

### Token pipeline

```sh
# 1. export from the design crate (CARGO_TARGET_DIR as usual)
cd client && cargo run -p thehideout-design --example export_tokens -- ../mobile/tokens/tokens.json
# 2. generate Swift and Kotlin
python3 -I mobile/tokens/generate.py          # writes both files
python3 -I mobile/tokens/generate.py --check  # CI: exit 1 if stale
```

`tokens.json` holds all **320 looks** (8 accents x 5 backgrounds x dark/light x tint x contrast) fully resolved to sRGB hex, exactly as desktop computes them (`fit` walks lightness until contrast passes, which deltas cannot reproduce), plus the channel palette (12 keys, icon ink and text-safe ink per look), the 8 avatar fills per accent and the FNV-1a avatar rule, radii, the 4 pt spacing scale, icon sizes, the type scale with the Instrument Sans / JetBrains Mono families, `Metrics` (row, control, header, footer, message padding, avatar and rail sizes) for every density x text size (2 x 4) with the type ramp scaled for each, motion springs (`duration`, `bounce`) and elevation shadows. Spot checks against `client/design/README.md`: dark `bg #16120e`, `surface #1e1a16`, `accent #68bea8`; light `bg #f6f3ef`, `accent_text #187363`; avatar fills `#8f4244 #136d5b …`.

Fonts: both are SIL OFL 1.1 (licence files are next to the fonts; ship them in the app's acknowledgements). Instrument Sans is a variable font: the generated code requests exact weights (450, 530, 650 are used on desktop) through the `wght` axis.

In the generated files the 320 looks are packed as one hex string each (`RRGGBBAA` x 45 tokens, then 12 channel keys x ink/text). That compiles in seconds (a 14k-element array literal does not) and decodes lazily per `Theme`.

### API pipeline

```sh
mobile/api/generate.sh           # buf generate at the repo root
mobile/api/generate.sh --check   # CI: fails when the committed output is stale
```

Plugins (pinned in `api/buf.gen.yaml`): `buf.build/apple/swift:v1.38.0`, `buf.build/connectrpc/swift:v1.2.3`, `buf.build/protocolbuffers/java:v36.2` (lite), `buf.build/protocolbuffers/kotlin:v36.2` (lite), `buf.build/connectrpc/kotlin:v0.10.0`. Outputs: `ios/Packages/HideoutAPI/Sources/HideoutAPI/Generated` (24 files), `android/api/src/main/java` (486) and `android/api/src/main/kotlin` (267).

**Why the output is committed and not generated in the build.** The apps then build offline with no BSR dependency, Xcode and Android Studio index the code without a pre-build step, reviewers see API-surface changes in the PR diff, and CI's `--check` guarantees the committed output matches `proto/`. The cost is repo size (about 7 MB), accepted. Keep `proto/` the only place a contract changes (additive only, see `proto/README.md`); after a proto change run `generate.sh` and commit the result in the same PR.

**Why one config at `mobile/api` and not per app.** A single template guarantees both platforms generate from the same revision of the protos, and the per-language outputs live where each toolchain expects them (a SwiftPM package; a Gradle module with conventional `src/main/{java,kotlin}`).

Generated names: Swift types are `Thehideout_V1_*` with `Thehideout_V1_*ServiceClient` / `...ClientInterface` (wrap them in the apps' own models, do not leak them into views); Kotlin is package `com.thehideout.v1` (`AuthServiceClient`, messages with Kotlin DSL builders).

Versions that are verified to compile together:

| Swift | Version | Kotlin / Android | Version |
|---|---|---|---|
| Xcode / Swift | 27.2 / 6.4 (language mode 6) | Gradle | 9.9.2 |
| connect-swift | 1.2.3 (exact) | AGP | 9.3.1 |
| swift-protobuf | 1.38.0 (exact) | Kotlin | 2.4.21 |
| iOS deployment target | 26.0 | connect-kotlin (+ okhttp, google-javalite-ext) | 0.10.0 |
| | | protobuf-javalite | 4.36.2 |
| | | OkHttp | 5.5.0 |
| | | Compose BOM / Material 3 | 2026.11.01 / 1.5.0 |
| | | compileSdk / targetSdk / minSdk | 37 / 37 / 26 (see below) |

`HideoutAPI` builds with `swift build` (macOS) and `xcodebuild -scheme HideoutAPI -destination 'generic/platform=iOS Simulator'`. The Kotlin side is the committed Gradle project in `android/` (phase 1). Two deviations from the table above, both forced by what resolves:

- **compileSdk 37, not 36.** Compose 1.13 (the BOM 2026.11.01 that carries Material 3 1.5.0, whose Expressive APIs the app is built on) and the adaptive libraries declare `minCompileSdk` 37 (one artifact 37.1). Only platform `android-37.0` exists in the SDK repository, so the project compiles against 37.0 and turns off AGP's `check*AarMetadata` tasks (see `android/build.gradle.kts`): the declaration is a floor for new API use, not a link-time requirement, and the libraries guard newer calls. Revisit when 37.1 ships. The older BOM 2026.09.00 has no `ToggleButton`, button groups or `LoadingIndicator` (Material 3 1.4), so it was not an option.
- Material 3 adaptive 1.3.0, activity-compose 1.13.0, lifecycle 2.11.1, DataStore 1.2.1, Roborazzi 1.79.0 and Robolectric 4.18.1 are pinned in `android/gradle/libs.versions.toml`.

### Icons

Both apps take their icons from `branding/out` (`ios/AppIcon-1024.png`; `android/` adaptive foreground and background, monochrome, legacy mipmaps, Play Store 512), produced from the owner's pixel art in `branding/TheHideout.png` (both on `main`). The art is pixel art: **only nearest-neighbour scaling**, never smoothed. Do not recreate or resize icons by hand; copy from `branding/out` into the asset catalog / `res/` and regenerate there if the art changes.

## Architecture: iOS

SwiftUI, iOS 26+, Swift 6 language mode, Observation (`@Observable`), async/await. No Combine, no third-party UI frameworks.

```
Hideout.xcodeproj
  Hideout (app)            thin: @main, scene, AppContainer, deep links, extensions glue
  Shared/                  DesignTokens.generated.swift (a folder reference, compiled into the app and every extension)
  Packages/
    HideoutAPI             generated (done)
    HideoutCore            Foundation only, `swift test` on macOS:
                             Models (value types mapped from proto), Store, Reducer (apply.rs port),
                             GatewayStream, Cache, Markdown parser, Permissions, Perms tests
    HideoutUI              SwiftUI: screens, components, theme injection
  Extensions               ShareExtension, NotificationService, BroadcastUpload (screen share)
```

**State.** One `@MainActor @Observable AppStore` per signed-in session owns the whole mirror (guilds, channels, layouts, roles, members, users, per-channel logs, outgoing messages, voice states, unread, UI-adjacent state such as the current place). It is the Swift port of desktop's `AppState` + `apply.rs`: views read it, intents are methods (`send`, `select(channel:)`, `loadOlder`, ...), the reducer applies gateway events and RPC results with desktop's exact semantics (optimistic layout and role edits with rollback, client-nonce pending messages, retained scroll anchors). Screens take `@Environment(AppStore.self)` and derive small view models; no per-screen network code. A `SessionPhase` (`loading`, `signedOut`, `signedIn`, `termsRequired`, `updateRequired`, `suspended`) drives the root view.

**Networking.** `ProtocolClient` (connect-swift) with an interceptor that adds `authorization: Bearer <token>` and maps `UNAUTHENTICATED` to sign-out. Unary calls are `async`; uploads are `POST /attachments` with a background `URLSession` (`Authorization: Bearer`, raw body). Errors become a typed `AppError` (wrong code with attempts left, `RESOURCE_EXHAUSTED`, `UNAVAILABLE`, ...) via the `CodeError` detail.

**Gateway.** `actor GatewayStream` owns the server stream: opens `Subscribe`, forwards events to the store on the main actor, watches heartbeats for a dead stream, reconnects with backoff, and marks the stream `Interrupted`; the store then refetches the open channel and lists because the server does not replay. It runs only while the app is active (`scenePhase`); on foreground it reconnects and refetches. While backgrounded, push notifications wake the user, not a socket; the server is gaining them on branch `push-notifications` (a device-token registration RPC, APNs + FCM; MOB-01), so regenerate `mobile/api` when it merges.

**Offline cache.** A small file cache, not SwiftData: the data is a mirror the server owns and the gateway keeps current, so schema migrations and a query layer would cost more than they give. `Cache` writes the guild/channel/layout/user lists and the last 100 messages of each recently opened channel as SwiftProtobuf binary blobs under `Caches/` (one file per channel, atomic writes, size-capped like desktop's 64 MB media budget), loads them before the first network answer, and drops them on sign-out. Drafts, look, collapsed sections and recents live in `UserDefaults`; the session token lives in the Keychain (`AfterFirstUnlock`, no iCloud sync).

**Navigation.** iPhone, compact width: `TabView` with the Liquid Glass tab bar and four tabs, each with its own `NavigationStack`:

| Tab | Root | Pushes |
|---|---|---|
| Servers | server list, "+" menu | server > channel list (sections) > channel (transcript) |
| DMs | conversations, compose button | DM transcript |
| Mentions | local feed of messages that mention you (from gateway events and unread), tap to jump | channel at that message |
| You | profile header, Settings, Security, Admin (admins), About | settings pages |

Voice: the mini player is a `tabViewBottomAccessory` that expands into the call screen. Members, emoji picker, message menu, create/join/invite and most dialogs are sheets with detents; destructive confirmations are `confirmationDialog` or `alert`. iPad and regular width: a single `NavigationSplitView` with the server rail as the leading column, channel list as the sidebar, transcript as detail and members as an `inspector`; the tab structure collapses into the sidebar (`TabView` with `.tabViewStyle(.sidebarAdaptable)` is the first thing to try). Deep links (`thehideout://invite/<token>` and the universal link) route through `onOpenURL` into the store.

**Local dev.** The simulator shares the Mac's network: server URL `http://127.0.0.1:8080`. Allow cleartext to localhost with `NSAllowsLocalNetworking` (not `NSAllowsArbitraryLoads`). A physical device needs the Mac's LAN address (and the same exception for local networking). Dev servers: `docker compose up` per repo README.

## Architecture: Android

Jetpack Compose, Material 3 Expressive, Kotlin coroutines and Flow, edge-to-edge, predictive back. minSdk 26, targetSdk the latest installed.

```
settings.gradle.kts, gradle/libs.versions.toml, gradle wrapper
  :app        Application, MainActivity (single activity), AppContainer, deep links, services
  :core       pure Kotlin/JVM (testable without Android): models, Store, Reducer (apply.rs port),
              GatewayStream, Cache, Markdown parser, Permissions
  :ui         Compose screens, components, navigation, theme composition
  :design     generated DesignTokens.kt (done)
  :api        generated protobuf-javalite + connect-kotlin clients (done)
```

**Modules as built (phase 1).** `:api` and `:core` are plain Kotlin/JVM libraries (protobuf-javalite and connect-kotlin are JVM libraries, so `:core` depends on `:api` directly and everything in it, including the real `ConnectApiClient`, the reducer and the cache, is tested by `./gradlew :core:test` in seconds with JUnit 5, Turbine and coroutines-test, no Android). `:design` is an Android library (Compose types, `res/font`). `:ui` holds the theme, components, the gallery, the shell and the Roborazzi tests. `:app` holds `MainActivity`, `AppContainer`, the Keystore token store and the manifest; the Android-specific `KeystoreTokenStore` is a thin key provider over `:core`'s `AesGcmBox`, which is unit tested on the JVM. Kotlin warnings are errors in `:core`, `:ui` and `:app`.

**State.** One `AppStore` per session (the Kotlin twin of the iOS store, same reducer, same fixtures) exposed as `StateFlow<AppState>` immutable snapshots; screen `ViewModel`s (`androidx.lifecycle`) map slices of it with `stateIn` and call store intents. Single-source-of-truth, unidirectional: `UI event -> ViewModel -> Store intent -> RPC/gateway -> Reducer -> new state -> UI`. **DI:** manual `AppContainer` created in `Application.onCreate` and passed through a `ViewModelProvider.Factory`; Hilt is skipped because the graph is small (store, client, cache, prefs, LiveKit controller) and kapt/KSP time and indirection buy nothing here.

**Networking.** connect-kotlin `ProtocolClient` over a shared `OkHttpClient` (HTTP/2, 30 s read timeout except the gateway stream), an interceptor for `authorization: Bearer`, and `UNAUTHENTICATED` handling as on iOS. Uploads: OkHttp `RequestBody` streaming from a `ContentResolver` input stream, run in a `WorkManager` expedited job for large files.

**Gateway.** `GatewayStream` wraps the server stream in a `callbackFlow`-style coroutine with the same heartbeat watchdog, backoff and `Interrupted` refetch as iOS; collected in the process while the app is in the foreground, plus while a foreground service (voice) is alive.

**Offline cache.** Same decision as iOS: files of serialized protobuf-lite bytes under `cacheDir` (not Room), written atomically and size-capped; preferences in Preferences DataStore; the token in an Android-Keystore-backed `EncryptedFile`/Tink AEAD (never in plain DataStore).

**Navigation.** `NavigationSuiteScaffold` chooses a bottom `NavigationBar` (compact) or a navigation rail (medium and expanded) for the same four destinations (Servers, DMs, Mentions, You). Servers and channels use the Material 3 adaptive `NavigableListDetailPaneScaffold` (channel list | transcript, with the member list as an extra pane on expanded widths); on phones it is a normal back stack with predictive back. Navigation 3 or Navigation Compose with type-safe routes; deep links `thehideout://invite/<token>` and the https App Link go through the same routes. The voice mini player sits above the navigation bar and expands into the call screen.

**Local dev.** The emulator reaches the host at `http://10.0.2.2:8080`; allow cleartext only in a **debug** `network_security_config` for `10.0.2.2` and `localhost` (release builds are HTTPS-only). Physical devices use the LAN address or `adb reverse tcp:8080 tcp:8080` with `127.0.0.1`.

### Android: running and testing (phase 1)

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"   # any JDK 17+; CI uses 21
cd mobile/android
./gradlew :core:test :app:assembleDebug        # unit tests and the debug APK
./gradlew :ui:recordRoborazziDebug             # re-record the references in ui/src/test/snapshots
./gradlew :ui:verifyRoborazziDebug             # compare against them (CI)
```

Debug launch flags (intent extras, ignored by release builds): `adb shell am start -n si.zabit.thehideout/.MainActivity --ez fake true` runs the whole app on the in-memory `FakeBackend` (seeded like desktop's `fake.rs`: ana, bruno, cvetka and tim, The Hideout with sections and voice channels, two DMs, a few unread messages); `--ez gallery true` opens the design gallery; the debug build also has a second launcher entry, "Hideout gallery". The debug build talks to `http://10.0.2.2:8080` (a debug-only network security config allows cleartext to 10.0.2.2, localhost and 127.0.0.1; release is HTTPS-only). The signed-out screen has "Explore with demo data" on debug builds.

The snapshot matrix is the gallery (phone, captured tall), the shell and its screens (phone and tablet, dark and light), the signed-out screen, the look controls and four extra looks (Ocean/Cool, Ember/Paper light, High contrast + Compact + text 125%). They run on Robolectric with native graphics, on the JVM, pinned to UTC and the fake world's clock. References are recorded on macOS; text anti-aliasing differs slightly between macOS and Linux, so CI compares with a small tolerance (`changeThreshold` 0.5%) and uploads the diffs when it fails.

## Theming

Generated tokens feed the platform theme; native chrome is left to the OS.

```
tokens.json --generate.py--> DesignTokens (Theme = ThemeSpec + system appearance)
                                  |
   iOS: @Entry theme in the Environment, .tint(accent), .preferredColorScheme
   Android: Theme.toColorScheme() -> MaterialExpressiveTheme(colorScheme, typography, shapes, motionScheme)
```

**Resolving a look.** `Theme(spec, systemDark)` picks one of the 320 packed looks by desktop's `look_index` and decodes it; `Metrics.of(density, textSize)` supplies sizes. The app holds the `ThemeSpec` (persisted as the same JSON desktop writes) and re-resolves on change or when the system appearance flips. Colours cross-fade on `Motion.reflow` unless Reduce Motion is on.

**iOS 26+ Liquid Glass with the warm palette.** The rule: *glass for chrome, tokens for content.*
- Navigation bars, toolbars, tab bar, `tabViewBottomAccessory`, sheets and menus are the system's: do not give them backgrounds or hide their glass. Set `.tint(theme.colors.accent)` so toolbar items, selection and the prominent button style pick up the accent; use `.buttonStyle(.glass)` / `.glassProminent` and `GlassEffectContainer` for floating controls (send button, jump-to-unread, voice controls).
- Content surfaces are `bg`, `surface`, `surface_raised`, `surface_overlay` from the palette, so the glass refracts warm tones, not system grey. The window background is `theme.colors.bg`; list rows sit on `surface`.
- Text is `text`, `text_muted`, `text_faint`; the accent as text is `accentText`. Fonts are Instrument Sans through `TypeStyle.font(scale:)` (scale from `@ScaledMetric` so Dynamic Type applies on top of the in-app text size); navigation titles may stay system (SF) where the system draws them: only content text is Instrument Sans.
- Increase Contrast, Reduce Transparency and the app's High contrast setting: use `ContrastLevel.high` looks and let the system flatten glass. Never put text directly over glass without a token background behind it.
- Crisp corners (radii 2-10 pt) are the desktop language; use `Radius.*` for custom views (message rows, chips, tiles) and leave system controls to their own shapes and concentricity.

**Android Material 3 Expressive with the warm palette.**
- `ThemeColors.toColorScheme(dark)` maps the semantic tokens onto every `ColorScheme` role (primary = `accent`, primaryContainer = `accentSubtle`, surface containers climb `bg < surface < surfaceRaised < surfaceOverlay`, error = `dangerSolid`, outline = `controlBorder`, ...), so stock components (buttons, switches, text fields, bottom sheets, navigation bar, dialogs) wear the palette. Dynamic color (wallpaper) is **not** used: the look is the product; offer none.
- `MaterialExpressiveTheme` with `MotionScheme.expressive()` for component motion; our `Motion.*` springs (`SpringToken.spec()`) for custom animations. Expressive shapes and button groups are used where they exist; message rows and chips follow `Radius.*`.
- Typography: `TypeRamp` to `Typography` (`bodyLarge` = body, `labelLarge` = control, `titleMedium` = title, `headlineSmall` = heading, `displaySmall` = display, mono for code) with `DesignFonts.ui` set at launch to the bundled variable font. System bars: edge-to-edge with transparent bars and `enableEdgeToEdge`, content drawn behind, insets handled per screen.
- Dark/light follow the system unless the user forces one; High contrast uses the `high` looks.

**Icons.** Chrome uses SF Symbols and Material Symbols (native look); desktop's Phosphor chrome set is not ported. User-chosen channel icons are Phosphor *fill* names stored on the server: a generator step (follow-up, PARITY DS-04) emits the glyph set from `client/design/src/icons/catalog` as vector data for both platforms.

## Testing

| Layer | iOS | Android |
|---|---|---|
| Unit (reducer, markdown, permissions, cache, gateway reconnect) | Swift Testing in `HideoutCore`, `swift test` on macOS (fast, no simulator) | JUnit5 + Turbine + coroutines-test in `:core` (JVM) |
| Contract fixtures | JSON vectors for markdown parsing, `perms`, `apply` events and token resolution exported from the desktop crate (follow-up); both apps and desktop run the same vectors | same files |
| Token regression | `generate.py --check` in CI; a test that `Theme()` dark bg is `#16120e` etc. | same |
| Snapshot | `pointfreeco/swift-snapshot-testing`: key screens x {dark, light} x {default look, Ocean/Cool, High contrast, Compact, text 125%} x iPhone and iPad sizes; the desktop scene names in `PARITY.md` are the screen list | **Roborazzi** (Robolectric, JVM, no emulator): same matrix on phone and tablet sizes |
| UI | XCUITest against `--ui-testing`, which swaps the network for a `FakeBackend` (desktop's `fake.rs` flows) | Compose UI tests (`createAndroidComposeRule`) against the same fake |
| End to end | A smoke suite against a real dev server (`docker compose up`, seeded) on a simulator/emulator, run nightly and on release branches; voice via a LiveKit dev server | same (Maestro flows may be shared) |
| Accessibility | Accessibility Inspector audits plus `performAccessibilityAudit()` in UI tests | Accessibility Test Framework checks in Compose tests, TalkBack pass per release |

A screen is not done until its snapshots exist for light and dark and it passes the accessibility audit.

## Phased build plan

Each phase ends with both apps working against a local dev server, snapshot tests green and the PARITY rows for that phase ticked on both platforms.

0. **Foundation** (this PR). Tokens, API clients, PARITY, architecture.
1. **App skeletons and plumbing.** Xcode project with local packages and Android Gradle project with modules, theming wired (`Theme` in the environment, Material scheme), fonts registered, `AppContainer`, `ProtocolClient`, Keychain/Keystore token store, `AppStore` + `Reducer` + `GatewayStream` in `HideoutCore` / `:core` with the contract fixtures, error presenter, fake backend. Debug "gallery" screen showing tokens and components (the mobile twin of `client/gallery`).
2. **Auth.** AUTH-01..15 including invite deep links, 2FA, native passkeys, reset, session restore, terms/update/suspended gates.
3. **Core chat.** Tabs/split view, server list, channel list with sections, transcript (markdown, mentions, jumbo, pending/failed), composer, reactions and emoji picker, DMs, unread and the Mentions feed, reconnecting state, local cache, attachments and embeds with lightbox.
4. **Management.** Create/join server and channel, invites, members and profiles, message edit/delete/reply, channel and section management with reordering, server settings (overview, roles, members, bans, emojis, danger zone), moderation.
5. **Settings and security.** Appearance (all axes, live preview), account security (email, password, TOTP, recovery codes, passkeys, devices), data export and account deletion, about/licences.
6. **Voice and video.** LiveKit join, voice rows, mini player and call screen, mute/deafen/PTT, camera, screen share (ReplayKit / MediaProjection), background voice (CallKit / foreground service), moderation actions, PiP.
7. **Admin.** Overview, servers, files, users, invites, audit.
8. **Polish and release.** Push notifications (needs the server work in PARITY), share extension, haptics, app lock, iPad/tablet refinements, accessibility pass, performance (transcript scrolling, image memory), app icons from `branding/out`, TestFlight and Play internal testing, store listings and privacy labels / Data safety form.

## CI

Added in this PR (`.github/workflows/ci.yml`):

- `client` job: re-exports `tokens.json` and diffs it against the committed copy, so a design-crate change cannot land without regenerating the mobile tokens.
- `mobile` job (macOS): `python3 -I mobile/tokens/generate.py --check`, `mobile/api/generate.sh --check`, and `swift build` of `Packages/HideoutAPI`.

To add as the apps land:

- iOS: the `ios` job (macOS 26, newest Xcode 26) runs `swift test` for `HideoutCore`, checks that `Hideout.xcodeproj` matches `project.yml`, builds the app for the simulator and runs the unit tests. The snapshot tests are skipped in CI (their references are recorded on one simulator runtime and Liquid Glass renders differently across runtimes; see `ios/README.md`). Still to add: UI tests, snapshot runs once a runner matches, diffs as artifacts, archive and TestFlight upload on tags.
- Android (Ubuntu runner): the `android` job runs `:core:test`, `:app:assembleDebug` and `:ui:verifyRoborazziDebug` today; still to add: `:app:lintDebug`, instrumented tests on a managed device nightly, bundle and Play internal upload on tags.
- A path filter so `mobile/**` and `client/design/**` changes run the mobile jobs and nothing else does.
- `buf breaking` already guards the protos; the `--check` jobs guard the generated clients.
