# thehideout design language

`thehideout-design` is the shared design language for the desktop, web and mobile clients: tokens,
a mapping onto `rune_kit::Kit`, and the basic controls. It is platform-neutral (no fs, no windowing).

**Direction.** Cozy and warm, but minimal and crisp, and still tool-like. OpenCode's radii, but warm
greys instead of cool ones and one sage-teal accent. Dark is a dim lamp-lit room (warm near-black, never
`#000`). Light is paper (warm off-white, never `#fff`). **Borders separate structure, not content: spacing and fills first, dividers only at scroll edges.**
Shadows are only for things that float.

Review it: `client/gallery/render.sh` renders `client/gallery/snapshots/{foundations,controls,icons,themes,chat,chat-dialog}-{dark,light}.png`, or run `cargo run -p thehideout-gallery` for a window with theme, tint and contrast toggles and a Themes page (every accent on every background; press a tile to apply it everywhere).

```rust
let k = thehideout_design::kit(dark);                    // &'static rune_kit::Kit, the default look
let t = thehideout_design::theme(dark);                  // semantic colour tokens (Tone = OKLCH)
Button::new("Send").variant(Variant::Primary).build(k);  // components take the kit

// A user's look: build the kit from a ThemeSpec instead (see Themes below).
let k = thehideout_design::kit_for(&spec, spec.is_dark(system_dark));
let t = thehideout_design::theme_of(k);                  // the theme that kit was built from
```

Apps pass `thehideout_design::{FONTS, FAMILIES}` to `AppSpec` (Instrument Sans + JetBrains Mono, embedded
from `design/fonts/`; see Type).

## Colour (`tokens/color.rs`, `tokens/theme.rs`)

Everything is authored in OKLCH (`Tone { l, c, h, a }`). Components read the semantic names below,
never the ramp.

**Neutral ramp** `N0` (lightest) to `N14`: L = .985 .965 .94 .90 .84 .74 .64 .54 .49 .38 .31 .26 .22 .185 .155,
hue 75 to 55, chroma .006 to .0105.

| Token | Dark | Light | Use |
|---|---|---|---|
| `bg` | `#16120e` | `#f6f3ef` | window |
| `surface` | `#1e1a16` | `#fdfaf6` | panels, cards |
| `surface_raised` | `#28231f` | `#fdfaf6` | inputs, raised controls |
| `surface_overlay` | `#342f2c` | `#fdfaf6` | menus, popovers, tooltips, dialogs |
| `border_subtle` | `#342f2c` | `#e1ddd9` | scroll-edge lines, the member panel's left edge, the inset panel and auth card outline, floating edges (dialog, toast, tooltip, hover actions), keycaps |
| `border` | `#46413e` | `#cecac5` | the quote bar, the spoiler block, the gallery's theme previews |
| `border_strong` | `#7b7672` | `#908b87` | edges that must read on their own: an unchecked checkbox, a hollow role dot; a control's hover (rune-kit) |
| `control_border` | `#5e5854` | `#aeaaa6` | a control's edge at rest: inputs, the composer, Secondary buttons, the icon field and invite code, the rail's "+" tile, the "no colour" swatch |
| `text` | `#eeeae6` | `#16120e` | primary text |
| `text_muted` | `#aeaaa6` | `#645f5b` | secondary text |
| `text_faint` | `#908b87` | `#726e6a` | placeholders, hints (still 4.5:1) |

**Accent** (`tokens/accent.rs`; Sage-teal is the default, the other seven are in Themes below). The default's tokens:

| Token | Dark | Light | Use |
|---|---|---|---|
| `accent` | `#68bea8` | `#62b7a1` | fills: primary button, switch/checkbox on, mention pill |
| `accent_fg` | `#051a15` | `#051a15` | ink on `accent` |
| `accent_text` | `#80d0bb` | `#187363` | accent as text/icon on `bg`; also `focus` |
| `accent_subtle` | `#13322a` | `#d1f0e6` | quiet wash behind accent text; the selected state of Home and toggles |

