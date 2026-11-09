import Foundation
import Vision
import AppKit

// Usage: swift ocrbox.swift <image.png> <query>
let path = CommandLine.arguments[1]
let query = CommandLine.arguments[2].lowercased()
guard let image = NSImage(contentsOfFile: path), let tiff = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiff), let cg = bitmap.cgImage else { exit(2) }
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
try VNImageRequestHandler(cgImage: cg, options: [:]).perform([request])
let width = CGFloat(cg.width), height = CGFloat(cg.height)
for observation in request.results ?? [] {
    guard let text = observation.topCandidates(1).first?.string else { continue }
    if text.lowercased().contains(query) {
        let b = observation.boundingBox
        let x = b.minX * width, y = (1 - b.maxY) * height
        print("\(Int(x))\t\(Int(y))\t\(Int(b.width * width))\t\(Int(b.height * height))\t\(text)")
    }
}
