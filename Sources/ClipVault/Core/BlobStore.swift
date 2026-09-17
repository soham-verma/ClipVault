import Foundation
import CryptoKit

/// Stores large clipboard payloads (images, file lists) on disk instead of in the DB row,
/// so the SQLite file stays small and fast even with an enormous history.
final class BlobStore {
    let rootDirectory: URL

    init() {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        rootDirectory = appSupport.appendingPathComponent("ClipVault/Blobs", isDirectory: true)
        try? FileManager.default.createDirectory(at: rootDirectory, withIntermediateDirectories: true)
    }

    /// Writes data under a content-hash-derived filename (shards into subdirectories to avoid
    /// huge flat directories once history grows into the hundreds of thousands of items).
    func write(_ data: Data, ext: String) -> (path: String, hash: String) {
        let digest = SHA256.hash(data: data)
        let hash = digest.map { String(format: "%02x", $0) }.joined()
        let shard = String(hash.prefix(2))
        let dir = rootDirectory.appendingPathComponent(shard, isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let filename = "\(hash).\(ext)"
        let fileURL = dir.appendingPathComponent(filename)
        if !FileManager.default.fileExists(atPath: fileURL.path) {
            try? data.write(to: fileURL, options: .atomic)
        }
        // Store paths relative to rootDirectory so the store is relocatable.
        return ("\(shard)/\(filename)", hash)
    }

    func url(for relativePath: String) -> URL {
        rootDirectory.appendingPathComponent(relativePath)
    }

    func delete(relativePath: String) {
        try? FileManager.default.removeItem(at: url(for: relativePath))
    }

    func totalSizeOnDisk() -> Int64 {
        guard let enumerator = FileManager.default.enumerator(at: rootDirectory, includingPropertiesForKeys: [.fileSizeKey]) else { return 0 }
        var total: Int64 = 0
        for case let fileURL as URL in enumerator {
            if let size = try? fileURL.resourceValues(forKeys: [.fileSizeKey]).fileSize {
                total += Int64(size)
            }
        }
        return total
    }
}
