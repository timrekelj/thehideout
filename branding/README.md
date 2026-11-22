# Branding

`TheHideout.png` is the master app icon: a 640×640 PNG that is a **64×64 pixel-art grid drawn at 10×**
(a night camp: tree, mountains, campfire, tent, stars). Don't edit it in place with a tool that
resamples, and don't commit a re-exported copy.

## Scaling rule

It is pixel art, so it must **always be scaled with nearest-neighbour, never smoothed**, and by
integer multiples of 64 px whenever possible (128, 192, 256, 512, 1024, ...). Never use bilinear,
bicubic or "smooth" scaling to enlarge it, and don't let a platform tool re-scale it behind your back
(`sips -z` smooths). The generator reduces it with an area filter only where a size cannot be an
integer multiple (16, 32, 48, 72, 96, 144, 180 px), which is a reduction and keeps the pixels crisp.

## Regenerate every icon

```sh
branding/make-icons.sh
```

Needs only macOS tools (`swift`, `iconutil`). It first recovers the 64×64 grid by sampling pixel
centres and fails unless the grid enlarged 10× matches the master exactly. It writes
`client/packaging/macos/AppIcon.icns` (committed) and `branding/out/` (committed, the mobile
projects take their icons from here).

| Output | What |
| --- | --- |
| `client/packaging/macos/AppIcon.icns` | Full iconset (16 to 1024, @1x/@2x). Art 832 px (13×64) in the 1024 canvas, masked to the rounded-rectangle icon shape (radius 185 at 824 px) with a soft drop shadow; smaller sizes are reduced from the 1024 render. |
| `out/macos/` | The 1024 and 32 px PNGs for inspection. |
| `out/ios/AppIcon-1024.png` | Full-bleed 1024×1024, opaque, square corners (iOS masks it), 16×. |
| `out/ios/AppIcon-1024-dark.png` | Same art for iOS 18 dark icons: it is already a night scene. |
| `out/ios/AppIcon-1024-tinted.png` | Greyscale (luminance) version for iOS 18 tinted icons. |
| `out/android/ic_launcher_foreground.png` | Adaptive icon foreground, 432×432 (xxxhdpi, 108 dp). Art at 5× (320 px) centred, so it covers the whole 72 dp visible area; the centre of the scene is inside the 66 dp safe circle. Circular masks crop the corners (the tent's right edge). |
| `out/android/ic_launcher_background.png`, `values/ic_launcher_background.xml` | Solid `#030C11`, the darkest sky pixel. |
| `out/android/ic_launcher_monochrome.png` | Themed-icon layer: tree, tent and campfire as a white silhouette. The classification is by palette and position in `make-icons.swift`; update it if the artwork changes. |
| `out/android/mipmap-anydpi-v26/ic_launcher{,_round}.xml` | Adaptive icon definitions (foreground drawable names `ic_launcher_foreground` and `ic_launcher_monochrome`). |
| `out/android/mipmap-{m,h,xh,xxh,xxxh}dpi/ic_launcher{,_round}.png` | Legacy icons, 48/72/96/144/192 px (rounded square and circle). |
| `out/android/play_store_512.png` | Play Store 512×512. |
| `out/web/` | `favicon.ico` (16+32), `favicon-16.png`, `favicon-32.png`, `apple-touch-icon.png` (180). |

The server's invite landing and legal pages don't use these yet. To wire them, serve
`out/web/*` and add `<link rel="icon" href="/favicon.ico">` and
`<link rel="apple-touch-icon" href="/apple-touch-icon.png">` to their `<head>`.

## Desktop window icon

The macOS bundle gets its icon from `AppIcon.icns` (cargo-packager). Rune's `AppSpec` has no window
or dock icon setting at the pinned revision, so an unbundled `cargo run` still shows the default
icon. Set it from `branding/TheHideout.png` once Rune supports it.
