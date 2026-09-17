import Foundation
import Combine

final class Settings: ObservableObject {
    static let shared = Settings()

    private let defaults = UserDefaults.standard

    @Published var maxHistoryCount: Int {
        didSet { defaults.set(maxHistoryCount, forKey: "maxHistoryCount") }
    }
    @Published var maxDiskMegabytes: Int {
        didSet { defaults.set(maxDiskMegabytes, forKey: "maxDiskMegabytes") }
    }
    @Published var launchAtLogin: Bool {
        didSet { defaults.set(launchAtLogin, forKey: "launchAtLogin") }
    }
    @Published var ignorePasswordManagers: Bool {
        didSet { defaults.set(ignorePasswordManagers, forKey: "ignorePasswordManagers") }
    }
    @Published var pasteOnSelect: Bool {
        didSet { defaults.set(pasteOnSelect, forKey: "pasteOnSelect") }
    }
    @Published var excludedBundleIDs: [String] {
        didSet { defaults.set(excludedBundleIDs, forKey: "excludedBundleIDs") }
    }

    private init() {
        // 0 means "unlimited" (bounded only by maxDiskMegabytes).
        maxHistoryCount = defaults.object(forKey: "maxHistoryCount") as? Int ?? 0
        maxDiskMegabytes = defaults.object(forKey: "maxDiskMegabytes") as? Int ?? 2048
        launchAtLogin = defaults.object(forKey: "launchAtLogin") as? Bool ?? false
        ignorePasswordManagers = defaults.object(forKey: "ignorePasswordManagers") as? Bool ?? true
        pasteOnSelect = defaults.object(forKey: "pasteOnSelect") as? Bool ?? true
        excludedBundleIDs = defaults.object(forKey: "excludedBundleIDs") as? [String] ?? []
    }
}
