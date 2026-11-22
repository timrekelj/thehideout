# Desktop parity inventory

Every feature and screen of the desktop app (`client/desktop`, `client/design`), with how each one is adapted for the native iOS (SwiftUI) and Android (Compose) apps. Both apps ship **all** of it; this file is the checklist the app workers tick off. Sources: `client/desktop/src/ui/*.rs`, `state.rs`, `forms.rs`, `backend.rs` (`Command`), `scenes.rs` and the scene names in `render.sh`, plus `proto/README.md` for server behaviour.

How to read it:

- **Status**: `☐` open, `◐` in progress, `☑` done (both platforms). Track per platform in the PR description; flip the cell only when iOS and Android both ship it.
- **Mobile adaptation**: what changes from desktop. "Same" means behaviour and copy match desktop; use the desktop string verbatim (`client/desktop/src/ui/*.rs`) unless noted.
- **iOS / Android**: the native component to build it from. iOS targets iOS 26+ (Liquid Glass applies to system bars, toolbars, tab bar, sheets and menus automatically; do not fake it on content). Android targets Material 3 Expressive on Compose, edge-to-edge, predictive back.
- Scene names in backticks are desktop's headless snapshot scenes (`client/desktop/render.sh`); render them for the reference picture.
- Items marked **(server)** need server work that does not exist on `main` yet; **(branch)** exist on an unmerged branch (see "Pending server branches" at the end).

General rules that apply to every row: the warm palette and type come from `mobile/tokens` (see README); every control has an accessibility label (VoiceOver / TalkBack), honours Dynamic Type / font scale on top of the app's own text-size setting, honours Reduce Motion, and has a 44pt / 48dp minimum touch target.

---

## 1. Auth and onboarding (`ui/auth.rs`, `forms.rs`, scenes `auth-*`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| AUTH-01 | Launch with a saved session shows a neutral loading screen, then the shell (`Screen::Loading`) | Splash while `GetMe` runs; keep the Keychain/Keystore token on 401 only if the failure was network, clear on `UNAUTHENTICATED` | SwiftUI root `switch` on `SessionPhase`; launch screen storyboard-free (`UILaunchScreen` colour) | `SplashScreen` API + same state machine | ☐ |
| AUTH-02 | Server URL field, "Server: … · Change" link (`auth-signin`) | Same, behind a quiet "Server" row on the sign-in screen; default from build config; validate with a `HEAD /` or a `GetMe` probe; `http://` allowed only for localhost/LAN (ATS exception / network security config in debug builds only) | `TextField` with `.keyboardType(.URL)` in a `Form` section | `OutlinedTextField`, `KeyboardType.Uri` | ☐ |
| AUTH-03 | Sign in with username or email + password (`auth-signin`) | Same; `textContentType(.username/.password)` so the system password manager and iCloud Keychain / Credential Manager autofill | `TextField`/`SecureField`, `.textContentType`, `.submitLabel(.go)`, `.scrollDismissesKeyboard(.interactively)` | `OutlinedTextField` + `autofill` semantics, Credential Manager `GetPasswordOption` | ☐ |
| AUTH-04 | Sign in with a code: "Email me a code", 6-digit entry, resend with 30 s countdown (`auth-code`) | Same; `oneTimeCode` autofill from the SMS/mail suggestion bar; auto-submit at 6 digits; resend timer survives rotation | `.textContentType(.oneTimeCode)`, segmented digit field | OTP field with `autofill(AutofillType.SmsOtpCode)`-equivalent (email: clipboard suggestion) | ☐ |
| AUTH-05 | Second factor challenge: authenticator code, recovery code toggle, passkey option, "Start again" on expiry (`auth-second-factor`) | Same; show attempts left from `CodeError`; expired challenge returns to step 1 | Pushed step in the auth `NavigationStack` | Same with `NavHost` | ☐ |
| AUTH-06 | Sign in with a passkey: username first, then browser ceremony and polling (`auth-passkey-waiting`) | **Use the platform passkey API directly** instead of the browser page: pass `options_json` to `ASAuthorizationPlatformPublicKeyCredentialProvider` / Credential Manager `GetPublicKeyCredentialOption`, send `credential_json` to `FinishPasskeyLogin`. Needs the RP id (server host) to publish `apple-app-site-association` (`webcredentials`) and `assetlinks.json` **(server: host those files; fall back to the ceremony URL in `SFSafariViewController` / Custom Tab until then)** | `AuthenticationServices`, `ASAuthorizationController`; fallback `ASWebAuthenticationSession` | `androidx.credentials` `CredentialManager`; fallback Custom Tabs | ☐ |
| AUTH-07 | Forgot password: request code, enter code + new password, signs out every session (`Reset your password`) | Same two steps; strong-password autofill (`newPassword` content type) | `.textContentType(.newPassword)` | Credential Manager `CreatePasswordRequest` after success | ☐ |
| AUTH-08 | Invite flow: "Have an invite? Join with a code", code `XXXX-XXXX`, lookup shows masked email, inviter, servers, expiry (`auth-invite-lookup`) | Same; also entered by a deep link `thehideout://invite/<token>` or the universal link `{PUBLIC_URL}/invite/<token>` (see MOB-05); code field auto-formats the dash and forgives I/L/O | Pasteboard-detect banner (`PasteButton`), `onOpenURL` | Intent filter + `ClipboardManager` chip | ☐ |
| AUTH-09 | Accept invite: choose username and password (or none), creates the account and first session (`auth-invite-accept`) | Same; errors inline (taken username keeps the invite usable) | `Form` with validation footers | Same with supporting text | ☐ |
| AUTH-10 | Terms and age: accept terms version and confirm minimum age 16 when accepting an invite; "terms update required" gate after sign-in **(branch `account-lifecycle`)** | A required checkbox row linking `{base}/terms` and `{base}/privacy` in an in-app browser; blocking full-screen "Accept updated terms" when `GetMe.terms_update_required` is set | `Toggle` + `SFSafariViewController` sheet; `.fullScreenCover` gate | `Checkbox` + Custom Tabs; full-screen destination gate | ☐ |
| AUTH-11 | Add a passkey after signing up prompt ("Welcome to thehideout … Protect it now?", Skip) | Same, shown once after account creation | `.sheet` with `presentationDetents([.medium])` | `ModalBottomSheet` | ☐ |
| AUTH-12 | Add-email banner for legacy accounts without an email (`add-email-banner`) | Dismissible banner above the server list; opens the Change email flow (SEC-03) | `safeAreaInset(edge: .top)` banner | `Card` banner in the list header | ☐ |
| AUTH-13 | Sign-out clears local data and returns to sign-in (user menu "Sign out") | Also deletes the Keychain/Keystore token and cached messages | `ConfirmationDialog` | `AlertDialog` | ☐ |
| AUTH-14 | Signup is invite-only; sign-up form errors (`auth-signup-error`) | No standalone sign-up screen; "Have an invite?" is the only way in | n/a | n/a | ☐ |
| AUTH-15 | Device name sent on sign-in (`device_name`) | Send the user-visible device name (`UIDevice.current.name` needs entitlement on iOS 16+, so use model name; Android `Build.MODEL`) | `UIDevice.current.model` | `Build.MODEL` | ☐ |

