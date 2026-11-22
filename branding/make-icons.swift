// Generates every platform icon from branding/TheHideout.png. Run through make-icons.sh.
//
//   make-icons.swift <TheHideout.png> <out dir> <AppIcon.icns output dir for the .iconset>
//
// The artwork is a 64x64 pixel-art grid drawn at 10x. The grid is recovered by sampling pixel
// centres, checked to round-trip to the original, and only ever enlarged by integer pixel
// replication (never interpolated). Sizes that cannot be an integer multiple of 64 are reduced from
// an integer-scaled render with an area filter, which is a reduction, not a smoothing of the art.
import CoreGraphics
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count == 4 else { fatalError("usage: make-icons.swift <TheHideout.png> <out dir> <iconset dir>") }
let srcPath = args[1], outDir = args[2], iconsetDir = args[3]
let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
let rgbaInfo = CGImageAlphaInfo.premultipliedLast.rawValue

func fail(_ m: String) -> Never { FileHandle.standardError.write(Data((m + "\n").utf8)); exit(1) }

func ensureDir(_ p: String) { try! FileManager.default.createDirectory(atPath: p, withIntermediateDirectories: true) }

func makeCtx(_ w: Int, _ h: Int, opaque: Bool = false) -> CGContext {
    CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: srgb,
              bitmapInfo: opaque ? CGImageAlphaInfo.noneSkipLast.rawValue : rgbaInfo)!
}

func savePNG(_ img: CGImage, _ path: String) {
    ensureDir((path as NSString).deletingLastPathComponent)
    let dst = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(dst, img, nil)
    if !CGImageDestinationFinalize(dst) { fail("could not write \(path)") }
}

func pngData(_ img: CGImage) -> Data {
    let d = NSMutableData()
    let dst = CGImageDestinationCreateWithData(d, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(dst, img, nil)
    CGImageDestinationFinalize(dst)
    return d as Data
}

// MARK: grid

let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: srcPath) as CFURL, nil)!
let srcImage = CGImageSourceCreateImageAtIndex(src, 0, nil)!
guard srcImage.width == 640, srcImage.height == 640 else { fail("expected a 640x640 source") }
var orig = [UInt8](repeating: 0, count: 640 * 640 * 4)
do {
    let c = CGContext(data: &orig, width: 640, height: 640, bitsPerComponent: 8, bytesPerRow: 2560, space: srgb, bitmapInfo: rgbaInfo)!
    c.draw(srcImage, in: CGRect(x: 0, y: 0, width: 640, height: 640))
}

var grid = [UInt8](repeating: 0, count: 64 * 64 * 4)  // RGBA, row 0 on top
for gy in 0..<64 { for gx in 0..<64 {
    let s = ((gy * 10 + 5) * 640 + gx * 10 + 5) * 4, d = (gy * 64 + gx) * 4
    for k in 0..<4 { grid[d + k] = orig[s + k] }
} }

/// Integer pixel replication of a w x h RGBA buffer by k.
func replicate(_ px: [UInt8], _ w: Int, _ h: Int, _ k: Int) -> [UInt8] {
    var out = [UInt8](repeating: 0, count: w * k * h * k * 4)
    for y in 0..<(h * k) { for x in 0..<(w * k) {
        let s = ((y / k) * w + x / k) * 4, d = (y * w * k + x) * 4
        for c in 0..<4 { out[d + c] = px[s + c] }
    } }
    return out
}

func image(_ px: [UInt8], _ w: Int, _ h: Int, opaque: Bool = false) -> CGImage {
    var p = px
    let ctx = CGContext(data: &p, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: srgb,
                        bitmapInfo: opaque ? CGImageAlphaInfo.noneSkipLast.rawValue : rgbaInfo)!
    return ctx.makeImage()!  // makeImage copies the bytes
}

// Round trip: the grid enlarged 10x must equal the source byte for byte.
let back = replicate(grid, 64, 64, 10)
if back != orig { fail("ROUND-TRIP FAILED: the 640 px source is not an exact 10x enlargement of a 64x64 grid") }
print("round-trip OK: 64x64 grid x10 == original 640x640 (\(back.count) bytes identical)")
if stride(from: 3, to: grid.count, by: 4).contains(where: { grid[$0] != 255 }) { print("note: artwork has transparent pixels") }

