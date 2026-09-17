import Foundation
import SQLite3

private let SQLITE_TRANSIENT = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

final class Database {
    private var db: OpaquePointer?
    private let queue = DispatchQueue(label: "clipvault.db")

    init(path: String) {
        if sqlite3_open(path, &db) != SQLITE_OK {
            fatalError("Unable to open database at \(path)")
        }
        exec("PRAGMA journal_mode=WAL;")
        exec("PRAGMA synchronous=NORMAL;")
        exec("PRAGMA foreign_keys=ON;")
        createSchema()
    }

    deinit {
        sqlite3_close(db)
    }

    private func createSchema() {
        exec("""
        CREATE TABLE IF NOT EXISTS items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            preview_text TEXT NOT NULL DEFAULT '',
            full_text TEXT,
            blob_path TEXT,
            source_app TEXT,
            source_app_name TEXT,
            created_at REAL NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0,
            pixel_width INTEGER NOT NULL DEFAULT 0,
            pixel_height INTEGER NOT NULL DEFAULT 0,
            byte_size INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT
        );
        """)
        exec("CREATE INDEX IF NOT EXISTS idx_items_created_at ON items(created_at DESC);")
        exec("CREATE INDEX IF NOT EXISTS idx_items_pinned ON items(pinned);")
        exec("CREATE INDEX IF NOT EXISTS idx_items_hash ON items(content_hash);")
        exec("""
        CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
            full_text, content='items', content_rowid='id', tokenize='porter unicode61'
        );
        """)
        exec("""
        CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
            INSERT INTO items_fts(rowid, full_text) VALUES (new.id, coalesce(new.full_text, new.preview_text));
        END;
        """)
        exec("""
        CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, full_text) VALUES('delete', old.id, coalesce(old.full_text, old.preview_text));
        END;
        """)
        exec("""
        CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, full_text) VALUES('delete', old.id, coalesce(old.full_text, old.preview_text));
            INSERT INTO items_fts(rowid, full_text) VALUES (new.id, coalesce(new.full_text, new.preview_text));
        END;
        """)
    }

    @discardableResult
    private func exec(_ sql: String) -> Bool {
        var errMsg: UnsafeMutablePointer<Int8>?
        let result = sqlite3_exec(db, sql, nil, nil, &errMsg)
        if result != SQLITE_OK {
            let message = errMsg.map { String(cString: $0) } ?? "unknown error"
            sqlite3_free(errMsg)
            print("SQLite error: \(message) — SQL: \(sql.prefix(200))")
            return false
        }
        return true
    }

    // MARK: - Writes

