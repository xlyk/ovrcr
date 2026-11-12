// Post a real click or wheel scroll at a point inside the fixture's window.
// usage: mouse <pid> click <x> <y> | mouse <pid> wheel <x> <y> <lines> <count>
// x and y are points from the window's top-left corner. Refuses unless the
// fixture process is frontmost and owns the topmost window at that point.
import AppKit
import CoreGraphics

let args = CommandLine.arguments
let pid = Int(args[1])!
let x = Double(args[3])!, y = Double(args[4])!
guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid_t(pid) else {
    FileHandle.standardError.write("fixture is not frontmost\n".data(using: .utf8)!); exit(2)
}
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
guard let window = list.first(where: { ($0[kCGWindowOwnerPID as String] as? Int) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }),
      let bounds = CGRect(dictionaryRepresentation: window[kCGWindowBounds as String] as! CFDictionary) else {
    FileHandle.standardError.write("fixture window not found\n".data(using: .utf8)!); exit(2)
}
let point = CGPoint(x: bounds.minX + x, y: bounds.minY + y)
// The first layer-0 window in front-to-back order that contains the point must be ours.
let top = list.first(where: {
    ($0[kCGWindowLayer as String] as? Int) == 0
        && (CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary)?.contains(point) ?? false)
})
guard (top?[kCGWindowOwnerPID as String] as? Int) == pid else {
    FileHandle.standardError.write("another window covers that point\n".data(using: .utf8)!); exit(2)
}
func post(_ event: CGEvent?) { event?.post(tap: .cghidEventTap); usleep(60_000) }
post(CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left))
if args[2] == "click" {
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left))
    post(CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left))
} else {
    let lines = Int32(args[5])!, count = Int(args[6])!
    for _ in 0..<count {
        post(CGEvent(scrollWheelEvent2Source: nil, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0))
    }
}
print("ok \(point.x) \(point.y) window \(bounds.width)x\(bounds.height)")
