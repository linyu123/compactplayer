import AppKit

let output = CommandLine.arguments.dropFirst().first ?? "resources/AppIcon.png"
let size = 1024
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
let context = NSGraphicsContext.current!.cgContext
context.clear(CGRect(x: 0, y: 0, width: size, height: size))

func color(_ red: CGFloat, _ green: CGFloat, _ blue: CGFloat, _ alpha: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: red / 255, green: green / 255, blue: blue / 255, alpha: alpha)
}

let tile = NSBezierPath(roundedRect: NSRect(x: 80, y: 80, width: 864, height: 864), xRadius: 194, yRadius: 194)
context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -16), blur: 28, color: NSColor.black.withAlphaComponent(0.32).cgColor)
color(20, 29, 46).setFill()
tile.fill()
context.restoreGState()
NSGradient(starting: color(51, 67, 91), ending: color(11, 18, 31))!.draw(in: tile, angle: -60)
color(154, 191, 225, 0.24).setStroke()
tile.lineWidth = 2
tile.stroke()

let disc = NSBezierPath(ovalIn: NSRect(x: 206, y: 206, width: 612, height: 612))
NSGradient(starting: color(17, 28, 46), ending: color(30, 48, 68))!.draw(in: disc, angle: 60)
color(135, 191, 219, 0.16).setStroke()
disc.lineWidth = 3
disc.stroke()

let arc = NSBezierPath()
arc.appendArc(withCenter: NSPoint(x: 512, y: 512), radius: 304, startAngle: 49, endAngle: 143)
arc.lineWidth = 9
arc.lineCapStyle = .round
color(107, 220, 245, 0.7).setStroke()
arc.stroke()

let play = NSBezierPath()
play.move(to: NSPoint(x: 438, y: 337))
play.curve(to: NSPoint(x: 411, y: 353), controlPoint1: NSPoint(x: 425, y: 329), controlPoint2: NSPoint(x: 411, y: 337))
play.line(to: NSPoint(x: 411, y: 671))
play.curve(to: NSPoint(x: 438, y: 687), controlPoint1: NSPoint(x: 411, y: 687), controlPoint2: NSPoint(x: 425, y: 695))
play.line(to: NSPoint(x: 697, y: 528))
play.curve(to: NSPoint(x: 697, y: 496), controlPoint1: NSPoint(x: 711, y: 520), controlPoint2: NSPoint(x: 711, y: 504))
play.close()
context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -8), blur: 24, color: color(0, 183, 234, 0.3).cgColor)
color(41, 194, 239).setFill()
play.fill()
context.restoreGState()
NSGradient(starting: color(140, 248, 233), ending: color(22, 157, 243))!.draw(in: play, angle: -65)

NSGraphicsContext.restoreGraphicsState()
try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: output))
print(output)