## 2. Server rail (`ui/rail.rs`, `ui/shell.rs`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| RAIL-01 | Vertical rail: Home (DMs) tile plus a tile per server (icon or initials on an avatar fill), selected bar, unread dot and mention count badge | iPhone: the **Servers** tab shows servers as a list/grid of large rows (icon, name, unread, mentions) at the root of its stack; iPad/tablet: the rail returns as a leading column in the split view | `List` of rows / `LazyVGrid` on iPad; `NavigationSplitView` column | `LazyColumn` list; `NavigationSuiteScaffold` rail on large screens | ☐ |
| RAIL-02 | "+" tile: Create server / Join server menu (`Menu::Add`) | Toolbar "+" button with a menu | Toolbar `Menu` (glass) | `TopAppBar` action + `DropdownMenu` | ☐ |
| RAIL-03 | Server icon fetched from `/media/{key}`, cached | Image cache with disk budget (see README, 64 MB like desktop) | `AsyncImage` replacement backed by `URLCache`/own cache | Coil with disk cache | ☐ |
| RAIL-04 | Guild unread aggregates: any unread channel gives a dot, mentions give a count | Same badges on server rows; the app icon badge shows total mentions (needs push, MOB-01) | `.badge(Int)` on list rows | `BadgedBox` / `Badge` | ☐ |
| RAIL-05 | Server context menu (reorder is not a desktop feature) | Long-press a server row: Mark as read (local), Invite people, Server settings (permission gated), Leave server | `.contextMenu` with `preview` | `combinedClickable` + `DropdownMenu` | ☐ |
| RAIL-06 | Rail order | Same order as `ListGuilds`; no reorder (none on desktop) | n/a | n/a | ☐ |

## 3. Sidebar, channels, sections (`ui/sidebar.rs`, `ui/channels.rs`, scenes `sections`, `reorder-menu`, `channel-settings`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| SIDE-01 | Server header with menu: Invite people, Invite by email, Server settings, Create channel, Create section, Leave server | Large title (server name) with a toolbar "..." menu; items gated by permission exactly as `perms.rs` | `navigationTitle` + toolbar `Menu` | `LargeTopAppBar` + overflow menu | ☐ |
| SIDE-02 | Channel list grouped by sections (top level, then sections), collapsible sections remembered per guild | Same, in a `List` with collapsible `Section`s; collapsed state persisted per (guild, section) | `List` + `Section(isExpanded:)` (iOS 17+) | `LazyColumn` with sticky headers + chevrons | ☐ |
| SIDE-03 | Channel row: icon (Phosphor fill name, palette colour tint), name, unread bold, mention pill, selected highlight | Same; render the Phosphor glyph via a bundled glyph set (SF Symbols cannot show user-chosen Phosphor names, see README "Icons") | Row with template `Image`/custom glyph view, `.badge` for mentions | Row with `Icon` + `Badge` | ☐ |
| SIDE-04 | Voice channel rows list their participants beneath (avatars/names, speaking ring, muted, deafened, video, screen share) (`voice-sidebar`, `voice-sidebar-live`) | Same nested rows; speaking ring animates with `motion-control` | Nested rows in `List` | Nested items in `LazyColumn` | ☐ |
| SIDE-05 | User strip at the bottom: avatar, username, connection presence dot, mute, deafen, push-to-talk indicator, gear menu | Folded into the **You** tab (avatar header) and the voice bar (VOICE-03); no persistent strip | `TabView` "You" tab | `NavigationBar` "You" destination | ☐ |
| SIDE-06 | User menu: Profile, Settings, Security, Admin (admins), Sign out | You tab list rows | `List` with `NavigationLink` rows | `LazyColumn` of `ListItem` rows | ☐ |
| SIDE-07 | Channel menu: Edit channel, Move up/down, Move to section (submenu), Create channel here, Delete channel (`reorder-menu`, `channel-settings`) | Long-press a channel row: context menu; "Move to section" is a submenu; drag handle replaces Move up/down in edit mode | `.contextMenu` with nested `Menu`; `EditButton` + `.onMove` | `DropdownMenu` + a reorderable `LazyColumn` (drag handle, e.g. the `sh.calvin.reorderable` library) | ☐ |
| SIDE-08 | Section menu: Rename, Delete (children stay), Create channel here | Long-press section header / swipe actions | `.swipeActions` + `.contextMenu` | `SwipeToDismissBox` + menu | ☐ |
| SIDE-09 | Reorder channels and sections by dragging, optimistic with `expected_version` and rollback on `ABORTED` (`reorder-menu`) | "Edit" mode toggled from the server menu (needs MANAGE_CHANNELS); drag rows within and across sections, drag section headers; retry on `ABORTED` | `List` edit mode, `onMove`, `Transferable` for cross-section moves | Long-press drag with `Modifier.draggable` / reorderable list lib | ☐ |
| SIDE-10 | Create channel dialog: name, type (Text/Voice), section, icon, colour, topic (`dialog-create-voice`) | Sheet with a form; icon picker pushes inside the sheet | `.sheet` with `Form`, `presentationDetents([.large])` | `ModalBottomSheet` full height | ☐ |
| SIDE-11 | Channel settings dialog: rename, topic, icon, colour, delete (`channel-settings`) | Sheet from the channel menu or the channel header; Delete confirms by dialog | `.sheet` + `.confirmationDialog` | Bottom sheet + `AlertDialog` | ☐ |
| SIDE-12 | Channel icon picker: Icons tab (1,512 Phosphor fill glyphs, searchable by name/tags), Emoji tab (Unicode and server emoji) (`icon-picker-search`, `channel-icon-emoji-tabs`) | Full-screen sheet with search field, grid, segmented control for Icons/Emoji, "Use the default icon" | `.searchable` + `LazyVGrid`, `Picker(.segmented)` | `SearchBar` + `LazyVerticalGrid`, `SingleChoiceSegmentedButtonRow` | ☐ |
| SIDE-13 | Channel colour: 12 palette keys plus none, swatches (`palette.rs`) | Swatch grid in the form, shows `ink` colour for the current appearance | `LazyVGrid` of `Button`s | `FlowRow` of swatches | ☐ |
| SIDE-14 | Create / rename / delete section dialogs | Same as sheets/alerts; delete says channels stay | `.alert` with `TextField` (iOS 16+) | `AlertDialog` with `OutlinedTextField` | ☐ |
| SIDE-15 | Direct messages section in the sidebar (collapsible) with "New message" | Moves to the **DMs** tab (section 6) | n/a | n/a | ☐ |
| SIDE-16 | Mute for me / unmute (channel notification mute) is not on desktop | Mobile-only (MOB-02) | n/a | n/a | ☐ |
| SIDE-17 | Keyboard: channel stepping (Alt+Up/Down), Ctrl+Shift+A etc. | n/a on phone; iPad/Chromebook hardware keyboard shortcuts via `keyboardShortcut` / `onKeyEvent` for next/previous channel, mark unread, search | `.keyboardShortcut` in `Commands`/buttons | `Modifier.onKeyEvent`, `KeyboardShortcutGroup` | ☐ |

