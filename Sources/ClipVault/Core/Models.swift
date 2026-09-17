import Foundation

enum ClipKind: String, Codable {
    case text
    case rtf
    case image
    case fileList
    case url
}

struct ClipItem: Identifiable, Equatable {
    let id: Int64
    var kind: ClipKind
    var previewText: String
    var fullText: String?
    var blobPath: String?
    var sourceApp: String?
    var sourceAppName: String?
    var createdAt: Date
    var pinned: Bool
    var pixelWidth: Int
    var pixelHeight: Int
    var byteSize: Int
}
