import Foundation
import Vision
import AppKit

// Read a screenshot's text through Vision, printing it top-to-bottom.
// Usage: swift ocr.swift <image.png>
let path = CommandLine.arguments[1]
guard let image = NSImage(contentsOfFile: path),
      let tiff = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiff),
      let cg = bitmap.cgImage else { exit(2) }
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
let handler = VNImageRequestHandler(cgImage: cg, options: [:])
try handler.perform([request])
guard let results = request.results else { exit(3) }
var lines: [(CGFloat, CGFloat, String)] = []
for observation in results {
    guard let candidate = observation.topCandidates(1).first else { continue }
    lines.append((observation.boundingBox.midY, observation.boundingBox.minX, candidate.string))
}
lines.sort { left, right in
    abs(left.0 - right.0) < 0.002 ? left.1 < right.1 : left.0 > right.0
}
print(lines.map { $0.2 }.joined(separator: "\n"))