## 4. Transcript (`ui/transcript.rs`, scenes `main`, `main-avatars`, `empty-channel`, `reconnecting`, `jumbo-emoji`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| TRANS-01 | Channel header: icon, name, topic, members toggle, channel settings | Navigation bar title (name) with the topic as subtitle; trailing buttons: members (sheet), settings | `navigationTitle` + `navigationSubtitle` (iOS 26), toolbar items | `TopAppBar` title + subtitle, actions | ☐ |
| TRANS-02 | Virtualised message list, newest at bottom, sticks to bottom, loads older pages on scroll up, "Loading earlier messages…" | Inverted/anchored list; preserve scroll anchor when older pages or embeds arrive; pagination via `ListMessages` | `ScrollView` + `LazyVStack` + `scrollPosition`, `defaultScrollAnchor(.bottom)` | `LazyColumn(reverseLayout = true)` | ☐ |
| TRANS-03 | Message groups: avatar, author name (role colour), time, follow-ups collapse into the gutter with hover time | Same grouping rules (same author within window); long-press for time since no hover; role colour via `palette_text` | Custom row view | Custom item composable | ☐ |
| TRANS-04 | Day dividers ("Today", dates) | Same, floating date pill while scrolling (mobile nicety) | `safeAreaInset` header pill | Sticky header | ☐ |
| TRANS-05 | "New messages" divider at the first unread; Marker states Pending/After | Same; tapping "Jump to unread" capsule scrolls to it | Overlay capsule button | `SmallFloatingActionButton` / `AssistChip` | ☐ |
| TRANS-06 | Unseen-below pill: "N new messages" when scrolled up (`toast` "New messages") | Floating "↓ N new" button above the composer | Glass capsule `Button` | `ExtendedFloatingActionButton` | ☐ |
| TRANS-07 | Pending / failed delivery: pending messages at 55% opacity, "Failed" line with Retry (client nonce) | Same; swipe or tap to retry or delete a failed message | Row `.opacity`, `swipeActions` | Row alpha + `SwipeToDismissBox` | ☐ |
| TRANS-08 | Empty states: no channels yet, no messages ("Welcome to #name"), DM start, no conversations, "Couldn't load messages" with Try again | Same copy; illustration-free `ContentUnavailableView` style | `ContentUnavailableView` | Custom `EmptyState` composable | ☐ |
| TRANS-09 | Reconnecting state: connection pill (Connecting / Reconnecting in N s / Offline) and a refetch on resume (`reconnecting`) | Banner under the navigation bar; on foreground resume resubscribe the gateway and refetch the open channel (no replay on server) | `safeAreaInset(edge: .top)` capsule | `AnimatedVisibility` banner in `Scaffold` | ☐ |
| TRANS-10 | Text selection and copy of message text as drawn | Selectable text in rows; long-press menu has Copy text | `.textSelection(.enabled)` | `SelectionContainer` | ☐ |
| TRANS-11 | New message entrance (6 pt rise + fade, only for live messages) | Same, skip when loading history or on Reduce Motion | `.transition` with `Motion.control` | `animateItem()` | ☐ |

## 5. Message rendering (`design/src/components/chat/body.rs`, `ui/attach.rs`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| MD-01 | Discord-flavoured markdown: `**bold**`, `*italic*`, `***both***`, `~~strike~~`, `__underline__`, headings `#`..`###`, lists (nested, ordered with start), quotes `>` and `>>>` | Own parser port (same rules and limits as `body.rs`: nesting 4, 16 markers, 4096 marks, 64 KiB); do not use `AttributedString(markdown:)` (different dialect) | `AttributedString` built from own AST rendered in `Text` | `AnnotatedString` from own AST | ☐ |
| MD-02 | Inline code and fenced code blocks (mono, language-coloured when known, `radius-md`, unclosed fence runs to end) | Horizontal scroll for long lines; Copy button on long-press; syntax colouring optional (v1: plain mono) | `ScrollView(.horizontal)` | `Modifier.horizontalScroll` | ☐ |
| MD-03 | Spoilers `\|\|text\|\|`: tap reveals, state local to the message | Same | `Text` with overlay tap gesture | `clickable` span | ☐ |
| MD-04 | Links: `[text](url)`, bare URLs, `<url>`; only http, https, mailto; tooltip with destination for masked links | Tap opens in-app browser (http/https) or mail; long-press shows the destination and Copy / Open / Share | `SFSafariViewController`, `.contextMenu` on link | Custom Tabs | ☐ |
| MD-05 | Mentions `@name`: chip (`accent_subtle`), row wash when it mentions me, role/palette colour | Same; tapping a mention opens the profile (PROF-01) | Inline chip via `AttributedString` attributes + `openURL` handler | `InlineTextContent` / clickable annotation | ☐ |
| MD-06 | Custom emoji `<:name:id>` and `<a:name:id>` inline at 20pt, `:name:` text when unknown | Same; animated ones play only as standalone elements (jumbo, reaction, picker) like desktop | Inline `Image` attachment (`Text` + `Image` interpolation) | `InlineTextContent` + Coil | ☐ |
| MD-07 | Jumbo emoji: a message of only emoji (<= 27) draws large (custom 48pt, Unicode 40pt) (`jumbo-emoji`) | Same | Rule in view model, `.font(size:)` | Same | ☐ |
| MD-08 | Backslash escapes, markers pair only when flanking text | Covered by porting `body.rs` tests as shared fixtures (`mobile/PARITY.md` follow-up: export desktop test vectors to JSON for both apps) | XCTest from JSON | JUnit from JSON | ☐ |
| MD-09 | Edited marker ("Edited today at 14:05") | Same, small faint text after the body | `Text` suffix | `Text` suffix | ☐ |

## 6. Attachments, embeds, lightbox (`ui/attach.rs`, `attach.rs`, scenes `attachments-*`, `lightbox`, `video-embed`, `embeds*`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| ATT-01 | Image attachments: inline thumbnail with aspect ratio from width/height, grids for several (`attachments-images`) | Same layouts (1, 2, 3, 4+ grid), blurred placeholder using w/h so nothing shifts | `AsyncImage`-like view with fixed aspect | Coil `AsyncImage` | ☐ |
| ATT-02 | Video attachments: poster, play in place (player created on first play, cap of 6 players) | Poster with play glyph; inline `VideoPlayer`, full-screen on tap | `AVPlayer` / `VideoPlayer` | Media3 `ExoPlayer` | ☐ |
| ATT-03 | Audio attachments: play/pause bar with progress | Compact player row | `AVPlayer` | Media3 | ☐ |
| ATT-04 | File attachments: file-kind icon, name, size (`human_size`), Save file | Tap opens share/save: Files app export / Storage Access Framework; `removed` attachments show "removed" placeholder | `ShareLink` / `UIDocumentPickerViewController(forExporting:)` | `ActivityResultContracts.CreateDocument`, `DownloadManager` | ☐ |
| ATT-05 | Lightbox: full-screen image/video viewer with scrim, close, open in browser, save (`lightbox`) | Pinch-zoom, double-tap zoom, swipe down to dismiss, swipe between a message's/channel's images, share and save to Photos | `fullScreenCover` with `matchedTransitionSource` zoom (iOS 18+), `MagnifyGesture` | Shared-element dialog, `Modifier.transformable`, `HorizontalPager` | ☐ |
| ATT-06 | Link embeds: link card (title, description, thumbnail, provider), image embed, video embed, rich post embed (author, published time, media gallery), provider look (`embeds`, `embeds-gallery`) | Same card, full width, tap opens URL; media opens in lightbox | Card view | `Card` | ☐ |
| ATT-07 | Collapse/expand an embed, state remembered (cap 400) (`embeds-collapsed`) | Chevron in the card header; persisted in the local store | `DisclosureGroup`-style | `AnimatedVisibility` | ☐ |
| ATT-08 | Embed arrives late (`message_updated`): eases in at the bottom, holds scroll anchor when reading history | Same rule | scroll anchor | `LazyListState` anchor | ☐ |
| ATT-09 | Composer tray: attach up to 10 files, chip per file with upload progress, remove, failed state; uploads start at once, pasted images accepted (`attachments-composer`) | "+" button opens photo picker, camera, files; paste from clipboard; same 10 file cap and 50 MiB default limit; upload via `POST /attachments` with Bearer token, background `URLSession` | `PhotosPicker` (multi), `UIImagePickerController`/`PHPicker`, `.fileImporter`, `PasteButton` | `PickVisualMedia` (multi), `TakePicture`, `OpenMultipleDocuments`, ClipData | ☐ |
| ATT-10 | Drop files to attach overlay | iPad/Android large screens: drag and drop onto the composer | `.dropDestination(for:)` | `dragAndDropTarget` | ☐ |
| ATT-11 | Send with attachments only (content may be empty) | Same | n/a | n/a | ☐ |
| ATT-12 | Download/save progress and "Open in browser" | Share sheet covers it; progress in the row | `ProgressView` | `LinearProgressIndicator` | ☐ |
| ATT-13 | Share into the app from other apps (mobile-only) | See MOB-04 | | | ☐ |

