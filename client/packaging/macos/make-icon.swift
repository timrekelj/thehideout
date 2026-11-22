// Draws the app icon's 1024x1024 master: a rounded square in the theme's default (sage-teal)
// accent with a minimal mark, an arched doorway, in the accent's ink colour.
// Usage: swift make-icon.swift out.png
import AppKit
import CoreGraphics

// OKLCH -> sRGB, so the colours are the design tokens' (design/src/tokens/accent.rs).
func oklch(_ l: Double, _ c: Double, _ hDeg: Double) -> CGColor {
    let h = hDeg * .pi / 180
    let a = c * cos(h), b = c * sin(h)
    let l_ = pow(l + 0.3963377774 * a + 0.2158037573 * b, 3)
    let m_ = pow(l - 0.1055613458 * a - 0.0638541728 * b, 3)
    let s_ = pow(l - 0.0894841775 * a - 1.2914855480 * b, 3)
    let r = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_
    let g = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_
    let bl = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_
    func enc(_ v: Double) -> CGFloat {
        let v = min(max(v, 0), 1)
        return CGFloat(v <= 0.0031308 ? 12.92 * v : 1.055 * pow(v, 1 / 2.4) - 0.055)
    }
    return CGColor(srgbRed: enc(r), green: enc(g), blue: enc(bl), alpha: 1)
}

let size = 1024
let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// macOS icon grid: an 824pt square centred on the 1024pt canvas.
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
ctx.setFillColor(oklch(0.74, 0.090, 175))
ctx.addPath(CGPath(roundedRect: body, cornerWidth: 185, cornerHeight: 185, transform: nil))
ctx.fillPath()

// The doorway: a rounded-top arch, centred.
let w: CGFloat = 250, h: CGFloat = 400
let door = CGRect(x: 512 - w / 2, y: 512 - h / 2 - 20, width: w, height: h)
let path = CGMutablePath()
path.move(to: CGPoint(x: door.minX, y: door.minY))
path.addLine(to: CGPoint(x: door.minX, y: door.maxY - w / 2))
path.addArc(center: CGPoint(x: door.midX, y: door.maxY - w / 2), radius: w / 2,
            startAngle: .pi, endAngle: 0, clockwise: true)
path.addLine(to: CGPoint(x: door.maxX, y: door.minY))
path.closeSubpath()
ctx.setFillColor(oklch(0.20, 0.030, 175))
ctx.addPath(path)
ctx.fillPath()

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[1]))