**Status** (each has `x`, `x_bg`, `x_border`; the hues below are Sage-teal's, each accent preset moves the ones that would collide with it): `success` (leaf green h138, livelier than the accent), `warning` (gold h90),
`danger` (red h25, plus `danger_solid` / `on_danger` for destructive buttons), `info` (slate blue h255). Resulting status text: success `#8dcb78` / `#337431`, warning `#e5c057` / `#845a0f`, info `#8ab4ea` / `#446396` (dark / light). The eight avatar and server-badge fills (L .48, same in both appearances): `#8f4244` `#136d5b` `#2c628d` `#456930` `#8b4356` `#1c6878` `#695e1d` `#7a4d67`.
**Overlays**: `hover`, `pressed` (text colour at 5-6% / 9-10%), `accent_hover`, `accent_pressed`
(black 10% / 18%), `scrim`.

Rules: red is for errors only (a mention is accent, not red). Never hard-code a colour in a screen; add a token.
Contrast is enforced by tests (`cargo test -p thehideout-design`): `text`, `text_muted`, `text_faint`,
`accent_text` and status text are at least 4.5:1 on `bg`/`surface`/`surface_raised`; ink on fills at least 4.5:1;
`border` at least 1.3:1, `border_strong` at least 3:1 (non-text) on `bg`, `surface` and `surface_raised` in both themes, `control_border` at least 1.8:1 on those and on `surface_overlay` (High: 4.5:1, 3:1 on the overlay) and between `border` and `border_strong`, `focus` at least 3:1; every token stays in sRGB gamut. The tests run for **every** look (see Themes), including avatar ink, and keep the status hues at least 25 degrees from the accent.

## Themes (`tokens/spec.rs`, `accent.rs`, `background.rs`)

A user's look is one `ThemeSpec`, and nothing else is configurable:

```rust
ThemeSpec {
    accent: AccentPreset,         // 8 presets
    background: BackgroundPreset, // 5 presets
    appearance: Appearance,       // System | Light | Dark, resolved to `dark: bool` with the system's preference
    tint: bool,                   // surfaces and borders pick up a faint accent hue
    contrast: Contrast,           // Standard | High
    density: Density,             // Comfortable | Compact: layout, not colour (see Density and text size)
    text_size: TextSize,          // 90 | 100 | 110 | 125 %: the type scale, not colour
}
```

`theme_for(&spec, dark)` and `kit_for(&spec, dark)` return a `&'static Theme` / `&'static Kit`, built once per distinct look and cached (the theme: at most `ThemeSpec::LOOKS` = 8 x 5 x 2 x 2 x 2 = 320, so the cache is bounded; `appearance` only decides `dark`. The kit: one per look and `Metrics`, at most `ThemeSpec::KITS` = 320 x 8 = 2,560. `density` and `text_size` never change a colour, so every `Metrics` of one look shares one `Theme`). `theme(dark)` and `kit(dark)` are the default spec's. **Components keep the same call shape**: they take `&Kit` and call `theme_of(k)`, which returns the theme that kit was built from (a lookup kept in `kit.rs`, keyed by an identity stored *inside* the kit, so a `Kit::clone()` moved into a `'static` closure, as the virtualized lists do, still finds its theme and metrics), so one kit carries the whole look through the tree and nothing is set globally (there is no thread-local palette any more). `metrics_of(k)` answers the same way for density and text size. An app builds the kit once per frame from its spec; `ThemePreview` builds a miniature of any spec for settings tiles and the gallery. `ThemeSpec` is serde-serialisable (kebab-case strings, missing fields default) for apps that save it.

**Presets only.** There is no colour picker, and there will not be: every accent x background x appearance x tint x contrast combination is checked by the contrast tests, and a free colour could not be. Add a preset, and the tests cover it for all the rest.

**Accent presets.** Each defines the fill, its ink, text, subtle wash and (through `accent_text`) the focus colour for dark and light, the status hues that move off it, and eight avatar / server-badge fills. Graphite shares Sage-teal's badges (it has no hue to avoid). Fills (dark / light), Sage-teal first (the default):

| Accent | `accent` dark | `accent` light | `accent_text` dark / light | Status and moved hues (dark) |
|---|---|---|---|---|
| Sage-teal | `#68bea8` | `#62b7a1` | `#80d0bb` / `#187363` | success `#8dcb78` / `#337431` danger `#fb817a` info `#8ab4ea` |
| Moss | `#94ba60` | `#8eb35a` | `#a8cc7a` / `#4e6f24` | success `#70ccb0` / `#0b7462` danger `#fb817a` info `#80b7e7` |
| Petrol | `#52a2b9` | `#4c9bb3` | `#72c1da` / `#07657d` | success `#80cd82` / `#21763c` danger `#fb817a` info `#98afeb` |
| Ocean | `#64aaf0` | `#5da3e9` | `#8cc3fc` / `#215da5` | success `#76cf8a` / `#0b7643` danger `#fb817a` info `#64c2cc` |
| Brass | `#d4b54f` | `#ceae48` | `#e0c262` / `#796006` | success `#76cf8a` / `#0b7643` danger `#fb817a` info `#76bbe2` |
| Ember | `#f28f36` | `#e98a35` | `#f8a052` / `#a84811` | success `#93ca73` / `#3a732c` danger `#fb817a` info `#76bbe2` |
| Coral | `#f08695` | `#ec7e8e` | `#fe9eaa` / `#b2354a` | success `#80cd82` / `#21763c` danger `#f98662` info `#80b7e7` |
| Graphite | `#c7cbd0` | `#2b2e33` | `#c7cbd0` / `#35383d` | success `#80cd82` / `#21763c` danger `#fb817a` info `#85b6e9` |

Ocean is a clear mid blue (h250) with info moved to cyan; Coral is a warm pink-red (h12) with danger moved to a brick-orange red (h38) so errors never read as the accent; Graphite is grey (chroma under .012): dark gets a light-grey fill with dark ink, light a charcoal fill with light ink, and `accent_hover` lightens instead of darkening to match. No preset is purple.

**Background presets.** Each is a 15-step neutral ramp (`Ramp`: lightness per step, chroma and hue drifting down it) for dark and for light; surfaces, borders and text are picked from it by step, exactly as the warm table above. Warm is the default and the original palette.

| Background | Dark | Light |
|---|---|---|
| Warm | bg `#16120e` surface `#1e1a16` raised `#28231f` border `#46413e` | bg `#f6f3ef` surface `#fdfaf6` border `#cecac5` |
| Neutral | bg `#131313` surface `#1b1b1b` raised `#242424` border `#424242` | bg `#f3f3f3` surface `#fafafa` border `#cacaca` |
| Cool | bg `#0f1318` surface `#171b20` raised `#20242a` border `#3e4348` | bg `#eff4f9` surface `#f6fbff` border `#c6cbd1` |
| Deep | bg `#0d0b09` surface `#171413` raised `#201e1c` border `#413f3d` | bg `#f6f3ef` surface `#fdfaf6` border `#cecac5` |
| Paper | bg `#16120e` surface `#1e1a16` raised `#28231f` border `#46413e` | bg `#f4f0e6` surface `#fdf9ef` border `#ccc7bd` |

Neutral is true grey (chroma 0). Cool is slate, hue 250 to 255, chroma .008 to .013. Deep sinks dark to near `#0b0b0b` with a warm hint (light is Warm's). Paper is cream, hue 88 to 70 (dark is Warm's).

**Tint.** With `tint` on, `bg`, the surfaces, the borders and the inset layers move toward the accent hue by .006 of OKLab chroma (borders a little less), lightness untouched. It scales with the accent's own chroma, so Graphite stays grey. A test caps the shift.

**Contrast.** `Contrast::High` raises text and edges against `bg`/`surface`/`surface_raised`: `text` 10:1, `text_muted` 8:1, `text_faint` 7:1, `accent_text` and status text 7:1, `border_subtle` 1.8:1, `border` 3:1, `border_strong` 4.5:1 and `control_border` = `border_strong` (Standard: 7, 4.5, 4.5, 4.5, 4.5, 1.1, 1.3, 3). Tokens are authored for the default look; a `fit` step moves only a token whose contrast falls short on the chosen background, tint or level, by the least lightness that clears the bar (and eases chroma back into gamut), so the presets read as designed wherever they already pass and every combination passes by construction.

**Tests** (`tokens/tests.rs`) build all 320 looks once (8 accents x 5 backgrounds x dark/light x tint on/off x Standard/High; `every_combination_is_covered` asserts the count and that `look_index` is a bijection) and check every rule above on each, the preset characters (Neutral is grey, Cool is blue-grey, Deep is under L .16, nothing is purple, Ocean is blue, Coral is pink-red, Graphite is grey), the tint cap, the serde round trip and the caches. They take about 30 ms.

## Type (`tokens/type_.rs`)

The table is the 100% scale. Components never use the constants directly: they call `type_::at(k, STYLE)`, which scales by the kit's text size (see Density and text size).

Instrument Sans for UI, JetBrains Mono for code (ligatures off). Size / line height, weight.

| Style | Size / LH | Weight | Use |
|---|---|---|---|
| `CAPTION` | 11 / 16 | 450 | timestamps, counts, hints |
| `LABEL` | 12 / 16 | 500 | section labels, chips |
| `CONTROL` / `CONTROL_STRONG` | 13 / 20 | 440 / 530 | text inside controls |
| `BODY` | 14 / 21 | 400 | messages, reading |
| `BODY_STRONG` | 14 / 21 | 600 | names, emphasis |
| `TITLE` | 16 / 24 | 600 | panel and dialog titles |
| `HEADING` | 20 / 28 | 650 | page headings |
| `DISPLAY` | 28 / 36 | 700 | rare, large moments |
| `CODE` | 12.5 / 19 | 400 mono | code, identifiers |

**Fonts.** `Family::Sans` is **Instrument Sans** (variable, weight and width axes) and `Family::Mono` is
**JetBrains Mono** (Regular + Bold). Both are SIL Open Font License 1.1, with the licence texts beside the files:
`client/design/fonts/instrument-sans/` and `client/design/fonts/jetbrains-mono/`. They are embedded with
`include_bytes!` and exposed as `thehideout_design::{FONTS, FAMILIES}`. Code text never uses ligatures
(Rune turns `liga`, `clig`, `calt` and `dlig` off for every mono style, so `->` stays two characters).

## Layout (`components/chat/mod.rs`)

**Borders separate structure, not content.** Reach for spacing and a fill step first (`bg` against `surface`, a
`hover` wash on `surface`). Then, per role, the quietest stroke that still works:

| Role | Stroke |
|---|---|
| Control at rest (inputs, composer, Secondary button, icon field, invite code, rail "+", "no colour" swatch) | 1px `control_border` (1.8:1+); hover washes the fill, focus swaps the ring for `focus` (3:1+), invalid for `danger` |
| Control whose edge is the whole control (unchecked checkbox, hollow role dot) | `border_strong` (3:1) |
| Focus | `focus`, 3:1+ on `bg`; never weakened |

**Rings and edges never clip.** A ring drawn outside its control is 2pt wide and 1pt clear (`space::RING_WIDTH`, `RING_OFFSET`), so it reaches `space::RING` (3pt) past the box; Button, IconButton, GuildBadge/RailTile, Checkbox, the swatches and media buttons all use it, and a clipping parent (a scroll view, a dialog's scrolling body) leaves `RING` of room round its content: pad the view by `RING` and take it back with a negative margin so nothing moves (`Dialog` and the rail do). A full-width row (NavRow, MemberRow, section toggles, list rows) has no room outside, so its ring is inset (`RING_INSET`, drawn just inside the box). An input's ring at rest is an inset 1px shadow, like its focus and invalid outlines, so it never grows past the box and lines up with the buttons beside it. `El::border` takes no layout space and paints under children, so a panel that clips filled rows draws its outline with `components::util::frame_over` as its last child instead. `desktop/render.sh` ends with keyboard-focus renders (`focus-*.png`) of edge-hugging controls.
| Selected (your reaction, picker tab, swatch, accent preset, theme preview) | `accent` / `text` / `accent_text`; never quieted |
| Floating (dialog, toast, tooltip, chart tip, hover actions; rune-kit menus and popovers at half `border`) | the fill step and shadow carry it; 1px `border_subtle` edge (it melts into the overlay in dark, a hairline in light) |
| Inset panel, auth card | 1px `border_subtle` outline (light `surface` is barely off `bg`) |
| Region divider (member panel's left edge, scroll edges) | 1px `border_subtle`, exactly one between two regions |
| Labels (badges, connection pill), empty-state tile, title/topic | none: a wash and its ink, or 16pt and muted ink |
| Content markers (quote bar, date and "New" dividers, embed rail) | not borders: a `border` bar, a `border_subtle` line with a label, an accent line, the provider's colour |
| Drop zones | transparent until a drag hovers, then `accent` |

There are no rules under headers, above footers, between settings rows or around cards; settings are told apart by
28pt of vertical rhythm, not lines.

The shell insets the rail, sidebar and panel 8pt from the window's top and bottom, then lines up headers and footers on one y:
`header_h()` = 48 (Compact 40) for the sidebar title, the channel header and the member panel title (their titles share a baseline);
`footer_h()` = 64 (Compact 52) for the sidebar's user strip and the composer zone (a multi-line composer grows its zone upward; at rest
they match). The composer card floats in its zone with 12pt on every side. Headers and the footer carry **no divider at
rest**; the sidebar sits on `bg` and the panel's own 1pt `border_subtle` outline is the only edge between them. Widths: the
sidebar is 240 from 1100pt up and 208 below; the member panel is only offered from 1000pt. In the transcript a group's
rows sit 2pt apart and groups ~16pt apart (1pt and ~10pt Compact). Every size here is a `Metrics` method; see Density and text size.

### Scroll edges (`components/scroll_edges.rs`)

The only divider a panel has. `ScrollEdges::new()` holds two `Live<f32>` opacities; `edges.wrap(k, scroller)` (or
`scroll_edges(k, &edges, scroller)`) wraps a scroll viewport in a box that fills the space between a header and a footer
and pins a 1pt `border_subtle` line over its top and bottom edge:

- the **top** line shows only while content is scrolled underneath the header (offset > 0.5pt from the top);
- the **bottom** line shows only while there is more content below the viewport (not at the end).

The viewport's `on_scroll_offset` (called in the frame that draws the scroll) only sets the two live values to 0 or 1,
and the lines are `live_opacity` bindings smoothed on `motion-control`: a scroll runs **no view**, and the line fades
in and out rather than popping. Keep one `ScrollEdges` per viewport in app state (the desktop's `ui.edges` has
`transcript`, `sidebar`, `members` and `page`), so a rebuilt view binds the same values again; read `edges.top()` /
`edges.bottom()` in tests. The line sits where the header ends and the footer begins, so the sidebar's and the panel's
lines share one y. Used by the transcript, the sidebar list, the member list and both settings pages. A short list that
fits shows neither line. A `--at 1` snapshot is needed to see the lines at rest (they fade in on the first frames).

## Density and text size (`tokens/metrics.rs`)

Two choices in `ThemeSpec` (Appearance settings, saved with the look; a look saved before they existed loads as Comfortable / 100%):

- **`Density`**: `Comfortable` (the shipped sizes, default) or `Compact`.
- **`TextSize`**: 90, 100 (default), 110 or 125 %, serialised as `"90"`, `"100"`, `"110"`, `"125"`.

`Metrics { density, text }` (`Copy`) holds them, and **every derived size is a method on it**, so screens never branch on the density themselves. A component reads them from the kit it was given: `let m = metrics_of(k);` and `type_::at(k, type_::BODY)` for a scaled style. `ThemeSpec::metrics()` builds one from a spec, and `kit_for` builds `k.ty` (rune-kit's own type scale) from it, so rune-kit controls scale too. There is no global.

| Method | Comfortable | Compact | Notes |
|---|---|---|---|
| `font(style)` | size x f, line height x f (to the half point) | same | f = .90 / 1 / 1.10 / 1.25; weight and tracking unchanged |
| `icon(base)` | base x (1 + (f - 1) / 2), rounded | same | 16 becomes 18 at 125% and 15 at 90%; 14 becomes 16 and 13 |
| `em(base)` | base x f, rounded | same | lengths that sit with text: inline and jumbo emoji, badge / kbd / count heights, reaction chips |
| `row_h()` | 32 | 28 | sidebar, DM and member rows; never shorter than the control line plus 4 |
| `row_pitch()` | 34 | 30 | row plus the 2pt gap; what a drag steps over |
| `section_h()` | 28 | 24 | collapsible section header |
| `control_h()` | 32 | 28 | Md `Button` / `IconButton`, `Segmented` |
| `field_h()` | 32 | 28 | the composer's input line; never shorter than the body line plus 6 |
| `header_h()` | 48 | 40 | sidebar title, channel header, member title, settings headers |
| `zone_pad()` | 12 | 8 | the composer card's margin in its zone |
| `footer_h()` | 64 | 52 | `field_h + 8 + 2 x zone_pad`: the composer zone and the sidebar's user strip |
| `group_gap()` | 12 | 7 | above a message group |
| `msg_pad(head)` | 3 / 1 and 1 / 1 | 2 / .5 and .5 / .5 | message row padding (top, bottom): about 16 between groups and 2 between rows in a group, against about 10 and 1 |
| `msg_avatar()` | 32 | 24 | the transcript avatar and the gutter beside follow-ups (32 again at 125%) |
| `rail_tile()` | 40 | 32 | server rail tiles; the rail is the tile plus 24 wide |
| `table_row(base)`, `pad(base)` | base | base - 8, base x .75 | admin tables, settings page rhythm |

`header_h` and `footer_h` are one function each, so the sidebar, channel header, member panel and composer zone cannot drift apart; the desktop tests check the alignment, the zone margins and the transcript's edges for all eight combinations. Heights are also floors under the scaled text (a row is at least its line plus 4), so a larger text size lengthens a row before it ever clips; where a line still doesn't fit (the topic, a long channel name) it truncates with an ellipsis.

**Intentionally fixed** (they are grids or pictures, not reading text; they keep their own geometry at every setting):

- the emoji picker (32pt cells, 304pt body) and the icon picker (36pt cells), glyphs included;
- attachment cards, image grids, the video card and the composer's tray chips;
- guild badges and avatars (their sizes are chosen by the caller: 20 / 24 / 32 / 40; only the transcript's and the rail's choice follows density);
- `rune_kit::TextInput` (28pt, `.large()` 32pt): its height is Rune's, its text still scales;
- the role list of server settings (36pt rows, 42pt pitch) and the admin tables' column widths.

**Animation.** The Appearance controls ask Rune for the layout to reflow on `motion-reflow` and the colours to cross on the calm spring; reduced motion snaps the layout.

**Tests**: `tokens/tests.rs` `metrics_tests` (defaults, compact, scaling, every combination, kits and clones); the desktop's `sizing` tests (alignment x 8, row height on a switch, the header title inside its header at 1280x800 and 1000x700) and the `appearance` persistence tests.

## Spacing, radius, elevation, motion

- **Space** (`space.rs`): `S0_5`=2 (hairline only), `S1`=4, `S2`=8, `S3`=12, `S4`=16, `S5`=20, `S6`=24, `S8`=32, `S10`=40, `S12`=48, `S16`=64.
- **Radius** (`radius.rs`), OpenCode's scale exactly: `XS`=2, `SM`=4, `MD`=6, `LG`=8, `XL`=10. `FULL` is for dots and count pills only. Nothing rounder than `XL` otherwise.
- **Elevation** (`elevation.rs`): `Flat` (no shadow, a border), `Popover` (menus, tooltips), `Modal` (dialogs). Two warm-umber shadow levels, subtle in light and a little stronger in dark.
- **Motion** (`motion.rs`), Rune's springs (duration, bounce): `PRESS` .10/0, `CONTROL` .20/0, `OVERLAY` .24/0, `DISCLOSURE` .30/0, `REFLOW` .40/0, `POP` .25/.15 (sparingly).

## Components (`components/`)

Every control takes `&Kit`, and shows `State::{Rest, Hover, Pressed, Focus, Disabled}` through `.state(..)` (forced, for the gallery and tests; at rest the runtime drives hover/press).

| Control | Source | Notes |
|---|---|---|
| `Button` | own | `Variant::{Primary, Secondary, Ghost, Danger}`, `Size::{Sm 24 / radius-sm, Md 32 / radius-md}`, optional `.icon(..)`. One Primary per view. |
| `IconButton` | own | Same variants/sizes, square (24 / 32), a label is required. Ghost by default; `.selected(true)` on a Ghost toggle is the quiet `accent_subtle` fill with an `accent_text` icon (never a bordered box). `.ink(color)` sets the glyph's ink (a muted microphone in `danger`) and `.hover_fill(color)` the hover wash (a destructive button's `danger_bg`); a `selected` toggle keeps its own ink. |
| `Segmented` | own | One-of-N switch: one container filled with `bg` (no border), 32pt, the selected segment a filled `surface_raised` tile. |
| `ScrollEdges`, `scroll_edges` | own | The scroll-edge hairlines (see Scroll edges under Layout). |
| `TextInput` | `rune_kit` | 28pt (`.large()` = 32pt, the Md control height, used in dialogs and the auth card), `radius-md`, 1px `control_border` ring at rest, a `hover` wash on hover; focus swaps the ring for `focus`; invalid uses `danger`. |
| `switch(on)` | `rune_kit::Switch` `.web()` | 24x16, 4pt corners, accent when on. |
| `Checkbox` | own | 16x16, `radius-sm`, off: `border_strong`; on / mixed: accent fill with `accent_fg` mark. |
| `Avatar` | own | Sizes 20/24/32/40, rounded squares (radius sm/sm/md/lg), initials on one of eight warm fills chosen by FNV-1a of the user id (all take the same ink at 4.5:1+), `Presence::{None, Online, Idle, Offline}` dot (offline is hollow). `.image(Some(pixels))` draws the user's picture (`rune::Pixels`, cropped to fill) over the initials, which stay visible while it decodes and if it never does; `GuildBadge` and `MemberRow` take it the same way. |
| `Badge` | own | 18pt, `radius-xs`, `Neutral`, `Accent`, `Success`, `Warning`, `Danger`, `Info`: a wash and its ink, no edge. |
| `UnreadDot` / `MentionPill` | own | Unread: quiet 6pt dot in `text`. Mention: accent pill, tabular count, `99+` cap. |
| `Kbd` | own | 18pt keycaps, `radius-xs`, raised fill, 1px `border_subtle`. |
| `Slider` | own | A 0..1 control: 4pt track (`border_strong`) with an accent fill, a 16pt accent thumb ringed in `surface`, in a 28pt box (`radius-md`, focus ring) whose track is inset by the thumb's radius, so the box lines up with the text column. It only draws: the page attaches `on_drag` (`slider_fraction(d.x, d.w)`) and `on_step` / `on_step_limit` for keys. Role `slider` with a 0..100 range. |
| `LevelMeter` | own | A live level bar: 8pt `border_strong` track, accent fill, optional threshold marker (a 2pt `text` line 3pt taller than the track on each side). Role `meter`. Smoothing (fast rise, slower fall) belongs to the caller. |
| `tooltip(..)` | `rune_kit::Tooltip` | Standard appearance: overlay surface, 1px `border_subtle`, `radius-sm`, popover shadow, optional `Kbd`. |
| `CodeInput` | own | A one-time code, one character per cell (6 by default): 40x48 cells (`em`-scaled), `radius-md`, `surface_raised` with the `control_border` edge; the next cell takes the 2px focus edge, invalid makes every edge `danger`. One field lies over the cells and takes the keys, so typing advances, Backspace steps back and a paste fills them all; the caller filters in `on_change` (`digits_only`) and may submit once full. |
| `QrCode` | own (`qrcode` crate, no image features) | A QR code drawn as run-length strips of elements (crisp at any scale), EC level M, 3-module quiet zone, `radius-md`. Always the light look's `text` on its `surface`, in dark too: inverted codes fail in many authenticator apps. |
| chat components | `components/chat/` | See "Chat components" below. |

## Chat components (`components/chat/`)

Data in, callbacks out: no app state types, every one takes the `Kit` like the controls above. The gallery's
**Chat** page shows each in its forced states, then an interactive assembled preview
(`chat-{dark,light}`, and `chat-dialog-{dark,light}` with the live dialog open).

| Component | For | States |
|---|---|---|
| `GuildBadge`, `RailTile` | The server icon, sizes 24/32/40, avatar radii, deterministic warm fill; `RailTile` is the same tile with an icon on a neutral face (Home, and the outlined "+"). A left rail holds the indicator. | rest, hover (short bar), selected (tall accent bar; `RailTile` adds an `accent_subtle` face), unread (4pt dot), mention count on the corner (overhangs the face 2pt; the rail gap is 12) |
| `NavRow`, `ChannelRow` | Sidebar rows, 32pt (`row_h()`; 28 Compact), 2pt apart, 8pt padding, 16pt icon, `radius-md`. Nav rows take an icon and a label (`NavRow::new(label, &icons::HOUSE)`); `NavRow::person` shows the user's 20pt `Avatar` instead; channels show `#` or their own Phosphor icon (`.icon`), tinted by a palette colour (`.tint`; the selected highlight takes a 12% wash of it), and a `.trailing` "more" button that fades in on hover. A voice channel gets `ChannelRow::connected(true)` while we are in it: `accent_text` label and icon (a tint still wins) and an a11y value of "connected", never the selection highlight (that is the open text channel's). | selected (highlight, strong text, `accent_text` icon; a channel's own tint wins), unread (brighter, heavier, dot), mentions (pill), muted (faint), hover |
| `VoiceParticipantRow` | One person in a voice channel, under the channel's row (`chat/voice_row.rs`): a `row_h()` row indented by `voice_row::LEAD` (8 padding + the `GLYPH_W` column + 8 gap) so the 20pt `Avatar` starts where the channel's name does, the name in the member's role colour (`.name_color`, `palette_text`; `text_muted` without one, `text_faint` when muted for you), and trailing 14pt state glyphs: `MICROPHONE_SLASH` in `danger` for a server mute (it wins), in `text_faint` for a self mute, and `SPEAKER_SLASH` in `text_faint` for a deafen; `VIDEO_CAMERA` in `text_faint` while their camera is on, and a small `Badge` (Danger) "LIVE" while they share a screen (`.on_watch` makes a press on the pill the pointer's way to watch it; the keyboard's is the row's menu). Speaking is a 2pt `accent` ring outside the avatar (`RING_WIDTH` / `RING_OFFSET`, so nothing clips). The row is a button; its state is its a11y description ("speaking", "muted", "muted by a moderator", "deafened", "muted for you"). | rest, hover, speaking, self-muted, deafened, server-muted, muted for you |
| `VoiceTile` | One participant in the voice stage (`chat/voice_tile.rs`): a rounded (`radius::LG`) button that fills the box its parent gives it. `TileMedia::Video(pixels)` letterboxes live video (`pixels_fit(.., SurfaceFit::Fit)`) on `MEDIA_BACK` over a quiet "Connecting…" line; `TileMedia::Avatar` centres the person's avatar (`TileSize` 96 / 64 / 40pt) on the inset `layers[0]` wash. The name sits bottom-left on a chip (the media dim over video, `surface` on an avatar tile) with a monitor glyph for a screen and a microphone-slash for a mute (danger for a server mute). Speaking is a 2pt `accent` frame drawn inside the tile (`frame_over`: tiles touch their neighbours, so a ring outside would clip); keyboard focus is the inset ring. Its a11y name is `label` ("bruno's camera", "ana's screen"), its description "speaking" / "muted". | video, avatar, speaking, muted, server-muted, screen, large / medium / small |
| `SidebarSection` | A collapsible group header (28pt; 24 Compact) with a chevron, a hover "+" and an optional hover "more" button (`.more`); `.decorate_header` lets the app add a context menu or a drag to the title. | open, closed, add shown |
| `MessageGroup`, `ChatMessage` | Avatar (`.avatars(..)` supplies pictures by author id), author (`.name_colors(..)`: the colour of their role, as `palette_text`), time, one or more bodies (`.mention_ink(..)` colours `@name` chips of coloured members). `MessageGroup::runs` splits a transcript: same author, at most 5 minutes (`GROUP_WINDOW_SECS`) apart. Follow-ups show their time in the gutter on hover. Bodies are markup (see "Message markup"). `links::<S>()` makes links and spoilers pressable. | `Sent`, `Pending` (body at 0.55, no spinner), `Failed` ("Failed to send · Retry"), mentions you (`accent_wash` + 2pt accent bar), hover (row highlight + floating reply/copy/more) |
| `DateDivider`, `NewMessagesDivider` | A hairline with the date; an accent line with a "New" tag. | static |
| `Composer` | The message box: "+", a field that grows to 8 lines then scrolls, smiley, Send. `bind` wires the field and send; Enter sends, Shift+Enter breaks the line; whitespace never sends. | empty (Send ghost, disabled), text (Send primary), focused (focus ring), disabled with a reason |
| `ChannelHeader` | `#` + name + topic + trailing IconButtons; `breadcrumb` gives "Guild / # channel". No rule under it and no tick before the topic: 16pt and muted ink. | static |
| `MemberRow` | Avatar (with a picture if given) and presence, the name (in a role colour if given), crown for the owner. | online, idle, offline (faint), hover |
| `EmptyState` | Icon tile, title, body, optional action `Button`. | static |
| `EmojiPicker`, `EmojiSuggest`, `EmojiSet` | See "Emojis" below. | rest, hover, highlighted, searching |
| `PickerTabs`, `IconGrid`, `ColorSwatches` | The channel look pickers: tabs (a tab given `false` shows "Soon"; the app enables Emoji and Server emoji, which hold an `EmojiPicker`), the Phosphor catalog as a virtualized 10-column grid (six rows high, or `.visible_rows(n)`; only rows near the viewport are built; a `highlight` for keyboard choice, the search field keeps focus), and the 12 palette swatches plus "none" (selected: ring and check; they wrap in a narrow column). | rest, hover, selected, highlighted |
| `VideoCard`, `AudioRow`, `FileCard`, `image_grid_el`, `tray`, `lightbox` | Attachments (`chat/attachments.rs`), data in and `Wire`s out (a `Wire` attaches the app's handler to an element, so the components know no state). Pictures: one keeps its aspect inside 400x300, two sit side by side, three put the first tall at the left, four or more are a 2x2 (the fourth says "+N"); a press opens the lightbox. `VideoCard` is the poster with a play button until the app starts a player, then the frames with a bar (play/pause, scrub, time, mute, expand); with the platform unable to play it, or no file kept, it is the poster with "Open in browser" / "Open original". Cards (`FileCard`, `AudioRow`, the removed placeholder) are filled with `hover`, not outlined. The composer's `tray` puts a chip per file above the input (thumbnail or type icon, name, size, a progress bar while it goes up; a failed one is `danger_bg` with Try again). `lightbox` pushes a scrim and the picture, with arrows, download and close. | rest, uploading, ready, failed; playing, paused, failed |
| `EmbedCard` | A link preview (`chat/embed.rs`): a left rail in the provider's palette colour (`border_strong` for a link), a header (site icon, site, chevron) that folds the card, and the body: title, description (3 lines) and a thumbnail, or for X, TikTok, Instagram and Reddit the author, the post text and the media the app built. | open, folded |
| `Dialog` | Modal (`.body_max_h(h)` makes the body scroll past `h`, so a tall form fits a short window): scrim, panel (24 padding; title to body 16, label to field 6, field to field 16, body to footer 24), title, body, right-aligned footer. Rune's modal stack gives the focus trap, Esc, scrim click and focus restore. `preview()` draws the panel inline. | closed, open |
| `Toast` | Bottom-centre notice, info / success / danger. `Toast::host` is its inert overlay layer; the caller dismisses. | kinds |
| `ConnectionPill` | Connected, Connecting…, Reconnecting in Ns, Offline. | status |
| `dialog_field` | A labelled field that spans the dialog body, with an inline error under it. | valid, error |

The one new token for these is `accent_wash` (accent at 7% dark / 8% light) behind a message that mentions you;
a test keeps `text`, `text_muted` and `accent_text` at 4.5:1 on it over `bg` and `surface`. Timestamps inside a washed
message therefore use `text_muted`, not `text_faint`.

### Message markup (`chat/markdown.rs`, drawn by `chat/body.rs`)

A Discord-flavoured CommonMark subset, parsed by our own small parser rather than `rune_kit::Markdown`: that is plain
CommonMark (no spoilers, mentions or emoji tokens, `__x__` means bold where Discord underlines), builds whole documents
and cannot be composed with the mention chips, so bending it costs more than a small linear parser with no extra
dependency. Everything is drawn as spans on one wrapping Rune text per paragraph. The composer sends the raw text
unchanged (no live preview). Selecting message text copies it as drawn (markers stripped; a hidden spoiler is left out).

| Write | Get |
|---|---|
| `**bold**`, `*italic*` or `_italic_`, `***both***`, `~~strike~~` | weight 600, italic, both, struck through |
| `__underline__` | underline (Discord's meaning; `___x___` is underline + italic) |
| `` `code` ``, ``` ``a ` b`` ``` | inline mono chip; nothing inside is parsed (no mentions, links or markup) |
| ```` ```rust ```` fence ```` ``` ````, or ` ```one line``` ` | mono block on a `bg` fill, no border, `radius-md`; syntax-coloured when Rune knows the language (`rune_kit::syntax`), plain otherwise; an unclosed fence runs to the end |
| `> quote`, `>>> quote to the end` | 3pt `border` bar, content indented; holds any block |
| `- item`, `* item`, `1. item`; two-space indent nests | bullets (`•`, then `◦`) or numbers (the first number sets the start) |
| `#`, `##`, `###` + space at line start | 16/15/14pt, semibold; `####` and a mid-line `#` stay text |
| `\|\|spoiler\|\|` | a block in `border` the text colour hides; click reveals it (state per message, local to the UI, not app state); a link inside stays inert until revealed |
| `[text](https://url)`, `https://url`, `<https://url>` | `accent_text`, underline on hover, hand cursor; click opens the default browser. A masked link shows its URL in a tooltip. Trailing punctuation, `*`, `_`, `~`, `\|` and an unbalanced `)` are not part of a bare URL |
| `@name` | the mention chip (`accent_subtle`); the row's `accent_wash` still comes from `mentions_me`. Not parsed in code; works inside bold, quotes, lists and spoilers, and keeps the emphasis around it (`**@tim**` is a bold chip, `*@tim*` an italic one) |
| `<:name:id>`, `<a:name:id>` | a custom emoji: a 20pt picture in the line when the app's `EmojiSet` knows the id, else `:name:` text; see "Emojis" |
| `\*`, `\@`, `\<`, any `\` + ASCII punctuation | the character itself |

Markers pair only when they flank text (`2 * 3 * 4` is text; `snake__case__word` is text) and an unmatched marker
stays literal. No HTML, no images, no tables, no setext headings.

**Safety.** Only `http://`, `https://` and `mailto:` become links, matched case-insensitively. Anything else
(`javascript:`, `file:`, `data:`, `//host`) is not a link and the whole `[label](url)` shows as typed, so a label can
never hide a scheme. Destinations and bare URLs are at most 2048 bytes with no whitespace or control characters; Rune's
`Tx::open_url` re-checks the same list before opening anything.

**Limits.** Parsing is linear: block nesting (quotes, lists) stops at 4 levels (deeper lines are plain text), inline
formatting at 16 open markers, at most 4096 marks per text, and only the first 64 KiB of a message is parsed (the rest
is one plain paragraph). Tests drive 60,000-character pathological inputs.

`body::parse` is the flat view the app uses to find mentions (`Block::Text` / `Block::Code`, mention and code marks only).

### Emojis (`chat/emoji.rs`, `chat/emoji_picker.rs`, drawn by `body.rs` and `message.rs`)

**Unicode.** The `emojis` crate (bundled, no network) gives about 1,900 default-tone emojis with names, shortcodes and
groups. Only Unicode 15.1 and older are offered: the crate already ships 16 to 18, which no colour emoji font draws yet
(they showed as boxes). Skin tones: the default only; a tone someone types still renders, being plain text. Emojis draw
as text, so the system's colour font does it: Apple Color Emoji on macOS (checked in the headless snapshots), Segoe UI
Emoji on Windows, Noto Color Emoji on Linux where installed (nothing is bundled; without one they fall back to
monochrome or boxes).

**Picker.** `EmojiPicker` is a 304pt body: a search field, a strip of section buttons (recent, this server, the nine
groups; they scroll to the section), and a virtualized grid of 32pt cells, nine across, eight rows high (the Unicode
set is 1,900 cells: only rows near the viewport are built). Sections have a title row; a search drops the titles and
lists matches of the server's emojis, then Unicode (an exact shortcode first, then prefixes, then anything containing
it, names included). `Scope` picks what is offered: `All` (composer, reactions), `Unicode` (a DM, the channel icon's
Emoji tab) or `Server`. Keyboard: the search field keeps focus; arrows move the highlighted cell (left/right by one,
up/down by a row, over titles), Enter picks, Escape closes. The caller owns `EmojiPickerState` and the recent list
(reaction keys, newest first, 24); a pick comes back as `Pick::Unicode` or `Pick::Custom`, whose `text()` is what goes
into the composer (the character, or `<:name:id>` / `<a:name:id>`). It is a body, not a popover: the app wraps it in
`rune_kit::popover` hung from the button.

**Animation.** In the picker an animated server emoji rests on its first frame and plays while the pointer is over its
cell, so thirty GIFs don't keep the window drawing. Under reduced motion it never plays. In message text Rune draws an
inline image from its first frame only (its inline-icon path has no animation clock), so a custom emoji inside a line
is still there; animated ones play where they are elements: jumbo messages, reaction chips and the picker. (Making
inline ones play needs Rune to pass its animation clock to inline icon images.)

**In messages.** `MessageGroup::emojis(EmojiSet)` resolves ids to pictures. A token the set knows draws 20pt in the line
(a faint square while the picture downloads, so nothing shifts when it lands); unknown ids (another server's, a deleted
emoji, anywhere in a DM) stay `:name:` text. A message of only emoji and spaces, at most 27 of them, draws jumbo: custom
at 48pt, Unicode at 40pt text (flags, keycaps, skin tones and ZWJ families count once). Any other text, or formatting
around them, makes it an ordinary message.

**Autocomplete.** `EmojiSuggest` is the list for `:sm` (two letters after a colon that starts the text or follows a
space): at most eight rows, the server's emojis ahead of Unicode ones on equal rank. The composer's `on_key` takes Up,
Down, Tab and Enter (accept) and Escape (hide until the word changes); rows keep the composer focused when pressed.

**Reactions.** Chips under the body: emoji, a rolling count (`RollingText`), 24pt high. Yours takes `accent_subtle`
and an `accent` border; the others have no border, just a quiet `hover` fill (as does "+"). A new chip pops in on `motion::POP`; counts roll on their own spring (reduced motion crossfades).
Press toggles yours; "+" (and the React button in the hover bar) asks the app for its picker, hung from the element
keyed `react-add-<id>` / `react-bar-<id>`. At 20 different emojis "+" is disabled. The hover text names the emoji and
your part (the server sends counts, not who reacted).

### Motion

Rules: an action that changes something plays one short, non-bouncing motion; the thing that moved is the thing that
changed. Springs come from `motion::*`; only the mention count's entrance uses `POP` (a mention is the one thing that asks for you; reserve it otherwise for reactions). Nothing animates on the
first frame, and a commit that no input caused (a poll, data arriving) lands at once: Rune's own rule, so a transcript
that loads or reconnects never plays an entrance. Reduced motion is Rune's: travel snaps and opacity/colour take a
0.15s fade.

| Action | Property | Token |
|---|---|---|
| New message appended | opacity 0 to 1 and 6pt rise (that row only) | `motion-control` |
| Pending to sent | body opacity 0.55 to 1 (nothing else) | `motion-control` |
| Selection moves between sidebar rows | the highlight travels (matched element: position, size) | `motion-reflow` |
| Row hover, press | overlay fill | Rune's hover/press tracks |
| Row text/icon changes (selected, unread, muted) | colour | `motion-control` |
| Section collapses or expands | chevron rotates a quarter | `motion-disclosure` |
| | rows fade, what is below moves to make room | `motion-disclosure` |
| Mention count appears (sidebar row, guild badge) | opacity and scale 0.6 to 1, with a small bounce | `motion-pop` |
| Hover a section header, a message, a guild badge | "+", action bar, gutter time, rail bar fade in | hover track |
| Guild badge state | rail bar height and colour | `motion-control` |
| Send becomes available / unavailable | fill colour (primary to ghost) | `motion-control` |
| Send pressed | scale to 0.96 and back, press response | `motion-press` (Rune's press spring) |
| Message sent | the field clears in the same commit, no animation | none |
| Dialog opens or closes | scrim opacity; panel opacity and scale 0.98 to 1 | `motion-overlay` |
| Embed folds or unfolds | chevron turns a quarter; the body fades while what is below moves to make room | `motion-disclosure` |
| Embed arrives for a message (`MessageUpdated`) | at the end of the transcript: eases in; while the reader is up in older messages: lands at once and the scroll anchor holds their row | `motion-control` |
| Lightbox opens or closes | scrim and picture opacity | `motion-overlay` |
| Upload progresses | the chip's bar width | `motion-control` |
| Toast shown or dismissed | opacity and 8pt rise; fade out | `motion-overlay` in, `motion-control` out |
| Connection status changes | pill fill, dot and label colour | `motion-control` |
| Content scrolls under a header, or more lies below | the scroll-edge line's opacity (live value, no view run) | `motion-control` |

Never animates: date and "New" dividers, channel header, empty states, member rows, avatars, the `Failed` line,
anything inside an existing message when it is edited, the order of a list, whole lists staggering in, scroll position
(the transcript sticks to the bottom). No spinners, flying bubbles or shake on failure: the state is the text.

Two Rune facts worth knowing when you compose these. A child that mounts inside a parent that is also mounting rides
the parent's entrance and plays none of its own, so put a transition on the outermost element that appears (the
`Dialog` puts its scale on the layer, not the panel; message groups carry `Transition::none()` and their rows carry
the entrance). And keep a message's `id` stable from pending to sent (a client nonce), or the row remounts and plays
the entrance again.

## Icons (`icons/`, `tokens/icon.rs`)

**Phosphor Icons** (`@phosphor-icons/core` 2.1.1, MIT, Copyright (c) 2023 Phosphor Icons; licence text in
`src/icons/LICENSES/phosphor-MIT.txt`). Generated Rust, no runtime dependency.

- **Weight.** Two sets, never mixed in one role. The app's own chrome (`icons::*`, `curated.rs`) is Phosphor
  **regular** (outline): chevrons are carets, `plus`, `x` and `check` are strokes, `hash` is the two-bar hash. The icons
  users choose (channel icons and the icon picker, `icons::catalog`) are **fill**. Channel icons are stored on the server
  as a plain Phosphor name (`"game-controller"`, no weight); the app renders the fill glyph for it. A glyph the app
  itself shows (nav, empty states, emoji categories, file types, device kinds) is a curated constant, never a
  `catalog::by_name` lookup; `by_name` is only for a name that came from a user.
- **Selection rule.** Selected or active is never a weight swap. Use the row highlight, stronger text and an
  `accent_text` icon (nav rows, rail Home, the members toggle, Ghost `IconButton::selected`). A channel with a palette
  colour keeps its tint and takes the 12% wash on the highlight.
- **Sizes** (`thehideout_design::icon`): `XS`=14 (inside 24pt controls), `SM`=16 (default), `MD`=20 (standalone, toolbars). Button and IconButton use `XS` at Sm and `SM` at Md. Icons take the ink of their
  label, or `icon_muted` in a Ghost IconButton.
- **Add an icon.** Append `("our-name", "phosphor-file-stem")` to the `ICONS` list at the top of `tools/gen_icons.py`
  and rerun it from `client/design` (the package is read as data, never executed):

  ```sh
  python3 -I tools/gen_icons.py --phosphor /path/to/@phosphor-icons/core --out src/icons
  ```

  It writes `src/icons/curated.rs` (the regular glyph for every entry, with an `ALL` list). To make a new icon available to
  channels you need nothing: the catalog already holds every Phosphor icon.
  Names are kebab-case in `Icon::name` and `SCREAMING_SNAKE` as the const (`chevron-up-down` is `CHEVRON_UP_DOWN`).
  The gallery's Icons page lists the curated set from `icons::ALL`. The same run also writes the full catalog
  (`src/icons/catalog/part_N.rs`, tags read from the package's `dist/index.mjs` as data); run `rustfmt --edition 2024`
  over `src/icons` afterwards.

### Channel palette and the full catalog

**Palette** (`tokens/palette.rs`): `palette_color(key, theme) -> Option<Tone>` maps the 12 wire keys
(`PALETTE_KEYS`: slate red orange amber yellow lime green teal cyan blue indigo pink; `""` = none) to icon ink.
One lightness per appearance (dark L .76, light L .53) and a chroma and hue per key, so the twelve read as a
family and contrast is a property of L: every tone has at least **3:1** (non-text) on `bg`, `surface`,
`surface_raised` and under the selected-row wash in **all 320 looks**, stays in gamut, and is distinct from
the others (`palette_tests`). Use it only for icon ink, dots and swatches, never for text.

**Text-safe variant** (`palette_text(key, theme)`): the same hue and chroma with the lightness moved just far
enough that the colour reads as *text*: 4.5:1 (7:1 on High contrast) on `bg`, `surface`, `surface_raised`
and `surface_overlay`, under a hover or pressed row, under the mention wash (`accent_wash`) and inside its own
chip wash (`palette_wash`, the icon tone at 16%). It is what a role-coloured name and an `@mention` of such a
member use. A test checks it on all 320 looks x 12 keys x 4 surfaces x 5 grounds.

**Full catalog** (`icons::catalog`): all 1512 Phosphor fill icons (the user-chosen set) with their search tags, generated into
`src/icons/catalog/part_{0..3}.rs` (sorted by name; same names and tags as Phosphor's regular set).
`by_name(&str) -> Option<&'static Icon>` is what a channel's stored icon name resolves through (unknown name:
fall back to `#`); `entry`, `all()` and `search(query)` (every word must match the name or a tag; exact name,
name prefix, name substring, then tag-only) serve the picker. The curated `icons::*` constants stay the app's own
chrome, in regular weight.

## Kit mapping (`kit.rs`)

`kit_for(&spec, dark)` (`kit(dark)` for the default) starts from `Kit::from_palette_focus`, then sets every colour, type, radius and motion field
from the tokens, flattens button/switch/knob elevation, gives inputs the strong ring, and sets the floating
shadows to `Popover`/`Modal`. Any rune-kit component (Select, Menu, Dialog, ...) added later inherits the look.

## Known limits

- Kit `Kit::blend_over` is set for Rune's default sRGB blending; a window that opts into linear-light blending would need the kit built through `rune::theme::with_blend`.
- `light.surface == surface_raised == surface_overlay` on purpose: in paper mode, rank comes from border and shadow, not tone.
