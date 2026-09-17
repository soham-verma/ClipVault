import SwiftUI

struct PopoverView: View {
    @ObservedObject var store: HistoryStore
    let blobStore: BlobStore
    let monitor: ClipboardMonitor
    let onCopyItem: (ClipItem, Bool) -> Void
    let onOpenPreferences: () -> Void
    let onQuit: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            filterBar
            Divider()
            if store.items.isEmpty {
                emptyState
            } else {
                List {
                    ForEach(Array(store.items.enumerated()), id: \.element.id) { index, item in
                        RowView(
                            item: item,
                            index: index,
                            blobStore: blobStore,
                            onCopy: { onCopyItem(item, true) },
                            onPin: { store.togglePin(item) },
                            onDelete: { store.delete(item) }
                        )
                        .onAppear { store.loadMoreIfNeeded(currentItem: item) }
                        .listRowInsets(EdgeInsets())
                    }
                }
                .listStyle(.plain)
            }
            Divider()
            footer
        }
        .frame(width: 380, height: 480)
    }

    private var header: some View {
        HStack {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Search clipboard history", text: $store.searchText)
                .textFieldStyle(.plain)
            if !store.searchText.isEmpty {
                Button {
                    store.searchText = ""
                } label: {
                    Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(10)
    }

    private var filterBar: some View {
        HStack(spacing: 6) {
            filterChip(nil, label: "All")
            filterChip(.text, label: "Text")
            filterChip(.image, label: "Images")
            filterChip(.url, label: "Links")
            filterChip(.fileList, label: "Files")
            Spacer()
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
    }

    private func filterChip(_ kind: ClipKind?, label: String) -> some View {
        let isSelected = store.kindFilter == kind
        return Button {
            store.kindFilter = kind
        } label: {
            Text(label)
                .font(.system(size: 11, weight: .medium))
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(isSelected ? Color.accentColor.opacity(0.2) : Color.clear)
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
    }

    private var emptyState: some View {
        VStack(spacing: 8) {
            Spacer()
            Image(systemName: "clipboard").font(.system(size: 32)).foregroundStyle(.secondary)
            Text(store.searchText.isEmpty ? "No clipboard history yet" : "No matches")
                .foregroundStyle(.secondary)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }

    private var footer: some View {
        HStack {
            Text("\(store.totalCount) items")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            Spacer()
            Button("Clear", role: .destructive) { store.clearAll(keepPinned: true) }
                .buttonStyle(.plain)
                .font(.system(size: 11))
            Button {
                onOpenPreferences()
            } label: {
                Image(systemName: "gearshape")
            }
            .buttonStyle(.plain)
            Button {
                onQuit()
            } label: {
                Image(systemName: "power")
            }
            .buttonStyle(.plain)
        }
        .padding(8)
    }
}