    func insert(kind: ClipKind, previewText: String, fullText: String?, blobPath: String?,
                sourceApp: String?, sourceAppName: String?, createdAt: Date,
                pixelWidth: Int, pixelHeight: Int, byteSize: Int, contentHash: String?) -> Int64 {
        queue.sync {
            let sql = """
            INSERT INTO items (kind, preview_text, full_text, blob_path, source_app, source_app_name,
                                created_at, pinned, pixel_width, pixel_height, byte_size, content_hash)
            VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?);
            """
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return -1 }
            defer { sqlite3_finalize(stmt) }
            bindText(stmt, 1, kind.rawValue)
            bindText(stmt, 2, previewText)
            bindText(stmt, 3, fullText)
            bindText(stmt, 4, blobPath)
            bindText(stmt, 5, sourceApp)
            bindText(stmt, 6, sourceAppName)
            sqlite3_bind_double(stmt, 7, createdAt.timeIntervalSince1970)
            sqlite3_bind_int64(stmt, 8, Int64(pixelWidth))
            sqlite3_bind_int64(stmt, 9, Int64(pixelHeight))
            sqlite3_bind_int64(stmt, 10, Int64(byteSize))
            bindText(stmt, 11, contentHash)
            guard sqlite3_step(stmt) == SQLITE_DONE else { return -1 }
            return sqlite3_last_insert_rowid(db)
        }
    }

    func deleteItem(id: Int64) {
        queue.sync {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "DELETE FROM items WHERE id = ?;", -1, &stmt, nil) == SQLITE_OK else { return }
            defer { sqlite3_finalize(stmt) }
            sqlite3_bind_int64(stmt, 1, id)
            sqlite3_step(stmt)
        }
    }

    func setPinned(id: Int64, pinned: Bool) {
        queue.sync {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "UPDATE items SET pinned = ? WHERE id = ?;", -1, &stmt, nil) == SQLITE_OK else { return }
            defer { sqlite3_finalize(stmt) }
            sqlite3_bind_int64(stmt, 1, pinned ? 1 : 0)
            sqlite3_bind_int64(stmt, 2, id)
            sqlite3_step(stmt)
        }
    }

    func clearAll(keepPinned: Bool) {
        queue.sync {
            _ = exec(keepPinned ? "DELETE FROM items WHERE pinned = 0;" : "DELETE FROM items;")
        }
    }

    /// Deletes oldest, unpinned items beyond `keepCount`, and returns their blob paths for cleanup.
    func trimHistory(keepCount: Int) -> [String] {
        queue.sync {
            var blobPaths: [String] = []
            let selectSql = """
            SELECT id, blob_path FROM items WHERE pinned = 0
            ORDER BY created_at DESC LIMIT -1 OFFSET ?;
            """
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, selectSql, -1, &stmt, nil) == SQLITE_OK else { return [] }
            sqlite3_bind_int64(stmt, 1, Int64(keepCount))
            var ids: [Int64] = []
            while sqlite3_step(stmt) == SQLITE_ROW {
                ids.append(sqlite3_column_int64(stmt, 0))
                if let cstr = sqlite3_column_text(stmt, 1) {
                    blobPaths.append(String(cString: cstr))
                }
            }
            sqlite3_finalize(stmt)
            for id in ids {
                deleteItem(id: id)
            }
            return blobPaths
        }
    }

    func mostRecentHash() -> String? {
        queue.sync {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "SELECT content_hash FROM items ORDER BY created_at DESC LIMIT 1;", -1, &stmt, nil) == SQLITE_OK else { return nil }
            defer { sqlite3_finalize(stmt) }
            guard sqlite3_step(stmt) == SQLITE_ROW else { return nil }
            guard let cstr = sqlite3_column_text(stmt, 0) else { return nil }
            return String(cString: cstr)
        }
    }

    // MARK: - Reads

    func fetchPage(offset: Int, limit: Int, kindFilter: ClipKind?, searchQuery: String?) -> [ClipItem] {
        queue.sync {
            var sql: String
            var stmt: OpaquePointer?
            if let q = searchQuery, !q.trimmingCharacters(in: .whitespaces).isEmpty {
                sql = """
                SELECT items.* FROM items
                JOIN items_fts ON items.id = items_fts.rowid
                WHERE items_fts MATCH ?
                \(kindFilter != nil ? "AND items.kind = ?" : "")
                ORDER BY items.pinned DESC, items.created_at DESC
                LIMIT ? OFFSET ?;
                """
                guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
                let matchTerm = ftsQuery(from: q)
                bindText(stmt, 1, matchTerm)
                var idx: Int32 = 2
                if let kf = kindFilter {
                    bindText(stmt, idx, kf.rawValue)
                    idx += 1
                }
                sqlite3_bind_int64(stmt, idx, Int64(limit)); idx += 1
                sqlite3_bind_int64(stmt, idx, Int64(offset))
            } else {
                sql = """
                SELECT * FROM items
                \(kindFilter != nil ? "WHERE kind = ?" : "")
                ORDER BY pinned DESC, created_at DESC
                LIMIT ? OFFSET ?;
                """
                guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
                var idx: Int32 = 1
                if let kf = kindFilter {
                    bindText(stmt, idx, kf.rawValue)
                    idx += 1
                }
                sqlite3_bind_int64(stmt, idx, Int64(limit)); idx += 1
                sqlite3_bind_int64(stmt, idx, Int64(offset))
            }
            defer { sqlite3_finalize(stmt) }
            var results: [ClipItem] = []
            while sqlite3_step(stmt) == SQLITE_ROW {
                results.append(rowToItem(stmt))
            }
            return results
        }
    }

    func totalCount() -> Int {
        queue.sync {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "SELECT COUNT(*) FROM items;", -1, &stmt, nil) == SQLITE_OK else { return 0 }
            defer { sqlite3_finalize(stmt) }
            guard sqlite3_step(stmt) == SQLITE_ROW else { return 0 }
            return Int(sqlite3_column_int64(stmt, 0))
        }
    }

    func totalByteSize() -> Int64 {
        queue.sync {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "SELECT COALESCE(SUM(byte_size),0) FROM items;", -1, &stmt, nil) == SQLITE_OK else { return 0 }
            defer { sqlite3_finalize(stmt) }
            guard sqlite3_step(stmt) == SQLITE_ROW else { return 0 }
            return sqlite3_column_int64(stmt, 0)
        }
    }

    // MARK: - Helpers

    private func ftsQuery(from raw: String) -> String {
        // Wrap each token in quotes with a trailing * for prefix matching, escaping inner quotes.
        let tokens = raw.split(separator: " ").map { token -> String in
            let escaped = token.replacingOccurrences(of: "\"", with: "\"\"")
            return "\"\(escaped)\"*"
        }
        return tokens.joined(separator: " ")
    }

    private func bindText(_ stmt: OpaquePointer?, _ index: Int32, _ value: String?) {
        if let value = value {
            sqlite3_bind_text(stmt, index, value, -1, SQLITE_TRANSIENT)
        } else {
            sqlite3_bind_null(stmt, index)
        }
    }

    private func rowToItem(_ stmt: OpaquePointer?) -> ClipItem {
        let id = sqlite3_column_int64(stmt, 0)
        let kindStr = String(cString: sqlite3_column_text(stmt, 1))
        let previewText = String(cString: sqlite3_column_text(stmt, 2))
        let fullText = sqlite3_column_text(stmt, 3).map { String(cString: $0) }
        let blobPath = sqlite3_column_text(stmt, 4).map { String(cString: $0) }
        let sourceApp = sqlite3_column_text(stmt, 5).map { String(cString: $0) }
        let sourceAppName = sqlite3_column_text(stmt, 6).map { String(cString: $0) }
        let createdAt = Date(timeIntervalSince1970: sqlite3_column_double(stmt, 7))
        let pinned = sqlite3_column_int64(stmt, 8) != 0
        let pixelWidth = Int(sqlite3_column_int64(stmt, 9))
        let pixelHeight = Int(sqlite3_column_int64(stmt, 10))
        let byteSize = Int(sqlite3_column_int64(stmt, 11))
        return ClipItem(id: id, kind: ClipKind(rawValue: kindStr) ?? .text, previewText: previewText,
                         fullText: fullText, blobPath: blobPath, sourceApp: sourceApp,
                         sourceAppName: sourceAppName, createdAt: createdAt, pinned: pinned,
                         pixelWidth: pixelWidth, pixelHeight: pixelHeight, byteSize: byteSize)
    }
}
