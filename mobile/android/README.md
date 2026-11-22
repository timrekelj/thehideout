# thehideout Android

Jetpack Compose (Material 3 Expressive) app for Android. minSdk 26, targetSdk/compileSdk 37.

## Modules

```
:app     Application, MainActivity (single activity), AppContainer, Keystore token store, manifest, debug gallery
:core    pure Kotlin/JVM (tested without Android): models, AppStore + Reducer, GatewayStream, cache,
         ApiClient (ConnectApiClient over connect-kotlin/OkHttp, FakeBackend), the auth state machine
:ui      Compose: theme, components, gallery, shell, auth screens, Roborazzi snapshots + Compose UI tests
:design  generated DesignTokens.kt (from mobile/tokens) + bundled fonts
:api     generated protobuf-javalite + connect-kotlin clients (committed; see ../api)
```

Kotlin warnings are errors in `:core`, `:ui`, `:app`.

## Deviations

- **compileSdk 37, not 36.** Compose 1.13 (BOM 2026.11.01, which carries Material 3 1.5.0's Expressive
  APIs) and the adaptive libraries declare `minCompileSdk` 37 (one artifact 37.1); only platform
  `android-37.0` exists in the SDK repository, so the project compiles against 37.0 and disables AGP's
  `check*AarMetadata` tasks (see the root `build.gradle.kts`). Revisit when 37.1 ships.
- Material 3 1.5 deprecates the `VisualTransformation` text-field flavors; the auth fields keep the
  outlined look under `@Suppress("DEPRECATION")` until they move to `OutlinedSecureTextField`.

## Build, test, run

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"   # any JDK 17+; CI uses 21
export ANDROID_HOME="$HOME/Library/Android/sdk"
cd mobile/android
./gradlew :core:test :app:assembleDebug        # unit tests and the debug APK
./gradlew :ui:testDebugUnitTest                # Compose UI tests against the fake backend
./gradlew :ui:recordRoborazziDebug             # re-record the snapshot references
./gradlew :ui:verifyRoborazziDebug             # compare against them (CI, 0.5% tolerance)
```

Debug launch flags (intent extras, ignored by release builds):

```sh
adb shell am start -n si.zabit.thehideout/.MainActivity --ez fake true     # the whole app on FakeBackend
adb shell am start -n si.zabit.thehideout/.MainActivity --ez gallery true  # the design gallery
```

The debug build talks to `http://10.0.2.2:8080` (debug-only `network_security_config` allows cleartext
to 10.0.2.2/localhost/127.0.0.1; release is HTTPS-only and defaults to `https://thehideout.zabit.si`).

## Authentication and onboarding (phase 2)

The root view switches on `SessionPhase` (`Loading`, `SignedOut`, `SignedIn`, `TermsRequired`,
`UpdateRequired`, `Suspended`). While signed out, `AuthScreen` renders the auth card; everything it
does goes through `AuthFlow` in `:core` — a pure-Kotlin state machine (one immutable `AuthUiState` in
a `StateFlow`, changed only through the `AuthPresenter` methods) whose every transition is unit
tested in `AuthFlowTest` against `FakeBackend`. The flows and copy are desktop's (`ui/auth.rs`,
`forms.rs`, `account.rs`): password, emailed code (6 digits, auto-submit, 30 s resend countdown),
second factor (TOTP / recovery code / passkey, attempts left, "Start again" when locked), passkey
sign-in, password reset, invite code → preview → accept (terms + age gates), suspension card.
Errors land under the field they belong to, a form-wide one above the button, and neither reveals
whether an account exists.

- **Server calls.** `ApiClient` gained the unauthenticated RPCs (`Login`, `RequestEmailCode`,
  `VerifyEmailCode`, `CompleteSecondFactor`, `Begin/FinishSecondFactorPasskey`,
  `RequestPasswordReset`, `ResetPassword`, `Begin/FinishPasskeyLogin`, `Begin/FinishPasskeyRegistration`,
  `LookupInvite`, `AcceptInvite`, `AcceptTerms`). `ConnectApiClient` implements them over
  connect-kotlin; `FakeBackend` mirrors desktop's `fake_account.rs` (code `123456`, `dora` has TOTP,
  `tim` has a passkey, invites `WXYZ-2345`/`EXPD-0001`/`RVKD-0001`/`USED-0001`).
