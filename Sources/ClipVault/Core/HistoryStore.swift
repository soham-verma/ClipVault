import Foundation
import Combine

@MainActor
final class HistoryStore: ObservableObject {
    @Published var items: [ClipItem] = []
    @Published var searchText: String = "" {
        didSet { reload() }
    }
    @Published var kindFilter: ClipKind? = nil {
        didSet { reload() }
    }
    @Published var totalCount: Int = 0

    private let db: Database
    private let blobStore: BlobStore
    private let pageSize = 200
    private var canLoadMore = true

    init(db: Database, blobStore: BlobStore) {
        self.db = db
        self.blobStore = blobStore
        reload()
    }

    func reload() {
        let page = db.fetchPage(offset: 0, limit: pageSize, kindFilter: kindFilter, searchQuery: searchText)
        items = page
        canLoadMore = page.count == pageSize
        totalCount = db.totalCount()
    }

    func loadMoreIfNeeded(currentItem: ClipItem) {
        guard canLoadMore, let index = items.firstIndex(where: { $0.id == currentItem.id }) else { return }
        if index >= items.count - 20 {
            loadNextPage()
        }
    }

    private func loadNextPage() {
        let nextPage = db.fetchPage(offset: items.count, limit: pageSize, kindFilter: kindFilter, searchQuery: searchText)
        canLoadMore = nextPage.count == pageSize
        items.append(contentsOf: nextPage)
    }

    func delete(_ item: ClipItem) {
        db.deleteItem(id: item.id)
        if let path = item.blobPath { blobStore.delete(relativePath: path) }
        items.removeAll { $0.id == item.id }
        totalCount = db.totalCount()
    }

    func togglePin(_ item: ClipItem) {
        db.setPinned(id: item.id, pinned: !item.pinned)
        reload()
    }

    func clearAll(keepPinned: Bool) {
        if !keepPinned {
            for item in db.fetchPage(offset: 0, limit: Int.max, kindFilter: nil, searchQuery: nil) {
                if let path = item.blobPath { blobStore.delete(relativePath: path) }
            }
        }
        db.clearAll(keepPinned: keepPinned)
        reload()
    }

    func fileURL(for item: ClipItem) -> URL? {
        guard let path = item.blobPath else { return nil }
        return blobStore.url(for: path)
    }
}