/// The art as a square image of `size` px: exact integer replication when size is a multiple of 64,
/// otherwise an integer-scaled render reduced with an area filter.
func art(_ size: Int, opaque: Bool = false) -> CGImage {
    if size % 64 == 0 { return image(replicate(grid, 64, 64, size / 64), size, size, opaque: opaque) }
    let k = (size + 63) / 64
    let big = image(replicate(grid, 64, 64, k), 64 * k, 64 * k, opaque: opaque)
    let c = makeCtx(size, size, opaque: opaque)
    c.interpolationQuality = .high
    c.draw(big, in: CGRect(x: 0, y: 0, width: size, height: size))
    return c.makeImage()!
}

func scaleDown(_ img: CGImage, to size: Int) -> CGImage {
    let c = makeCtx(size, size)
    c.interpolationQuality = .high
    c.draw(img, in: CGRect(x: 0, y: 0, width: size, height: size))
    return c.makeImage()!
}

// MARK: macOS

/// 1024 canvas, 832 px body (13 x 64, the nearest integer multiple to the 824 px of the macOS icon
/// grid), corner radius 185 scaled to the body, a soft drop shadow like system icons.
func macMaster() -> CGImage {
    let c = makeCtx(1024, 1024)
    let body = CGRect(x: 96, y: 96, width: 832, height: 832)
    let path = CGPath(roundedRect: body, cornerWidth: 832 * 185 / 824, cornerHeight: 832 * 185 / 824, transform: nil)
    c.saveGState()
    c.setShadow(offset: CGSize(width: 0, height: -12), blur: 24, color: CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.35))
    c.addPath(path)
    c.setFillColor(CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 1))
    c.fillPath()
    c.restoreGState()
    c.saveGState()
    c.addPath(path)
    c.clip()
    c.interpolationQuality = .none
    c.draw(art(832), in: body)
    c.restoreGState()
    return c.makeImage()!
}

let master = macMaster()
let iconset = iconsetDir + "/AppIcon.iconset"
try? FileManager.default.removeItem(atPath: iconset)
ensureDir(iconset)
let iconsetSizes: [(String, Int)] = [
    ("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256),
    ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024),
]
for (name, px) in iconsetSizes {
    savePNG(px == 1024 ? master : scaleDown(master, to: px), "\(iconset)/icon_\(name).png")
}
savePNG(master, outDir + "/macos/AppIcon-1024.png")
savePNG(scaleDown(master, to: 32), outDir + "/macos/AppIcon-32.png")

// MARK: iOS

ensureDir(outDir + "/ios")
savePNG(art(1024, opaque: true), outDir + "/ios/AppIcon-1024.png")
savePNG(art(1024, opaque: true), outDir + "/ios/AppIcon-1024-dark.png")  // already a night scene
var gray = grid
for i in stride(from: 0, to: gray.count, by: 4) {
    let l = UInt8((0.2126 * Double(gray[i]) + 0.7152 * Double(gray[i + 1]) + 0.0722 * Double(gray[i + 2])).rounded())
    gray[i] = l; gray[i + 1] = l; gray[i + 2] = l
}
savePNG(image(replicate(gray, 64, 64, 16), 1024, 1024, opaque: true), outDir + "/ios/AppIcon-1024-tinted.png")

// MARK: Android

let android = outDir + "/android"
// Background: the darkest sky pixel.
var darkest = (255, 255, 255), best = Int.max
for i in stride(from: 0, to: grid.count, by: 4) {
    let s = Int(grid[i]) + Int(grid[i + 1]) + Int(grid[i + 2])
    if s < best { best = s; darkest = (Int(grid[i]), Int(grid[i + 1]), Int(grid[i + 2])) }
}
let bgHex = String(format: "#%02X%02X%02X", darkest.0, darkest.1, darkest.2)
ensureDir(android + "/values")
try! """
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <color name="ic_launcher_background">\(bgHex)</color>
</resources>

""".write(toFile: android + "/values/ic_launcher_background.xml", atomically: true, encoding: .utf8)
ensureDir(android + "/mipmap-anydpi-v26")
try! """
<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@color/ic_launcher_background" />
    <foreground android:drawable="@drawable/ic_launcher_foreground" />
    <monochrome android:drawable="@drawable/ic_launcher_monochrome" />
</adaptive-icon>

""".write(toFile: android + "/mipmap-anydpi-v26/ic_launcher.xml", atomically: true, encoding: .utf8)
try! FileManager.default.copyItem(atPath: android + "/mipmap-anydpi-v26/ic_launcher.xml", toPath: android + "/mipmap-anydpi-v26/ic_launcher_round.xml") 
    
