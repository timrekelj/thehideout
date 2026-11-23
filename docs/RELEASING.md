# Releasing the desktop app

Pushing a `v*` tag runs `.github/workflows/release.yml`. It builds the desktop app for macOS
(arm64 and x86_64, signed and notarized), Linux x86_64 and Windows x86_64, attaches the builds and
a `SHA256SUMS` file to a GitHub Release, and updates the Homebrew cask in
`timrekelj/homebrew-tap`.

| Platform | File | Notes |
| --- | --- | --- |
| macOS arm64 | `thehideout-<version>-macos-arm64.dmg` | signed, notarized, stapled |
| macOS x86_64 | `thehideout-<version>-macos-x86_64.dmg` | cross-built on an Apple silicon runner |
| Linux x86_64 | `thehideout-<version>-linux-x86_64.tar.gz` | built on Ubuntu 24.04 (glibc 2.39+) |
| Windows x86_64 | `thehideout-<version>-windows-x86_64.zip` | unsigned |

Linux and Windows are built with `--no-default-features`, which drops the camera (the capture
backend, nokhwa, only supports AVFoundation on macOS). Voice, screen share and the rest are in.
The macOS minimum is 11.0, set by `MACOSX_DEPLOYMENT_TARGET` in the workflow and `MIN_MACOS` in
`packaging/macos/bundle.sh`; keep both and the cask's `depends_on macos:` in step.

## One-time setup

### 1. The Homebrew tap

```sh
gh repo create timrekelj/homebrew-tap --public --description "Homebrew tap" --add-readme
```

The workflow creates `Casks/thehideout.rb` itself on the first release.

### 2. A token for the tap

Create a fine-grained personal access token at <https://github.com/settings/personal-access-tokens/new>:

- Repository access: only `timrekelj/homebrew-tap`
- Permissions: Contents → Read and write

```sh
gh secret set HOMEBREW_TAP_TOKEN --repo timrekelj/thehideout
```

Without this secret the Homebrew job is skipped with a warning.

### 3. The Apple Developer ID certificate

You need a paid Apple Developer account.

1. In Xcode (Settings → Accounts → Manage Certificates) or at developer.apple.com, create a
   **Developer ID Application** certificate.
2. In Keychain Access, find it under My Certificates, right-click → Export, and save it as a
   `.p12` with a password.
3. Find the signing identity name:
   `security find-identity -v -p codesigning` gives a line like
   `Developer ID Application: Your Name (ABCDE12345)`.
4. Create an app-specific password at <https://account.apple.com> → Sign-In and Security →
   App-Specific Passwords.

### 4. Set the secrets

```sh
repo=timrekelj/thehideout
base64 -i DeveloperID.p12 | gh secret set APPLE_CERTIFICATE_P12_BASE64 --repo $repo
gh secret set APPLE_CERTIFICATE_PASSWORD --repo $repo      # the .p12 password
gh secret set APPLE_SIGNING_IDENTITY --repo $repo --body "Developer ID Application: Your Name (ABCDE12345)"
gh secret set APPLE_ID --repo $repo                        # your Apple ID email
gh secret set APPLE_APP_SPECIFIC_PASSWORD --repo $repo     # from step 3.4
gh secret set APPLE_TEAM_ID --repo $repo                   # the 10-character team ID
```

Delete the `.p12` file afterwards.

If the certificate secrets are missing, the macOS jobs still run: the app is signed ad hoc, a
`::warning::` is shown, and the dmg works on the machine that built it but Gatekeeper blocks it
on other Macs. If only the notarization secrets are missing, the dmg is signed but not notarized.
That lets you try the workflow before the certificate exists.

## Cutting a release

1. Bump `version` under `[workspace.package]` in `client/Cargo.toml` and commit (this also updates
   `client/Cargo.lock`).
2. Tag and push:

   ```sh
   git tag v0.1.0
   git push origin main v0.1.0
   ```

The workflow fails early if the tag doesn't match the version in `client/Cargo.toml`. Tags with
a `-` (for example `v0.1.0-rc1`, with `0.1.0-rc1` in `Cargo.toml`) are published as pre-releases
and don't touch the Homebrew tap.

If a run fails after the release was created, fix the problem and re-run the failed jobs. The
release job uploads with `--clobber` into the existing release.

## What friends do

**macOS**

```sh
brew install --cask timrekelj/tap/thehideout
```

Or download the `.dmg` for their chip (Apple silicon: arm64, Intel: x86_64), open it and drag
thehideout to Applications. Update with `brew upgrade --cask thehideout`.

**Linux**: download `thehideout-<version>-linux-x86_64.tar.gz`, extract it and run
`./thehideout-desktop`. `INSTALL.txt` in the archive explains adding a launcher and making
`thehideout://invite/...` links open the app. It needs Vulkan drivers, ALSA, xkbcommon and
Wayland or X11.

**Windows**: download and extract the `.zip`, run `thehideout-desktop.exe`. The build is not
code-signed, so SmartScreen shows "Windows protected your PC": click **More info**, then
**Run anyway**.

## Testing the workflow

Run it without a tag:

```sh
gh workflow run release.yml --repo timrekelj/thehideout --ref <branch>
gh run watch --repo timrekelj/thehideout
```

A `workflow_dispatch` run builds all four platforms (the version is `<Cargo version>-dev`) and
keeps the results as workflow artifacts on the run page. It never creates a release or touches
the tap.

## Local macOS packaging

```sh
cd client && cargo build --release -p thehideout-desktop && cd ..
packaging/macos/bundle.sh --binary client/target/release/thehideout-desktop \
  --version 0.1.0 --arch arm64 --out /tmp/hideout-dist
```

Without `APPLE_SIGNING_IDENTITY` in the environment this signs ad hoc. Then verify with
`codesign --verify --deep --strict --verbose=2 /tmp/hideout-dist/thehideout.app`.
