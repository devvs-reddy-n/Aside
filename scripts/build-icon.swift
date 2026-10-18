// Export the generated master artwork to desktop icons.
// Run from the project root: swift scripts/build-icon.swift
import Foundation

let files = FileManager.default
let root = URL(fileURLWithPath: files.currentDirectoryPath)
let source = root.appendingPathComponent("public/aside-icon.png")
guard files.fileExists(atPath: source.path) else { fatalError("Missing public/aside-icon.png") }
let temporary = files.temporaryDirectory.appendingPathComponent("aside-icons-\(UUID().uuidString)")
try files.createDirectory(at: temporary, withIntermediateDirectories: true)
defer { try? files.removeItem(at: temporary) }

// Tauri handles the platform sizes, alpha channels and native containers.
let export = Process()
export.executableURL = URL(fileURLWithPath: "/usr/bin/env")
export.arguments = ["npx", "tauri", "icon", source.path, "--output", temporary.path]
try export.run()
export.waitUntilExit()
guard export.terminationStatus == 0 else { fatalError("Icon export failed") }

let destination = root.appendingPathComponent("src-tauri/icons")
for name in ["icon.png", "32x32.png", "128x128.png", "128x128@2x.png", "icon.icns", "icon.ico"] {
    let target = destination.appendingPathComponent(name)
    if files.fileExists(atPath: target.path) { try files.removeItem(at: target) }
    try files.copyItem(at: temporary.appendingPathComponent(name), to: target)
    print("Updated \(target.path)")
}
