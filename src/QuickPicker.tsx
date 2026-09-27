import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Clipboard,
  Search,
  Settings2,
  Network,
  ArrowUpRight,
  Power,
  X,
} from "lucide-react";
import { ClipRow } from "./ClipRow";
import {
  api,
  connectionLabel,
  type Item,
  type Kind,
  type Page,
  type Snapshot,
} from "./api";

export function QuickPicker() {
  const [page, setPage] = useState<Page>({
    items: [],
    total: 0,
    has_more: false,
  });
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const search = useRef<HTMLInputElement>(null);
  const revision = useRef(0);
  const copying = useRef(false);
  const reload = useCallback(async () => {
    const current = ++revision.current;
    try {
      const result = await api.history(query, kind);
      if (current === revision.current) {
        setPage(result);
        setSelected((old) =>
          result.items.some((item) => item.id === old)
            ? old
            : (result.items[0]?.id ?? null),
        );
        setLoading(false);
      }
    } catch (e) {
      setError(String(e));
      setLoading(false);
    }
  }, [query, kind]);
  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.snapshot());
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const timer = setTimeout(() => void reload(), 100);
    return () => clearTimeout(timer);
  }, [reload]);
  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    for (const name of [
      "history-changed",
      "connection-changed",
      "capabilities-changed",
      "quick-opened",
      "app-error",
    ]) {
      void listen(name, (event) => {
        if (disposed) return;
        if (name === "quick-opened") {
          setQuery("");
          setKind(null);
          setSelected(null);
          setError("");
          search.current?.focus();
          void reload();
          void refresh();
        } else if (name === "history-changed") void reload();
        else if (name === "app-error") setError(String(event.payload));
        else void refresh();
      }).then((stop) => {
        if (disposed) stop();
        else cleanups.push(stop);
      });
    }
    return () => {
      disposed = true;
      cleanups.forEach((stop) => stop());
    };
  }, [reload, refresh]);
  useEffect(() => {
    if (selected !== null)
      document
        .getElementById(`clip-${selected}`)
        ?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  async function copy(item: Item) {
    if (copying.current) return;
    copying.current = true;
    setBusy(true);
    try {
      await api.copy(item.id);
    } catch (e) {
      setError(String(e));
    } finally {
      copying.current = false;
      setBusy(false);
    }
  }
  async function change(action: () => Promise<unknown>) {
    try {
      await action();
      await reload();
    } catch (e) {
      setError(String(e));
    }
  }
  async function more() {
    const current = revision.current;
    setBusy(true);
    try {
      const next = await api.history(query, kind, page.items.length);
      if (current === revision.current)
        setPage((old) => ({ ...next, items: [...old.items, ...next.items] }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const status = snapshot?.status;
  return (
    <div
      className="quick-picker"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          void getCurrentWindow().hide();
        }
        if (
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          search.current?.focus();
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const index = page.items.findIndex((item) => item.id === selected);
          const next = Math.max(
            0,
            Math.min(
              page.items.length - 1,
              index + (event.key === "ArrowDown" ? 1 : -1),
            ),
          );
          setSelected(page.items[next]?.id ?? null);
        }
        const target = event.target as HTMLElement;
        if (
          event.key === "Enter" &&
          (target === search.current ||
            target.getAttribute("role") === "listbox")
        ) {
          const item = page.items.find((item) => item.id === selected);
          if (item) {
            event.preventDefault();
            void copy(item);
          }
        }
        if (
          /^[1-9]$/.test(event.key) &&
          event.target !== search.current &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.altKey
        ) {
          const item = page.items[Number(event.key) - 1];
          if (item) {
            event.preventDefault();
            void copy(item);
          }
        }
      }}
    >
      <div className="search-box quick-search">
        <Search size={17} />
        <input
          autoFocus
          ref={search}
          value={query}
          aria-label="Search clipboard"
          placeholder="Search clipboard history"
          onChange={(e) => setQuery(e.target.value)}
        />
        {query && (
          <button aria-label="Clear search" onClick={() => setQuery("")}>
            <X size={16} />
          </button>
        )}
      </div>
      <div className="filter-row quick-filters">
        {(
          [
            [null, "All"],
            ["text", "Text"],
            ["image", "Images"],
            ["url", "Links"],
            ["fileList", "Files"],
          ] as [Kind | null, string][]
        ).map(([value, label]) => (
          <button
            key={label}
            className={`filter ${kind === value ? "selected" : ""}`}
            onClick={() => setKind(value)}
          >
            {label}
          </button>
        ))}
      </div>
      {error && (
        <div className="quick-error" role="alert">
          <span>{error}</span>
          <button aria-label="Dismiss error" onClick={() => setError("")}>
            <X size={14} />
          </button>
        </div>
      )}
      {snapshot && !snapshot.capabilities.monitoring && (
        <button
          className="quick-setup"
          onClick={() => void change(() => api.workspace("settings"))}
        >
          Set up clipboard access <ArrowUpRight size={14} />
        </button>
      )}
      {!!status?.pairings.length && (
        <button
          className="quick-setup"
          onClick={() => void change(() => api.workspace("sharing"))}
        >
          Verify a new device <ArrowUpRight size={14} />
        </button>
      )}
      <div
        className="history-list quick-list"
        role="listbox"
        aria-label="Quick clipboard history"
        aria-activedescendant={selected ? `clip-${selected}` : undefined}
        aria-busy={busy}
        tabIndex={0}
      >
        {page.items.map((item, index) => (
          <ClipRow
            key={item.id}
            item={item}
            index={index}
            selected={item.id === selected}
            onSelect={() => setSelected(item.id)}
            onCopy={() => void copy(item)}
            onPin={() => void change(() => api.pin(item.id, !item.pinned))}
            onDelete={() => void change(() => api.delete(item.id))}
          />
        ))}
        {!page.items.length && (
          <div className="quick-empty">
            <Clipboard size={30} />
            <strong>
              {loading
                ? "Loading your clipboard…"
                : query
                  ? "No matches"
                  : "No clipboard history yet"}
            </strong>
            <span>
              {query
                ? "Try another word or filter."
                : "Copied items will appear here."}
            </span>
          </div>
        )}
        {page.has_more && (
          <button
            className="load-more"
            disabled={busy}
            onClick={() => void more()}
          >
            Load more
          </button>
        )}
      </div>
      <div className="quick-footer">
        <span>{page.total.toLocaleString()} items</span>
        <span className="quick-footer-actions">
          <button
            className="quick-clear"
            disabled={busy || page.total === 0}
            onClick={() => void change(() => api.clear(true))}
          >
            Clear
          </button>
          <button
            className={status?.peers.length ? "connected" : ""}
            title={status ? connectionLabel(status) : "Device sharing"}
            aria-label="Device sharing"
            onClick={() => void change(() => api.workspace("sharing"))}
          >
            <Network size={15} />
          </button>
          <button
            title="Preferences"
            aria-label="Preferences"
            onClick={() => void change(() => api.workspace("settings"))}
          >
            <Settings2 size={15} />
          </button>
          <button
            title="Quit ClipVault"
            aria-label="Quit ClipVault"
            onClick={() => void api.quit()}
          >
            <Power size={15} />
          </button>
        </span>
      </div>
    </div>
  );
}
