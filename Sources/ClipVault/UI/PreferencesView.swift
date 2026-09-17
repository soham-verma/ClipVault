import SwiftUI

struct PreferencesView: View {
    @ObservedObject var settings: Settings
    @State private var newExcludedApp: String = ""

    var body: some View {
        Form {
            Section("General") {
                Toggle("Launch at login", isOn: Binding(
                    get: { settings.launchAtLogin },
                    set: { newValue in
                        settings.launchAtLogin = newValue
                        LoginItem.setEnabled(newValue)
                    }
                ))
                Toggle("Paste directly into frontmost app", isOn: $settings.pasteOnSelect)
                Toggle("Ignore password manager copies", isOn: $settings.ignorePasswordManagers)
                Text("Hotkey: ⌘⇧V")
                    .foregroundStyle(.secondary)
            }

            Section("Storage") {
                Stepper(value: $settings.maxHistoryCount, in: 0...1_000_000, step: 1000) {
                    Text(settings.maxHistoryCount == 0 ? "Item limit: Unlimited" : "Item limit: \(settings.maxHistoryCount)")
                }
                Stepper(value: $settings.maxDiskMegabytes, in: 100...102_400, step: 100) {
                    Text("Max disk usage: \(settings.maxDiskMegabytes) MB")
                }
                Text("Text is nearly free to store; images and files count toward the disk cap. Pinned items are never auto-removed.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Excluded Apps") {
                ForEach(settings.excludedBundleIDs, id: \.self) { bundleID in
                    HStack {
                        Text(bundleID)
                        Spacer()
                        Button(role: .destructive) {
                            settings.excludedBundleIDs.removeAll { $0 == bundleID }
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.plain)
                    }
                }
                HStack {
                    TextField("com.example.app", text: $newExcludedApp)
                    Button("Add") {
                        let trimmed = newExcludedApp.trimmingCharacters(in: .whitespaces)
                        guard !trimmed.isEmpty else { return }
                        settings.excludedBundleIDs.append(trimmed)
                        newExcludedApp = ""
                    }
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 440, height: 420)
    }
}