// Foreground: 108 dp canvas at xxxhdpi = 432 px. The visible mask is the central 72 dp (288 px); the art
// is 5x (320 px) centred, so it covers the whole visible area, with the camp scene in the middle.
func androidLayer(_ px: [UInt8]) -> CGImage {
    let layer = makeCtx(432, 432)
    layer.interpolationQuality = .none
    layer.draw(image(replicate(px, 64, 64, 5), 320, 320), in: CGRect(x: 56, y: 56, width: 320, height: 320))
    return layer.makeImage()!
}
ensureDir(android)
savePNG(androidLayer(grid), android + "/ic_launcher_foreground.png")
do {
    let c = makeCtx(432, 432)
    c.setFillColor(CGColor(srgbRed: CGFloat(darkest.0) / 255, green: CGFloat(darkest.1) / 255, blue: CGFloat(darkest.2) / 255, alpha: 1))
    c.fill(CGRect(x: 0, y: 0, width: 432, height: 432))
    savePNG(c.makeImage()!, android + "/ic_launcher_background.png")
}
// Monochrome (themed icons): white where the tree, tent and fire are, transparent elsewhere. Classified
// by palette and position since the ground shares the tree's greens.
var mono = [UInt8](repeating: 0, count: 64 * 64 * 4)
let tentColors: Set<String> = ["8a6846", "caa682", "71563a", "b9946f", "352a10"]
let fireColors: Set<String> = ["ac3232", "d5d339", "d9ab37"]
let treeColors: Set<String> = ["266f52", "14593d", "37946e", "0a3c28"]
for gy in 0..<64 { for gx in 0..<64 {
    let i = (gy * 64 + gx) * 4
    let hex = String(format: "%02x%02x%02x", grid[i], grid[i + 1], grid[i + 2])
    let on = (tentColors.contains(hex) && gx >= 38)
        || (tentColors.contains(hex) && gx < 17 && gy >= 46 && gy <= 54)  // trunk
        || (fireColors.contains(hex) && gx >= 20 && gx <= 40 && gy >= 44)
        || (treeColors.contains(hex) && gx < 26 && gy >= 14 && gy < 47)
    if on { for k in 0..<4 { mono[i + k] = 255 } }
} }
savePNG(androidLayer(mono), android + "/ic_launcher_monochrome.png")

// Legacy launcher icons: the full art in a rounded square (12%) and a circle.
for (density, px) in [("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)] {
    let a = art(px)
    for (name, round) in [("ic_launcher", false), ("ic_launcher_round", true)] {
        let c = makeCtx(px, px)
        let r = CGRect(x: 0, y: 0, width: px, height: px)
        c.addPath(round ? CGPath(ellipseIn: r, transform: nil)
                        : CGPath(roundedRect: r, cornerWidth: CGFloat(px) * 0.12, cornerHeight: CGFloat(px) * 0.12, transform: nil))
        c.clip()
        c.draw(a, in: r)
        savePNG(c.makeImage()!, "\(android)/mipmap-\(density)/\(name).png")
    }
}
savePNG(art(512, opaque: true), android + "/play_store_512.png")

// MARK: web

let web = outDir + "/web"
let f16 = art(16), f32 = art(32)
savePNG(f16, web + "/favicon-16.png")
savePNG(f32, web + "/favicon-32.png")
savePNG(art(180, opaque: true), web + "/apple-touch-icon.png")
// favicon.ico holding the 16 and 32 px PNGs.
do {
    let imgs = [(16, pngData(f16)), (32, pngData(f32))]
    var ico = Data([0, 0, 1, 0, UInt8(imgs.count), 0])
    var offset = 6 + 16 * imgs.count
    func le32(_ v: Int) -> [UInt8] { (0..<4).map { UInt8((v >> (8 * $0)) & 255) } }
    for (s, d) in imgs {
        ico.append(contentsOf: [UInt8(s), UInt8(s), 0, 0, 1, 0, 32, 0] + le32(d.count) + le32(offset))
        offset += d.count
    }
    for (_, d) in imgs { ico.append(d) }
    try! ico.write(to: URL(fileURLWithPath: web + "/favicon.ico"))
}
print("background colour \(bgHex)")
