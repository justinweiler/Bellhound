#!/usr/bin/env swift
// Local-only credential entry for the experimental Bellhound MCP canary.
// The value is written only to the parent process pipe. This helper never uses
// clipboard APIs, command-line arguments, files, logs, or persistent storage.

import AppKit
import Foundation

final class VisibilityController: NSObject {
    let secureField: NSSecureTextField
    let visibleField: NSTextField

    init(secureField: NSSecureTextField, visibleField: NSTextField) {
        self.secureField = secureField
        self.visibleField = visibleField
    }

    @objc func toggleVisibility(_ sender: NSButton) {
        if sender.state == .on {
            visibleField.stringValue = secureField.stringValue
            secureField.isHidden = true
            visibleField.isHidden = false
            visibleField.window?.makeFirstResponder(visibleField)
        } else {
            secureField.stringValue = visibleField.stringValue
            visibleField.isHidden = true
            secureField.isHidden = false
            secureField.window?.makeFirstResponder(secureField)
        }
    }
}

let application = NSApplication.shared
application.setActivationPolicy(.accessory)

let accessory = NSView(frame: NSRect(x: 0, y: 0, width: 520, height: 104))
let explanation = NSTextField(labelWithString:
    "Paste the restricted one-day tunnel key. Use Show key to verify it before submission. The value is not saved."
)
explanation.frame = NSRect(x: 0, y: 72, width: 520, height: 32)
explanation.maximumNumberOfLines = 2

let secureField = NSSecureTextField(frame: NSRect(x: 0, y: 38, width: 520, height: 26))
secureField.placeholderString = "Restricted OpenAI tunnel runtime key"
let visibleField = NSTextField(frame: secureField.frame)
visibleField.placeholderString = secureField.placeholderString
visibleField.isHidden = true

let checkbox = NSButton(checkboxWithTitle: "Show key", target: nil, action: nil)
checkbox.frame = NSRect(x: 0, y: 4, width: 140, height: 24)
let controller = VisibilityController(secureField: secureField, visibleField: visibleField)
checkbox.target = controller
checkbox.action = #selector(VisibilityController.toggleVisibility(_:))

accessory.addSubview(explanation)
accessory.addSubview(secureField)
accessory.addSubview(visibleField)
accessory.addSubview(checkbox)

let alert = NSAlert()
alert.alertStyle = .informational
alert.messageText = "Bellhound Gate 2 credential"
alert.informativeText = "This native dialog is local to your Mac. Browser and agent inspection must remain paused until it closes."
alert.accessoryView = accessory
alert.addButton(withTitle: "Use key")
alert.addButton(withTitle: "Cancel")

application.activate(ignoringOtherApps: true)
alert.window.initialFirstResponder = secureField
let response = alert.runModal()
guard response == .alertFirstButtonReturn else {
    exit(2)
}

let value = checkbox.state == .on ? visibleField.stringValue : secureField.stringValue
guard !value.isEmpty, value.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else {
    exit(3)
}
FileHandle.standardOutput.write(Data(value.utf8))