## 7. Message actions, reactions, emoji (`ui/emoji.rs`, `emoji.rs`, `moderation_ui.rs`, scenes `reactions`, `message-menu`, `message-editing`, `message-delete-confirm`, `emoji-*`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| MSG-01 | Hover action bar: React, Reply, More | Long-press opens the message menu with a quick-reaction row on top; **swipe right to reply**, **swipe left for react/more** | `.contextMenu` + custom preview, `.swipeActions` on rows (needs `List`) or `DragGesture` | Bottom sheet with reaction row; `SwipeToDismissBox` for reply | ☐ |
| MSG-02 | Message menu: Reply, Copy text, Edit message (own), Delete message (own, or MANAGE_MESSAGES) | Same items; destructive Delete at the bottom, confirms (`message-delete-confirm` copy: someone else's message wording differs) | Menu with `Button(role: .destructive)` | `ModalBottomSheet` list | ☐ |
| MSG-03 | Reply: desktop prepends `@name ` to the draft and focuses the composer (no reference or banner exists in the protocol) | Same behaviour, so replies stay compatible; a thread-style banner needs a protocol change **(server)** and is not part of parity | Composer text edit + focus (`@FocusState`) | Same via `TextFieldValue` | ☐ |
| MSG-04 | Edit in place: Esc cancels, Enter saves, 4000-char limit, "deleted while editing" notice (`message-editing`) | Edit mode swaps composer into edit state with Cancel / Save; 4000-char counter | Composer mode enum | ViewModel state | ☐ |
| MSG-05 | Delete confirm: "This message will be deleted for everyone" | Alert (own) or alert with moderation wording | `.confirmationDialog` | `AlertDialog` | ☐ |
| MSG-06 | Reaction chips: emoji, rolling count, yours highlighted, tap toggles, "+" opens picker, max 20 distinct, pop animation on new chip (`reactions`) | Same; long-press chip shows who is not available (server sends counts only), show emoji name | `Button` chips in a flow layout (`Layout`) | `FlowRow` of `FilterChip`s | ☐ |
| MSG-07 | Emoji picker: search, recent (24), this server, nine Unicode groups, 9x8 grid, section strip (`emoji-picker`) | Bottom sheet; medium detent, search focused expands to large; scope All / Unicode / Server as desktop | `.sheet` with `presentationDetents([.medium, .large])` | `ModalBottomSheet` | ☐ |
| MSG-08 | Unicode emoji set limited to Unicode 15.1 and older (no tofu) | Filter the bundled emoji data to <= 15.1 (or to what the OS font draws) | `Character.isEmoji` + bundled list | `EmojiCompat` / bundled list | ☐ |
| MSG-09 | Custom emoji: from server, animated first frame until hover, `<:name:id>` in composer | Animated play in picker only when the cell is long-pressed or visible one at a time; never under Reduce Motion | Image decoding via ImageIO frames | Coil GIF/animated WebP decoder | ☐ |
| MSG-10 | Emoji autocomplete: `:sm` shows up to 8 rows, server emoji first (`emoji-autocomplete`) | Suggestion strip above the keyboard | Custom strip in `safeAreaInset` | `LazyRow` above the field | ☐ |
| MSG-11 | Recents stored in prefs | Local store | `UserDefaults` | `DataStore` | ☐ |
| MSG-12 | Mention autocomplete | Desktop has none for `@`; not required (parity), optional enhancement | n/a | n/a | ☐ |

## 8. Composer (`ui/composer.rs`, scenes `attachments-composer`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| COMP-01 | Multi-line field, placeholder "Message #name" / "Message @name", Enter sends, Shift+Enter newline | Return inserts newline, send button sends (Enter-to-send toggle on hardware keyboard / iPad); field grows to ~6 lines | `TextField(axis: .vertical)` + `.lineLimit(1...6)` | `OutlinedTextField(maxLines = 6)` | ☐ |
| COMP-02 | Send button states: primary when sendable, ghost when empty, press scale 0.96 | Same, glass circular button | `Button` with `.buttonStyle(.glassProminent)` when sendable | `FilledIconButton` | ☐ |
| COMP-03 | Per-channel drafts kept while switching channels | Persist drafts per channel (survive process death) | `@SceneStorage` / store | `SavedStateHandle` / DataStore | ☐ |
| COMP-04 | Composer disabled in voice channels and when no permission | Voice channels have no text composer; show the voice stage instead | n/a | n/a | ☐ |
| COMP-05 | Emoji button and attach button in the composer | Leading "+" (attach) and trailing emoji button | Toolbar in field row | Row of `IconButton`s | ☐ |
| COMP-06 | Paste an image | Paste button/menu, keyboard image insertion (GIF keyboards) via `UITextView` image paste / `contentReceiver` | `PasteButton`, `.onPasteCommand` | `Modifier.contentReceiver` | ☐ |
| COMP-07 | Message length limit 4000 | Counter appears at 3800+, blocks send above 4000 | | | ☐ |
| COMP-08 | Sending clears the field at once (no animation); a failed send stays in the transcript as Failed with Retry | Same as TRANS-07 | | | ☐ |

## 9. DMs and Home (`ui/sidebar.rs`, `state.rs`, scenes `home-dms`, `home-empty`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| DM-01 | Home shows the DM list with avatar, name, unread count | **DMs tab** root list sorted like desktop; swipe to mark read | `List` + `.swipeActions` | `LazyColumn` + swipe | ☐ |
| DM-02 | "New message": choose someone from your servers (searchable), opens the DM (`OpenDm`) | Toolbar compose button opens a people picker sheet with search | `.sheet` + `.searchable` | `ModalBottomSheet` + `SearchBar` | ☐ |
| DM-03 | DM channel: transcript and composer, header shows the peer, "This is the start of your chat with name" | Same screens as channels (shared transcript) | shared | shared | ☐ |
| DM-04 | `dm_opened` event adds a conversation live | Same | | | ☐ |
| DM-05 | No conversations empty state ("Start a conversation with someone from your servers.") | Same | `ContentUnavailableView` | EmptyState | ☐ |
| DM-06 | Emoji picker in DMs is Unicode-only scope (no server emoji) | Same | | | ☐ |

## 10. Members panel and profiles (`ui/members.rs`, `ui/dialogs.rs` Profile, scenes `main-members`, `profile-dialog`, `member-menu-moderation`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| MEM-01 | Right-hand members panel ("Members · N"), toggled from the header | A sheet from the channel toolbar (iPhone); a trailing inspector column on iPad/tablet | `.sheet` (detents medium/large) / `.inspector` (iPad) | `ModalBottomSheet` / `PermanentDrawerSheet` side pane | ☐ |
| MEM-02 | Member rows: avatar, name in highest role colour, grouped under role headers (`members-role-<id>`) when any member has a role | Same list with role section headers; add search (mobile nicety, desktop has none in the panel) | `List` + `.searchable` | `LazyColumn` + `SearchBar` | ☐ |
| MEM-03 | Tap a member: menu with Kick, Ban (moderators only, hierarchy checked), role-aware (`member-menu-moderation`) | Tap opens profile sheet; moderation actions inside it (and long-press menu) | `.contextMenu`, nested sheet | bottom sheet | ☐ |
| MEM-04 | Members loaded on demand (`want_members`), `guild_member_joined/removed` events apply live | Same | | | ☐ |
| PROF-01 | Profile dialog of another user: avatar, username, role chips | Profile sheet from tapping an avatar, name or mention; "Message" button (opens DM) | `.sheet` with `presentationDetents([.medium])` | bottom sheet | ☐ |
| PROF-02 | Own profile: change avatar (PNG/JPEG/WebP/GIF up to 8 MiB, server crops 256x256), shows "This is how you look to everyone" | You tab > Profile; photo picker with crop preview | `PhotosPicker` + crop (system) | `PickVisualMedia` + crop | ☐ |
| PROF-03 | Avatars: deterministic fill by FNV-1a of user id (8 fills per accent), initials, rounded squares with the `Radius.avatar` rule, picture over initials | Generated helpers `AvatarStyle` in `mobile/tokens` | `AvatarView` | `Avatar` composable | ☐ |
| PROF-04 | Deleted users shown as "Deleted user" **(branch)** | Same | | | ☐ |

## 11. Server dialogs: create, join, invite (`ui/dialogs.rs`, scenes `dialog-create-server`, `invite-by-email`, `invite-reset`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| DLG-01 | Create server: name, icon upload | Sheet | `.sheet` form | bottom sheet | ☐ |
| DLG-02 | Join server with an invite code ("Paste an invite code from a friend.") | Sheet; also reached by deep link, paste detection | `.sheet` | bottom sheet | ☐ |
| DLG-03 | Invite people: shows invite code and link with Copy, pending invites list with Withdraw and Resend, Reset code (`invite-reset`) | Sheet with `ShareLink` for the link and QR code (mobile nicety) | `ShareLink`, `CIFilter` QR | `ShareCompat`/`Intent.ACTION_SEND`, ZXing-free QR via `QRCodeWriter` | ☐ |
| DLG-04 | Invite by email: address, optional server list (admins), "make admin" (admins) (`invite-by-email`) | Same form, permission gated | `Form` | form | ☐ |
| DLG-05 | Leave server (confirm; owners must transfer first) (`leave-server-confirm`) | Alert with destructive style | `.confirmationDialog` | `AlertDialog` | ☐ |
| DLG-06 | Delete server (type server name to confirm) (`server-danger-zone`) | Alert with text field, button disabled until match | `.alert` + `TextField` | `AlertDialog` + field | ☐ |
| DLG-07 | Transfer ownership: choose a member, confirm (`transfer-dialog`) | Member picker sheet then confirm | `.sheet` | bottom sheet | ☐ |
| DLG-08 | Toast notifications (`toast`): success/error/info, 3.5 s, queue of 3 | System-style transient banner at top (iOS) / `Snackbar` (Android); errors with Retry where actionable | Custom overlay (no system toast API), VoiceOver announcements | `SnackbarHost` | ☐ |
| DLG-09 | Confirm dialogs with "Deleting…/Saving…" busy states | Buttons show progress and disable | `ProgressView` in button | `CircularProgressIndicator` | ☐ |

## 12. Server settings (`ui/server_settings.rs`, `server_settings.rs`, `ui/emoji_settings.rs`, scenes `server-settings-*`, `server-danger-zone`)

Needs the matching permission for each page; non-permitted pages are hidden, partial ones are read-only as on desktop.

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| SSET-01 | Overview: server name, icon (PNG/JPEG/WebP/GIF up to 8 MiB), Save/Discard (`server-settings-overview`) | Pushed screen with a `Form` and Save in the toolbar (dirty-state aware) | `NavigationStack` > `Form` | `Scaffold` + sections | ☐ |
| SSET-02 | Roles: list (drag to reorder, up/down), create role, edit name, colour (12 keys), permissions (14 switches incl. voice ones), delete role, hierarchy limits ("Above your highest role", read-only view) (`server-settings-roles`, `-limited`, `-voice`) | Master-detail: list pushes the role editor; reorder via edit mode with drag handles; permission switches grouped | `List` + `onMove`, `Form` with `Toggle`s | reorderable `LazyColumn`, `Switch` rows | ☐ |
| SSET-03 | Roles `ReorderRoles` with version and rollback | Same optimistic flow | | | ☐ |
| SSET-04 | Members tab: search, assign/remove roles per member, kick, ban (`server-settings-members`) | Searchable list; tapping a member opens a role picker sheet (multi-select) with Kick / Ban in a menu | `.searchable`, `List` multi-select | `SearchBar`, checkbox list | ☐ |
| SSET-05 | Bans: list with reason and who banned, Unban (`server-settings-bans`), ban dialog with reason and "delete recent messages" window (last hour, 24 hours, 7 days, don't delete) (`ban-dialog`) | Same; swipe to unban | `.swipeActions` | `SwipeToDismissBox` | ☐ |
| SSET-06 | Emojis: upload (<= 200 per server, animated allowed), rename, delete, "Added by" (`server-settings-emojis`) | Photo picker; grid/list with swipe delete; name validation as desktop | `PhotosPicker`, `List` | `PickVisualMedia`, `LazyColumn` | ☐ |
| SSET-07 | Danger zone: Transfer ownership, Delete server (`server-danger-zone`) | Bottom section of Overview | `Section` with red buttons | red `OutlinedButton`s | ☐ |
| SSET-08 | Kick confirm dialog ("Kick {who}") | Alert | `.confirmationDialog` | `AlertDialog` | ☐ |

## 13. User settings (`ui/settings.rs`, `ui/voice_settings.rs`, scenes `settings-*`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| SET-01 | Appearance: accent (8 presets), background (5), appearance System/Light/Dark, tint toggle, contrast Standard/High, density Comfortable/Compact, text size 90/100/110/125, reduce motion (System/On) | Same controls with a live preview card (the "Bruno / ramen" sample conversation); follows system by default; stored locally, one `ThemeSpec` JSON identical to desktop's serde | `Form`, swatch grid, `Picker`, preview `Section` | `Scaffold` + `ListItem`s, swatches | ☐ |
| SET-02 | Reduce motion override | Default follows OS Reduce Motion / `ANIMATOR_DURATION_SCALE`; user can force On | `@Environment(\.accessibilityReduceMotion)` | `Settings.Global.ANIMATOR_DURATION_SCALE` | ☐ |
| SET-03 | Voice & audio: input device, output device, input mode (Voice activity / Push to talk), sensitivity slider with live level meter, mic test, noise suppression, echo cancellation, sounds for join/leave/mute, volume (`settings-voice`, `settings-voice-ptt`) | Device pickers use the audio route picker (system); PTT becomes an on-screen hold button (VOICE-08); sensitivity slider and level meter via LiveKit local audio level | `AVRoutePickerView`, `Slider` | `AudioManager` communication devices, `Slider` | ☐ |
| SET-04 | Voice hotkeys (mute, deafen, PTT key recorder) | n/a on phone (hardware keyboard on iPad/ChromeOS optional); iOS can bind Action Button / AirPods stem via CallKit mute; Android media-button `MediaSession` | CallKit mute action | `MediaSession` | ☐ |
| SET-05 | Settings entry points: Settings, Security | You tab rows | | | ☐ |
| SET-06 | Text size and density apply to every screen (Metrics) | Use `Metrics.of(...)`; combined multiplicatively with Dynamic Type | `@ScaledMetric` scaling | `LocalDensity.fontScale` | ☐ |
| SET-07 | App-level extras (mobile-only): notifications per server/channel, app lock, haptics toggle, data/caching, about/licences | See section 20 | | | ☐ |

## 14. Security and account (`ui/security.rs`, `account.rs`, scenes `settings-security*`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| SEC-01 | Security page: Email, Password, Two-factor, Passkeys, Devices sections (`settings-security`) | You > Security list | `Form` sections | `LazyColumn` sections | ☐ |
| SEC-02 | Change password (current, new, confirm; signs out other devices) | Same | `SecureField`s | fields | ☐ |
| SEC-03 | Add / change email: send code, confirm code (`ChangeEmail`, `ConfirmEmailChange`) | Same two steps in a sheet | `.sheet` | bottom sheet | ☐ |
| SEC-04 | TOTP setup: QR code, copyable key, confirm code, shows 10 recovery codes once (`settings-security-totp-setup`, `-recovery-codes`) | Same; **"Open in authenticator app"** button via `otpauth://` URL; recovery codes: Copy, Save to Files/Share, "I saved them" required | `UIApplication.open(otpauthURL)`, `ShareLink`, `UIPasteboard` | `Intent.ACTION_VIEW` `otpauth://`, share sheet, `ClipboardManager` | ☐ |
| SEC-05 | Disable TOTP (code or recovery code), Make new codes (regenerate, old stop working) | Same | alerts with fields | dialogs | ☐ |
| SEC-06 | Passkeys: list with created/used, add (ceremony), rename, remove (`BeginPasskeyRegistration`, `RenamePasskey`, `DeletePasskey`) | Add via platform passkey creation (see AUTH-06); swipe to remove, tap to rename | `ASAuthorizationPlatformPublicKeyCredentialRegistrationRequest`, `.swipeActions` | `CreatePublicKeyCredentialRequest`, swipe | ☐ |
| SEC-07 | Devices (sessions): list with "This device", last seen, sign out one, sign out of all other devices | Same | `List` + `swipeActions` | `LazyColumn` + swipe | ☐ |
| SEC-08 | Email notice / verification state (`email_verified`, "No email yet", Add email) | Same | | | ☐ |
| ACCT-01 | Account deletion: re-authenticate (password, or emailed code if passwordless; plus 2FA), blocked while owning servers (shows owned servers) **(branch)** | **Required by App Store 5.1.1(v) and Play policy: must be in-app.** You > Security > Delete account; owned servers list with "Transfer" shortcuts | `NavigationLink` flow, destructive `Button` | same | ☐ |
| ACCT-02 | Export my data (emails a link; rate-limited) **(branch)** | Row in You > Security with last-requested state | `Button` + toast | same | ☐ |
| ACCT-03 | Terms re-acceptance gate (see AUTH-10), links to Terms and Privacy in settings **(branch)** | You > About | `Link` | `uriHandler` | ☐ |
| ACCT-04 | Suspended account message ("Your account is suspended") **(branch)** | Full-screen state after sign-in attempt | `ContentUnavailableView` | EmptyState | ☐ |
| ACCT-05 | Minimum client version gate: "Update required — This version of thehideout is too old to connect" with a download link (`ui/update_required.rs`) | Full-screen blocker with "Update" opening the App Store / Play listing | `ContentUnavailableView` + `Link` (App Store URL) | EmptyState + Play Store intent | ☐ |

## 15. Voice and video (`voice.rs`, `voice_engine.rs`, `ui/voice_bar.rs`, `ui/voice_stage.rs`, scenes `voice-*`)

Uses the official LiveKit SDKs; `JoinVoice` returns `{ url, token }`, room `voice-<channel_id>`.

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| VOICE-01 | Join a voice channel (row "Join"), one at a time; joining another moves you | Tap a voice channel > full-screen call sheet or join button in row; request mic permission on first use | `LiveKit` `Room.connect`, `AVAudioSession` `.playAndRecord` | `io.livekit:livekit-android`, `RECORD_AUDIO` runtime permission | ☐ |
| VOICE-02 | Voice rows show participants with speaking indicator, mute/deafen/server-mute icons, camera and screen share icons (`voice-sidebar-live`) | Same in channel list; speaking ring from LiveKit active speakers | LiveKit `Participant.isSpeaking` | same | ☐ |
| VOICE-03 | Voice bar: connected channel name, mute, deafen, camera, share screen, disconnect, connection state ("Connecting…", "Reconnecting…") (`voice-connected`) | **Mini player** docked above the tab bar (like Now Playing) that expands to the call screen | `tabViewBottomAccessory` (iOS 26) | `BottomAppBar`-adjacent `Surface` + expandable sheet | ☐ |
| VOICE-04 | Mute / deafen toggles (deafen implies mute, server mute respected: "Muted by a moderator") (`voice-server-muted`) | Same; haptic on toggle | `Toggle` buttons | `IconToggleButton` | ☐ |
| VOICE-05 | Camera on/off with device pick | Front/back flip button; preview tile | `LocalVideoTrack` camera capturer | `LocalVideoTrack` + `CameraCapturer` | ☐ |
| VOICE-06 | Stage view: tiles for participants with video or screen shares, "Nobody has video on" empty state, "Back to chat" (`voice-stage`, `voice-stage-screen`) | Call screen grid (1-2 columns phone, adaptive iPad); tap a tile to focus; pinch zoom a screen share; PiP when leaving the app | `VideoView` from LiveKitComponents, `AVPictureInPictureController` | `VideoTrackView`, `PictureInPictureParams` | ☐ |
| VOICE-07 | Share screen: pick a screen or window (`voice-screen-picker`) | **Broadcast**: iOS ReplayKit Broadcast Upload Extension (system picker); Android `MediaProjection` foreground service; Stop sharing button | `RPSystemBroadcastPickerView` + extension target | `MediaProjectionManager.createScreenCaptureIntent` | ☐ |
| VOICE-08 | Push to talk (key held) | A large hold-to-talk button on the call screen when input mode is PTT; mic opens only while pressed | `DragGesture(minimumDistance: 0)` | `pointerInput` press | ☐ |
| VOICE-09 | Watch stream button on a voice row | Joins and opens that participant's tile | | | ☐ |
| VOICE-10 | Connection quality / reconnect, `voice_moved` event rejoin, `voice_snapshot` reset | Same logic ported; show banner "Reconnecting to voice" | | | ☐ |
| VOICE-11 | Participant menu: Mute for me (local only, per user), Server mute/unmute, Move to, Disconnect (moderators) (`voice-participant-menu`) | Long-press a participant tile or row; "Move to" submenu lists other voice channels | `.contextMenu` | `DropdownMenu` | ☐ |
| VOICE-12 | Voice sounds: join/leave/mute chimes | Bundled short sounds; respect silent switch? (play via `AVAudioSession` playback category in call) | `AVAudioPlayer` | `SoundPool` | ☐ |
| VOICE-13 | Voice channel creation and `MANAGE_*` voice permissions (CONNECT, SPEAK, VIDEO, MUTE_MEMBERS, MOVE_MEMBERS) | Enforced from server state; disabled buttons explain why | | | ☐ |
| VOICE-14 | LiveKit unavailable (`UNAVAILABLE` "voice is not configured") | Row disabled with explanation | | | ☐ |
| VOICE-15 | Background voice: keep the call alive when the app is backgrounded or the screen locks (**mobile-only**, see MOB-03) | CallKit/foreground service | | | ☐ |

## 16. Moderation (`moderation.rs`, `moderation_ui.rs`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| MOD-01 | Kick member (confirm, hierarchy aware) | Alert from member profile or settings | `.confirmationDialog` | `AlertDialog` | ☐ |
| MOD-02 | Ban member with reason, optional message cleanup window (1h, 24h, 7d, none), banned list with Unban | Sheet with reason field and `Picker` | `Form` sheet | form sheet | ☐ |
| MOD-03 | Delete others' messages with MANAGE_MESSAGES (different confirm copy) | In message menu | | | ☐ |
| MOD-04 | Server-mute, move, disconnect in voice (VOICE-11) | | | | ☐ |
| MOD-05 | Permission model (`perms.rs`): owner, ADMINISTRATOR, role OR, hierarchy | Port `perms.rs` with tests from shared vectors | | | ☐ |

## 17. Admin dashboard (`ui/admin.rs`, `admin.rs`, scenes `admin-*`) — site admins only

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| ADMIN-01 | Entry: user menu "Admin…" for `is_admin`; "Admin service not available" state with Try again (`admin-unavailable`) | You > Admin row, hidden for non-admins | | | ☐ |
| ADMIN-02 | Overview: users, active users today, sign-ups, messages today, live connections, storage by category, activity range selector and chart (`admin-overview`) | Scrollable cards and a `Charts` line/bar; range `Picker` | Swift Charts | Vico or Canvas chart | ☐ |
| ADMIN-03 | Servers: searchable list (name, owner, members, channels, messages, storage) (`admin-servers`) | Searchable list pushing a detail | `.searchable` | `SearchBar` | ☐ |
| ADMIN-04 | Files: list with filters (kind, server), size, select, select all, delete one or many with reason, progress "Deleting N of M…" (`admin-files`, `admin-files-delete`) | Edit mode multi-select, bulk delete bar, reason sheet | `List(selection:)` `EditButton` | selection mode `LazyColumn` | ☐ |
| ADMIN-05 | Users: search, status, last active, Make admin / Remove admin with reason, cannot demote yourself or last admin (`admin-users`) | Row menu / swipe, reason alert | `.swipeActions` | `SwipeToDismissBox` | ☐ |
| ADMIN-06 | Invites: list with status filter (Pending, Accepted, Expired, Revoked), search, create invite (email, servers to join, make admin), Resend, Withdraw, copy code/link (`admin-invites`) | Same with share sheet | `Form` sheet | form sheet | ☐ |
| ADMIN-07 | Audit log: time, actor, action, target, kind filter, paging ("Loading more…") (`admin-audit`) | Infinite scrolling list, filter menu | `List` | `LazyColumn` paging | ☐ |
| ADMIN-08 | Suspend, unsuspend, delete user **(branch)** | Row actions with reason | | | ☐ |

## 18. App shell, connection, system states

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| SYS-01 | Gateway stream (`GatewayService.Subscribe`): heartbeats, no replay, reconnect with backoff, clears voice states on open, `Interrupted` flag forces refetch (`reconnecting`) | One stream actor/coroutine per signed-in session; foreground only; refetch on resume; idle timeout detection via heartbeat | `actor GatewayStream` with connect-swift server stream | `Flow` from connect-kotlin `ServerOnlyStream` | ☐ |
| SYS-02 | All gateway events applied (message created/updated, reactions, channel/guild/layout/roles/member/user/emoji updates, DM opened, guild added/removed, member joined/removed, voice updates) (`apply.rs`) | Port `apply.rs` reducer with identical semantics; share event fixtures | `AppStore.apply(event:)` | `Reducer` | ☐ |
| SYS-03 | Optimistic edits with rollback (layout, roles) | Same | | | ☐ |
| SYS-04 | Unread tracking per channel, guild and home, `mentions_me` (`@username` match) | Local store of last-read ids (desktop has no server read state); sync across devices is not possible without server state **(server)** | | | ☐ |
| SYS-05 | Media cache budget 64 MB, decode by key (`media.rs`) | Disk + memory cache, same budget | | | ☐ |
| SYS-06 | Prefs saved locally: look, collapsed sections, collapsed embeds, recent emojis, drafts, server URL | Same set in `UserDefaults`/`DataStore` (token in Keychain/Keystore) | | | ☐ |
| SYS-07 | Crash reporting hook (`crash.rs`) | Sentry or MetricKit/Play vitals; no PII | | | ☐ |
| SYS-08 | Error mapping: `UNAUTHENTICATED` signs out, `RESOURCE_EXHAUSTED` message, `UNAVAILABLE` for optional services | One shared error presenter per app | | | ☐ |
| SYS-09 | a11y: every control labelled, focus order, contrast levels (`a11y_tests.rs`) | VoiceOver / TalkBack audit as part of definition of done | | | ☐ |
| SYS-10 | Small-window layout (`main-small`) | Compact width = iPhone layout; regular width = split view | `horizontalSizeClass` | `WindowSizeClass` | ☐ |
| SYS-11 | Look variants (`main-compact`, `main-text-125`, `main-ocean-cool`, `main-coral-paper-light`, `main-graphite-deep`, `emoji-picker-ocean`, `channel-settings-ocean`) | Snapshot-test each against the tokens | snapshot tests | snapshot tests | ☐ |

## 19. Design-system parity (`client/design`)

| ID | Desktop feature | Mobile adaptation | iOS | Android | Status |
|---|---|---|---|---|---|
| DS-01 | 320 looks (8 accents x 5 backgrounds x dark/light x tint x contrast), every semantic token | Generated `Theme` / `ThemeColors` from `tokens.json` | `Theme`, `@Entry theme` | `Theme`, `LocalTheme` | ☐ |
| DS-02 | Type scale (caption, label, control, body, title, heading, display, code) in Instrument Sans / JetBrains Mono | Bundle fonts, register at launch | `DesignFonts.register()` | `res/font` + `DesignFonts.ui` | ☐ |
| DS-03 | Radii, spacing (4 pt scale), icon sizes, springs, shadows | Generated constants | `Radius`, `Space`, `Motion` | same names | ☐ |
| DS-04 | Icons: Phosphor regular for chrome, Phosphor fill for user-chosen channel icons | Chrome uses **SF Symbols / Material Symbols** (native look, decided in README); user-chosen channel icons need the Phosphor fill set: bundle a subset or render from SVG path data generated from `client/design/src/icons/catalog` (follow-up generator) | SF Symbols + `PhosphorGlyph` view | `ImageVector`s generated | ☐ |
| DS-05 | Components: Button (Primary, Secondary, Ghost, Danger), IconButton, Switch, Checkbox, Select, Input, Menu, Dialog, Toast, Tooltip, Avatar, Badge, Chip, Tabs, Skeletons | Use native controls themed by tokens (README "Theming"), only custom where no native equivalent exists (reaction chips, mention chips, voice tiles, message rows) | native | native | ☐ |
| DS-06 | Contrast High mode and Reduce motion | Honoured by the generated palette and `Motion` | | | ☐ |

## 20. Mobile-only necessities (desktop lacks these)

| ID | Item | Plan | iOS | Android | Status |
|---|---|---|---|---|---|
| MOB-01 | **Push notifications for messages and mentions** | **(server) The server has no push support.** Needs: device-token registration RPC (`RegisterPushToken`/`UnregisterPushToken` with platform), APNs (token auth, `.p8`) and FCM v1 senders, notification payload policy (no message content in the payload, `mutable-content` for a Notification Service Extension that fetches the message), per-guild/channel mute settings, badge count. Apps: permission prompt after first sign-in, in-app settings, notification channels (Android), grouping by channel, inline reply action, tap opens the channel | `UNUserNotificationCenter`, APNs, `UNNotificationServiceExtension`, `UNNotificationAction` reply | FCM `FirebaseMessagingService`, `NotificationChannel`s, `RemoteInput` reply | ☐ |
| MOB-02 | Notification and mute settings per server and channel (mute, mentions only, all) | Local settings synced via the push registration payload **(server)** | `Form` | `Preferences` screen | ☐ |
| MOB-03 | Background voice | iOS: audio background mode + CallKit (`CXProvider`, `CXCallController`) so the call shows in the system UI and lock screen; Android: `ForegroundService` of type `microphone` (+ `mediaProjection` when sharing) with an ongoing notification and `ConnectionService`/Telecom self-managed optional; leave voice when the app is killed | CallKit, `UIBackgroundModes: audio, voip?` (use `audio` only: no PushKit without server VoIP push) | `FOREGROUND_SERVICE_MICROPHONE`, `POST_NOTIFICATIONS` | ☐ |
| MOB-04 | Share sheet and photo picker integration | Share extension: share images/files/text from other apps into a channel or DM (pick destination); camera capture; photo picker without library permission | Share Extension target (App Group for the session), `PHPicker` | `ACTION_SEND` intent filter + `ShareTarget` direct share, Photo Picker | ☐ |
| MOB-05 | Deep links and universal links for invites | `thehideout://invite/<token>` custom scheme (matches desktop `app.rs`) and `https://{PUBLIC_URL}/invite/<token>`; route to AUTH-08 when signed out or DLG-02 when signed in **(server: host `apple-app-site-association` and `assetlinks.json`)** | Associated Domains `applinks:`, `onOpenURL` | `intent-filter` with `autoVerify`, App Links | ☐ |
| MOB-06 | Haptics | Light impact on reaction, send, toggle mute; success/error notification on actions; respect system setting | `sensoryFeedback` (iOS 17+) | `HapticFeedback` | ☐ |
| MOB-07 | App lock (optional) | Biometric/passcode gate when returning after N minutes; hides content in the app switcher | `LocalAuthentication`, privacy overlay | `BiometricPrompt`, `FLAG_SECURE` option | ☐ |
| MOB-08 | iPad / tablet / foldable layouts | `NavigationSplitView`: rail + sidebar + detail, members as `inspector`; Android `NavigableListDetailPaneScaffold` + rail; Stage Manager / multi-window; hardware keyboard shortcuts | `NavigationSplitView`, `inspector`, `.keyboardShortcut` | Material3 adaptive, `NavigationSuiteScaffold` | ☐ |
| MOB-09 | Secure token storage | Keychain (`kSecAttrAccessibleAfterFirstUnlock`, no iCloud sync) / Android Keystore-backed `EncryptedFile` or DataStore with Tink | Keychain Services | `androidx.security` / Tink | ☐ |
| MOB-10 | Offline tolerance | Cache last N messages per channel and the guild/channel lists; show cached while loading; queue sends? (no: show Failed with Retry like desktop) | SwiftData or file cache (README) | Room (README) | ☐ |
| MOB-11 | App icon, launch assets, display name | Icons come from `branding/out` (branch `app-icon`); see README | Asset catalog `AppIcon` | `mipmap` / adaptive icon | ☐ |
| MOB-12 | Accessibility: Dynamic Type, VoiceOver rotor / TalkBack actions on message rows (reply, react, copy), Switch Control | Custom accessibility actions on rows | `.accessibilityAction` | `semantics { customActions }` | ☐ |
| MOB-13 | Localisation scaffolding | English only for v1; use String Catalog / `strings.xml` from day one | `Localizable.xcstrings` | `strings.xml` | ☐ |
| MOB-14 | Handoff of keyboard between composer and emoji picker, interactive dismiss, inset handling | Use system keyboard layout guides | `.scrollDismissesKeyboard` | `imePadding()` | ☐ |
| MOB-15 | Shortcuts / widgets (nice to have, post-1.0) | Mentions widget, "Open DM" App Intent | App Intents | Glance | ☐ |
| MOB-16 | In-app browser for links and terms | System browser sheets, never a raw `WKWebView` | `SFSafariViewController` | Custom Tabs | ☐ |

---

## Pending server branches (not on `main` at the time of writing)

- `account-lifecycle`: account deletion (`RequestAccountDeletionCode`, `DeleteAccount`), data export (`ExportData`), terms acceptance (`AcceptTerms`, `terms_version`, `age_confirmed` on `AcceptInvite`), admin suspend/unsuspend/delete user, minimum client version gate, `User.deleted`. The generated API clients in `mobile/api` do not contain these yet: after the branch merges, rerun `mobile/api/generate.sh` and commit (CI's `--check` will fail until you do). Items tagged **(branch)** depend on it.
- `app-icon`: brand artwork and generated platform icons in `branding/out` (MOB-11).

## Server work the apps need (not in this repository yet)

1. Push notification registration and delivery (APNs + FCM): MOB-01, MOB-02, RAIL-04 badge.
2. `apple-app-site-association` (webcredentials, applinks) and `assetlinks.json` hosting: AUTH-06 native passkeys, MOB-05 invite links.
3. Server-side read state (optional): cross-device unread sync, SYS-04.
4. VoIP/incoming-call pushes are **not** planned (voice channels are join-based, not ring-based).

## Counts

| Prefix | Area | Items |
|---|---|---|
| AUTH | Auth and onboarding | 15 |
| RAIL | Server rail | 6 |
| SIDE | Sidebar, channels, sections | 17 |
| TRANS | Transcript | 11 |
| MD | Message rendering | 9 |
| ATT | Attachments, embeds, lightbox | 13 |
| MSG | Message actions, reactions, emoji | 12 |
| COMP | Composer | 8 |
| DM | DMs and Home | 6 |
| MEM | Members panel | 4 |
| PROF | Profiles | 4 |
| DLG | Server dialogs | 9 |
| SSET | Server settings | 8 |
| SET | User settings | 7 |
| SEC | Security | 8 |
| ACCT | Account lifecycle | 5 |
| VOICE | Voice and video | 15 |
| MOD | Moderation | 5 |
| ADMIN | Admin dashboard | 8 |
| SYS | App shell and system states | 11 |
| DS | Design system | 6 |
| MOB | Mobile-only necessities | 16 |
| | **Total** | **203** |

Regenerate with `grep -o '^| [A-Z]*-' mobile/PARITY.md | sort | uniq -c`.