- **Session restore.** `MainActivity` uses the SplashScreen API (`installSplashScreen` +
  `setKeepOnScreenCondition { phase == Loading }`), so a saved session never flashes the sign-in
  screen. `AppStore.bootstrap` loads the cache, asks `GetMe` (an unaccepted `terms_update_required`
  puts up the terms gate), then `loadInitial` and the gateway. Only a refused token
  (`UNAUTHENTICATED`) clears the Keystore token; a network failure keeps it for next launch.
  `AcceptTerms` on the gate re-bootstraps into the app.
- **Server URL.** The sign-in card's "Server: … · Change" opens a sheet that probes the URL
  (`HEAD /`) and persists it in DataStore; changing it signs the old session out and rebuilds the
  client/store/auth against the new server. Debug default `http://10.0.2.2:8080`, release
  `https://thehideout.zabit.si`.
- **Invite codes and deep links.** The code field auto-formats `XXXX-XXXX` (uppercase, dash after
  the fourth character, I/L/O forgiven as 1/1/0); pasted links are rejected with "Paste just the
  code from your invite email, not the link." `thehideout://invite/<token>` and the App Link
  `https://thehideout.zabit.si/invite/<token>` (`autoVerify` intent filter) send the token as is.
  `AcceptInvite` requires `accepted_terms_version` and `age_confirmed`; the accept screen links
  `{server}/terms` and `/privacy` (Custom Tabs / LinkAnnotation) and disables "Create account"
  until both boxes are ticked.
- **Passkeys.** `Begin*` return `options_json` — WebAuthn JSON, exactly what Credential Manager's
  `GetPublicKeyCredentialOption` / `CreatePublicKeyCredentialRequest` take — so `PasskeyLauncher`
  runs the ceremony natively (`androidx.credentials`) and sends the `PublicKeyCredential` JSON to
  `Finish*`. **Native passkeys need the server to serve `/.well-known/assetlinks.json` on the RP-ID
  domain** (the host of `THEHIDEOUT_PUBLIC_URL`, e.g. `thehideout.zabit.si`) with the app's signing
  cert SHA-256:

  ```json
  [{
    "relation": ["getlogin"],
    "target": {
      "namespace": "android_app",
      "package_name": "si.zabit.thehideout",
      "sha256_cert_fingerprints": ["<SHA-256 OF THE SIGNING CERTIFICATE, UPPERCASE HEX>"]
    }
  }]
  ```

  Until that is hosted (and for non-domain hosts like `localhost` or `10.0.2.2`, where WebAuthn
  cannot have app passkeys), the launcher falls back to opening the server's `ceremony_url` in a
  Custom Tab and polling `Finish*` with an empty `credential_json` every 1.5 s — the page's redirect
  is a 127.0.0.1 loopback, unusable as a callback. Password fields carry autofill content types
  (`Username`/`Password`/`EmailAddress`/`NewPassword`/`SmsOtpCode`) so Credential Manager and the
  system autofill offer and save them.
- **Gates.** Terms update (accept or sign out), suspended (reason, until, contact hint), update
  required (`ClientTooOld`; the button opens the server base URL until there is a Play listing —
  TODO), and sign-out (clears the Keystore token, the cache and the state).

## Phase-1 review fixes (in the phase-2 PR)

- Mention count badges (channel rows, server tiles, the navigation bar, the gallery) were red. Red is
  for errors only: a mention pill is now the accent fill with `accent_fg` ink, like desktop.
- Channel rows used a 48 dp minimum *visual* height. They now use the tokens' density metrics
  (`Metrics.rowHeight` 32 dp comfortable / 28 compact, `rowGap`, `sectionHeight`, `railTile`) for the
  visual height, with the 48 dp minimum kept as a touch target via `minimumInteractiveComponentSize`,
  so the sidebar matches desktop's rhythm.

## Testing

- `:core:test` — JUnit 5 + Turbine + coroutines-test on the JVM: the auth state machine
  (`AuthFlowTest`: every transition, error mapping, invite normalization and link rejection,
  terms/age gating, deep-link parsing, session restore and the terms gate), plus the existing
  reducer/gateway/cache/error-mapping suites.
- `:ui:testDebugUnitTest` — Roborazzi snapshots (phone + tablet, dark + light) of every auth screen
  and the shell/gallery, and Compose UI tests (`AuthUiTest`) against the fake: sign in → shell,
  invite → accept → shell, wrong code → inline error, pasted link rejected.
- Real-server smoke runs happen on the emulator against `docker compose up -d` + `cargo run` in
  `server/` (see the repo README); the debug APK points at `http://10.0.2.2:8080`.
