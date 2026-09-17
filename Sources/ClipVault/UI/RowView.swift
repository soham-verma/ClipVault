import SwiftUI
import AppKit

struct RowView: View {
    let item: ClipItem
    let index: Int
    let blobStore: BlobStore
    let onCopy: () -> Void
    let onPin: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if index < 9 {
                Text("\(index + 1)")
                    .font(.system(size: 10, weight: .medium, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .frame(width: 14, alignment: .center)
            } else {
                Spacer().frame(width: 14)
            }

            icon
                .frame(width: 32, height: 32)
                .clipShape(RoundedRectangle(cornerRadius: 6))

            VStack(alignment: .leading, spacing: 2) {
                Text(item.previewText.isEmpty ? "(empty)" : item.previewText)
                    .font(.system(size: 13))
                    .lineLimit(2)
                    .truncationMode(.tail)
                HStack(spacing: 6) {
                    if let appName = item.sourceAppName {
                        Text(appName)
                    }
                    Text(item.createdAt, style: .relative)
                }
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            }

            Spacer()

            if item.pinned {
                Image(systemName: "pin.fill")
                    .font(.system(size: 11))
                    .foregroundStyle(.orange)
            }
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 8)
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { onCopy() }
        .contextMenu {
            Button("Copy", action: onCopy)
            Button(item.pinned ? "Unpin" : "Pin", action: onPin)
            Divider()
            Button("Delete", role: .destructive, action: onDelete)
        }
    }

    @ViewBuilder
    private var icon: some View {
        switch item.kind {
        case .image:
            if let path = item.blobPath, let nsImage = NSImage(contentsOf: blobStore.url(for: path)) {
                Image(nsImage: nsImage).resizable().aspectRatio(contentMode: .fill)
            } else {
                placeholderIcon("photo")
            }
        case .fileList:
            placeholderIcon("doc.on.doc")
        case .url:
            placeholderIcon("link")
        case .rtf:
            placeholderIcon("doc.richtext")
        case .text:
            placeholderIcon("doc.plaintext")
        }
    }

    private func placeholderIcon(_ systemName: String) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 6).fill(Color.secondary.opacity(0.15))
            Image(systemName: systemName).foregroundStyle(.secondary)
        }
    }
}
