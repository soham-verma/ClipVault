import React, { useState, useEffect, useCallback, useRef } from "react";
import { createRoot } from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Clipboard,
  History,
  Network,
  Settings2,
  Search,
  Trash2,
  ArrowRight,
  Check,
  X,
  ShieldCheck,
  Monitor,
  Pause,
  Play,
  LogOut,
  Plus,
  RefreshCw,
  ChevronDown,
  Wifi,
  Command,
  Power,
} from "lucide-react";
import {
  api,
  connectionLabel,
  type Item,
  type Kind,
  type Page,
  type Snapshot,
  type Settings,
} from "./api";
import "./style.css";
import { ClipRow } from "./ClipRow";
import { QuickPicker } from "./QuickPicker";
import { ShortcutInput } from "./ShortcutInput";

type View = "history" | "sharing" | "settings";
const emptyPage: Page = { items: [], total: 0, has_more: false };
function App() {
  const [view, setView] = useState<View>("history"),
    [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [page, setPage] = useState<Page>(emptyPage);
  const [query, setQuery] = useState(""),
    [kind, setKind] = useState<Kind | null>(null),
    [selected, setSelected] = useState<number | null>(null);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(
      null,
    ),
    [busy, setBusy] = useState(false),
    [loading, setLoading] = useState(true);
  const request = useRef(0),
    search = useRef<HTMLInputElement>(null);
  const refresh = useCallback(async () => {
    try {
      setSnapshot(await api.snapshot());
    } catch (e) {
      setNotice({ text: String(e), error: true });
    }
  }, []);
  const reload = useCallback(async () => {
    const id = ++request.current;
    try {
      const result = await api.history(query, kind);
      if (id === request.current) {
        setPage(result);
        setLoading(false);
      }
    } catch (e) {
      setNotice({ text: String(e), error: true });
      setLoading(false);
    }
  }, [query, kind]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const stop = listen<View>("navigate", ({ payload }) => setView(payload));
    let disposed = false;
    void stop
      .then(() => api.workspaceView())
      .then((view) => {
        if (!disposed) setView(view);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      void stop.then((unlisten) => unlisten());
    };
  }, []);
  useEffect(() => {
    const timer = setTimeout(() => void reload(), 120);
    return () => clearTimeout(timer);
  }, [reload]);
  useEffect(() => {
    let disposed = false;
    const remove: (() => void)[] = [];
    for (const name of [
      "history-changed",
      "connection-changed",
      "discovery-changed",
      "capabilities-changed",
      "app-error",
    ]) {
      void listen(name, (event) => {
        if (disposed) return;
        if (name === "history-changed") void reload();
        else if (name === "app-error")
          setNotice({ text: String(event.payload), error: true });
        else void refresh();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else remove.push(unlisten);
      });
    }
    return () => {
      disposed = true;
      remove.forEach((fn) => fn());
    };
  }, [refresh, reload]);
  useEffect(() => {
    if (!notice || notice.error) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);
  async function act(operation: () => Promise<unknown>, success?: string) {
    setBusy(true);
    try {
      const result = await operation();
      if (success) setNotice({ text: success, error: false });
      await refresh();
      return result;
    } catch (e) {
      setNotice({ text: String(e), error: true });
    } finally {
      setBusy(false);
    }
  }
  async function copy(item: Item) {
    await act(() => api.copy(item.id), "Copied to your clipboard");
  }
  async function loadMore() {
    const id = request.current;
    setBusy(true);
    try {
      const next = await api.history(query, kind, page.items.length);
      if (id === request.current)
        setPage((old) => ({ ...next, items: [...old.items, ...next.items] }));
    } catch (e) {
      setNotice({ text: String(e), error: true });
    } finally {
      setBusy(false);
    }
  }
  const status = snapshot?.status;
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-icon">
            <Clipboard size={21} />
          </span>
          <span>
            ClipVault<small>A little less friction.</small>
          </span>
        </div>
        <div className="nav-label">YOUR WORKSPACE</div>
        <nav>
          {(
            [
              ["history", History, "Clipboard"],
              ["sharing", Network, "Device sharing"],
              ["settings", Settings2, "Preferences"],
            ] as const
          ).map(([key, Icon, label]) => (
            <button
              key={key}
              className={view === key ? "nav active" : "nav"}
              onClick={() => setView(key)}
            >
              <Icon size={18} />
              {label}
              {key === "sharing" && !!status?.peers.length && (
                <span className="count">{status.peers.length}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="connection">
            <span
              className={`dot ${status?.peers.length && !status.paused && !status.room_paused ? "online" : ""}`}
            />
            <span>{status ? connectionLabel(status) : "Starting…"}</span>
          </div>
          <p>
            Your history stays on this device.
            <br />
            Sharing stays on your network.
          </p>
          <button className="quiet quit" onClick={() => void api.quit()}>
            <Power size={14} />
            Quit ClipVault
          </button>
        </div>
      </aside>
      <main>
        {notice && (
          <div
            className={`notice ${notice.error ? "error" : ""}`}
            role={notice.error ? "alert" : "status"}
          >
            <span>{notice.text}</span>
            <button
              aria-label="Dismiss message"
              onClick={() => setNotice(null)}
            >
              <X size={16} />
            </button>
          </div>
        )}
        {!snapshot ? (
          <div className="empty">
            <Clipboard size={38} />
            <h2>Opening your workspace</h2>
            <p>Loading clipboard history and device settings.</p>
            <button onClick={() => void refresh()}>Retry</button>
          </div>
        ) : (
          <>
            {!snapshot.capabilities.monitoring && (
              <div className="setup-banner">
                <ShieldCheck size={20} />
                <div>
                  <strong>Clipboard access needs setup</strong>
                  <p>{snapshot.capabilities.message}</p>
                </div>
                <button
                  className="secondary"
                  onClick={() => setView("settings")}
                >
                  Set up
                </button>
              </div>
            )}
            {view === "history" && (
              <>
                <header>
                  <div>
                    <div className="eyebrow">ALWAYS WITHIN REACH</div>
                    <h1>Your clipboard</h1>
                    <p>Everything you copied, ready for what’s next.</p>
                  </div>
                  <span className="total">
                    {page.total.toLocaleString()} items
                  </span>
                </header>
                <div className="search-box">
                  <Search size={18} />
                  <input
                    ref={search}
                    aria-label="Search clipboard history"
                    placeholder="Find something you copied…"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                  />
                  {query ? (
                    <button
                      aria-label="Clear search"
                      onClick={() => setQuery("")}
                    >
                      <X size={16} />
                    </button>
                  ) : (
                    <kbd>
                      <Command size={11} /> F
                    </kbd>
                  )}
                </div>
                <div className="filter-row">
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
                      onClick={() => {
                        setKind(value);
                        setSelected(null);
                      }}
                    >
                      {label}
                    </button>
                  ))}
                  <span className="filter-spacer" />
                  <span className="hint">Double-click to copy</span>
                </div>
                <div
                  className="history-list"
                  role="listbox"
                  aria-label="Clipboard history"
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      void getCurrentWindow().hide();
                      return;
                    }
                    if ((e.metaKey || e.ctrlKey) && e.key === "f") {
                      e.preventDefault();
                      search.current?.focus();
                      return;
                    }
                    if (e.target !== e.currentTarget) return;
                    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                      e.preventDefault();
                      const index = page.items.findIndex(
                        (i) => i.id === selected,
                      );
                      const next = Math.max(
                        0,
                        Math.min(
                          page.items.length - 1,
                          index + (e.key === "ArrowDown" ? 1 : -1),
                        ),
                      );
                      setSelected(page.items[next]?.id ?? null);
                    }
                    const item =
                      e.key === "Enter"
                        ? page.items.find((i) => i.id === selected)
                        : /^[1-9]$/.test(e.key)
                          ? page.items[Number(e.key) - 1]
                          : null;
                    if (item) {
                      e.preventDefault();
                      void copy(item);
                    }
                  }}
                >
                  {loading ? (
                    <div className="empty">
                      <p>Loading history…</p>
                    </div>
                  ) : page.items.length === 0 ? (
                    <div className="empty">
                      <div className="empty-icon">
                        <Clipboard size={32} />
                      </div>
                      <h2>{query ? "No matches yet" : "A fresh clipboard"}</h2>
                      <p>
                        {query
                          ? "Try a different word or content type."
                          : "Copy some text, a link, or an image. It will appear here."}
                      </p>
                    </div>
                  ) : (
                    page.items.map((item, index) => (
                      <ClipRow
                        key={item.id}
                        item={item}
                        index={index}
                        selected={selected === item.id}
                        onSelect={() => setSelected(item.id)}
                        onCopy={() => void copy(item)}
                        onPin={() =>
                          void act(() => api.pin(item.id, !item.pinned))
                        }
                        onDelete={() => void act(() => api.delete(item.id))}
                      />
                    ))
                  )}
                  {page.has_more && (
                    <button
                      className="load-more"
                      disabled={busy}
                      onClick={() => void loadMore()}
                    >
                      Load more <ChevronDown size={16} />
                    </button>
                  )}
                </div>
                <footer>
                  <span>
                    <ShieldCheck size={14} /> Stored locally
                  </span>
                  <span>
                    ↑ ↓ to browse <span className="sep">·</span> Enter to copy{" "}
                    <span className="sep">·</span> 1–9 for quick access
                  </span>
                </footer>
              </>
            )}
            {view === "sharing" && (
              <Sharing snapshot={snapshot} busy={busy} act={act} />
            )}
            {view === "settings" && (
              <Preferences snapshot={snapshot} busy={busy} act={act} />
            )}
          </>
        )}
      </main>
      {status?.pairings.map((pairing) => (
        <div className="modal-backdrop" key={pairing.id}>
          <section
            className="pairing-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="pairing-title"
          >
            <span className="modal-icon">
              <ShieldCheck size={30} />
            </span>
            <div className="eyebrow">ONE-TIME VERIFICATION</div>
            <h2 id="pairing-title">Is this your device?</h2>
            <p>
              Compare this code with the one shown on{" "}
              <strong>{pairing.device_name}</strong>. Only approve if every
              group matches.
            </p>
            <div className="verification">
              {pairing.verification.match(/.{1,4}/g)?.join(" ")}
            </div>
            <p className="muted">
              Both devices must confirm. Clipboard content stays private until
              then. This device will be remembered for automatic reconnection.
            </p>
            <div className="actions">
              <button
                className="secondary"
                disabled={busy}
                onClick={() => void act(() => api.confirm(pairing.id, false))}
              >
                Reject
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void act(() => api.confirm(pairing.id, true))}
              >
                <Check size={16} />
                Codes match
              </button>
            </div>
          </section>
        </div>
      ))}
    </div>
  );
}

type Act = (
  operation: () => Promise<unknown>,
  success?: string,
) => Promise<unknown>;
function Sharing({
  snapshot,
  busy,
  act,
}: {
  snapshot: Snapshot;
  busy: boolean;
  act: Act;
}) {
  const [code, setCode] = useState(""),
    [address, setAddress] = useState(""),
    [bind, setBind] = useState(""),
    [advanced, setAdvanced] = useState(false),
    [clock, setClock] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setClock(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const status = snapshot.status,
    active = status.phase !== "offline",
    hosting = status.role === "host" && active;
  const remaining = Math.max(
    0,
    Math.floor((status.code_expires_at ?? 0) - clock / 1000),
  );
  return (
    <div className="scroll-page">
      <header>
        <div>
          <div className="eyebrow">ONE CLIPBOARD. YOUR DEVICES.</div>
          <h1>Better together</h1>
          <p>Copy here. Paste there. All on your local network.</p>
        </div>
        <Network className="header-art" size={35} />
      </header>
      <div className="sharing-note">
        <ShieldCheck size={17} />
        <span>Encrypted connections · Mac + Linux · No account needed</span>
      </div>
      {!active ? (
        <div className="sharing-grid">
          <section className="card">
            <span className="card-icon">
              <Wifi size={24} />
            </span>
            <h2>Host a server</h2>
            <p>
              Start a shared clipboard and invite your other devices. Keep this
              computer running while you share.
            </p>
            {snapshot.interfaces.length > 1 && (
              <label>
                Network interface
                <select value={bind} onChange={(e) => setBind(e.target.value)}>
                  <option value="">Automatic</option>
                  {snapshot.interfaces.map(([name, ip]) => (
                    <option key={`${name}-${ip}`} value={ip}>
                      {name} · {ip}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <button
              className="primary"
              disabled={busy || !snapshot.capabilities.monitoring}
              onClick={() => void act(() => api.host(bind || null))}
            >
              Host Server <ArrowRight size={16} />
            </button>
          </section>
          <section className="card">
            <span className="card-icon blue">
              <Monitor size={24} />
            </span>
            <h2>Join a server</h2>
            <p>
              Enter the six-digit code shown on the hosting device, then compare
              the verification codes.
            </p>
            <label htmlFor="join-code">Server code</label>
            <input
              id="join-code"
              className="code-input"
              inputMode="numeric"
              maxLength={6}
              placeholder="000 000"
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
            />
            <button
              className="quiet advanced"
              onClick={() => setAdvanced(!advanced)}
            >
              {advanced ? "Hide" : "Enter"} IP and port manually{" "}
              <ChevronDown size={13} />
            </button>
            {advanced && (
              <input
                aria-label="Server IP and port"
                placeholder="192.168.1.5:43210"
                value={address}
                onChange={(e) => setAddress(e.target.value)}
              />
            )}
            <button
              className="secondary full"
              disabled={
                busy || code.length !== 6 || !snapshot.capabilities.monitoring
              }
              onClick={() => void act(() => api.join(code, address || null))}
            >
              Join Server <ArrowRight size={16} />
            </button>
          </section>
        </div>
      ) : (
        <section className="card session-card">
          <div className="session-top">
            <div>
              <span
                className={`status-tag ${status.paused || status.room_paused ? "paused" : ""}`}
              >
                <span className="dot online" />
                {connectionLabel(status)}
              </span>
              <h2>{hosting ? "Your shared clipboard" : "You’re connected"}</h2>
              <p>{status.address}</p>
            </div>
            <button
              className="secondary"
              disabled={busy}
              onClick={() => void act(() => api.pause(!status.paused))}
            >
              {status.paused ? <Play size={15} /> : <Pause size={15} />}{" "}
              {status.paused ? "Resume" : "Pause"}
            </button>
          </div>
          {hosting && (
            <div className="invite">
              <div>
                <span className="eyebrow">INVITE A DEVICE</span>
                <div className="join-code">
                  {remaining > 0 && status.join_code
                    ? `${status.join_code.slice(0, 3)} ${status.join_code.slice(3)}`
                    : "— — —"}
                </div>
                <small>
                  {remaining > 0
                    ? `Code expires in ${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`
                    : "Open a code when you want to pair a new device."}
                </small>
              </div>
              <button
                className="secondary"
                disabled={busy}
                onClick={() => void act(() => api.newCode())}
              >
                <Plus size={15} />
                {remaining > 0 ? "New code" : "Add a device"}
              </button>
            </div>
          )}
          <div className="device-list">
            {status.peers.length ? (
              status.peers.map((peer) => (
                <div className="device" key={peer.id}>
                  <span className="device-icon">
                    <Monitor size={19} />
                  </span>
                  <div>
                    <strong>{peer.name}</strong>
                    <small>{peer.paused ? "Paused" : "Ready to share"}</small>
                  </div>
                  <span className={`dot ${peer.paused ? "" : "online"}`} />
                </div>
              ))
            ) : (
              <div className="waiting">
                <Network size={23} />
                <span>
                  {hosting
                    ? "Waiting for your other devices…"
                    : "Looking for the host…"}
                </span>
              </div>
            )}
          </div>
          <p className="muted">
            New text, links, and images are shared with everyone connected. Your
            existing history and copied files stay local.
          </p>
          <button
            className="quiet danger"
            disabled={busy}
            onClick={() => void act(() => api.disconnect())}
          >
            <LogOut size={15} />
            {hosting ? "Stop server" : "Disconnect"}
          </button>
        </section>
      )}
      {!active && snapshot.nearby.filter((h) => h.code).length > 0 && (
        <section className="nearby">
          <h3>Nearby servers</h3>
          {snapshot.nearby
            .filter((h) => h.code)
            .map((host) => (
              <button
                className="nearby-host"
                key={host.id}
                onClick={() => {
                  setAddress(host.address);
                  setAdvanced(true);
                }}
              >
                <Monitor size={16} />
                <span>
                  {host.name}
                  <small>{host.address}</small>
                </span>
                <ArrowRight size={14} />
              </button>
            ))}
        </section>
      )}
      {snapshot.trusted.length > 0 && (
        <section className="trusted">
          <h3>Remembered devices</h3>
          <p className="muted">
            Verified devices reconnect automatically when the apps reopen.
            Disconnecting stops automatic reconnection.
          </p>
          {snapshot.trusted.map((device) => (
            <div className="device" key={device.id}>
              <Monitor size={18} />
              <div>
                <strong>{device.name}</strong>
                <small>Verified connection</small>
              </div>
              <button
                className="quiet danger"
                disabled={busy}
                onClick={() =>
                  void act(() => api.forget(device.id), "Device forgotten")
                }
              >
                Forget
              </button>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}

function Preferences({
  snapshot,
  busy,
  act,
}: {
  snapshot: Snapshot;
  busy: boolean;
  act: Act;
}) {
  const [draft, setDraft] = useState<Settings>(snapshot.settings),
    [clearOpen, setClearOpen] = useState(false);
  const caps = snapshot.capabilities;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setDraft((old) => ({ ...old, [key]: value }));
  return (
    <div className="scroll-page preferences">
      <header>
        <div>
          <div className="eyebrow">MAKE IT YOURS</div>
          <h1>Preferences</h1>
          <p>Simple controls for your clipboard and your privacy.</p>
        </div>
      </header>
      <section className="settings-section">
        <h3>This device</h3>
        <label>
          Device name
          <input
            value={draft.device_name}
            maxLength={128}
            onChange={(e) => set("device_name", e.target.value)}
          />
        </label>
        <Toggle
          label="Open ClipVault at login"
          description="Runs quietly in the background."
          value={draft.launch_at_login}
          onChange={(v) => set("launch_at_login", v)}
        />
        <Toggle
          label="Paste when selecting history"
          description={
            caps.automatic_paste
              ? "Copies and pastes into your previous app. Mac may require Accessibility access."
              : "This desktop supports copying; paste with your usual keyboard shortcut."
          }
          value={draft.paste_on_select && caps.automatic_paste}
          disabled={!caps.automatic_paste}
          onChange={(v) => set("paste_on_select", v)}
        />
        <ShortcutInput
          value={draft.shortcut}
          onChange={(value) => set("shortcut", value)}
        />
      </section>
      <section className="settings-section">
        <h3>
          Clipboard access{" "}
          <span className={`status-tag ${caps.monitoring ? "" : "paused"}`}>
            {caps.backend}
          </span>
        </h3>
        {caps.message && <p className="muted">{caps.message}</p>}
        {caps.helper_required && (
          <button
            className="secondary"
            disabled={busy}
            onClick={() =>
              void act(async () => {
                const message = await api.helper();
                return message;
              }).then((result) => {
                if (typeof result === "string") window.alert(result);
              })
            }
          >
            <Plus size={15} />
            Install GNOME helper
          </button>
        )}
        <button
          className="quiet"
          disabled={busy}
          onClick={() =>
            void act(() => api.retry(), "Clipboard access checked")
          }
        >
          <RefreshCw size={14} />
          Recheck clipboard access
        </button>
      </section>
      <section className="settings-section">
        <h3>History storage</h3>
        <div className="field-grid">
          <label>
            Disk budget (MiB)
            <input
              type="number"
              min={100}
              max={102400}
              value={draft.max_disk_megabytes}
              onChange={(e) =>
                set("max_disk_megabytes", Number(e.target.value))
              }
            />
          </label>
          <label>
            Item limit (0 = unlimited)
            <input
              type="number"
              min={0}
              max={1000000}
              value={draft.max_history_count}
              onChange={(e) => set("max_history_count", Number(e.target.value))}
            />
          </label>
        </div>
        <p className="muted">
          Oldest unpinned items are removed first. Pinned items are retained,
          even when they exceed the budget.
        </p>
      </section>
      <section className="settings-section">
        <h3>Privacy</h3>
        <p className="muted">
          Copies marked sensitive by password managers are excluded from history
          and sharing. Clipboard contents and pairing secrets are never written
          to logs.
        </p>
        <label>
          Excluded applications
          <textarea
            disabled={!caps.app_exclusions}
            rows={3}
            placeholder={
              caps.app_exclusions
                ? "One app identifier per line"
                : "Unavailable on this clipboard backend"
            }
            value={draft.excluded_apps.join("\n")}
            onChange={(e) =>
              set(
                "excluded_apps",
                e.target.value
                  .split("\n")
                  .map((v) => v.trim())
                  .filter(Boolean),
              )
            }
          />
        </label>
      </section>
      <div className="settings-actions">
        <button
          className="primary"
          disabled={busy}
          onClick={() =>
            void act(() => api.settings(draft), "Preferences saved")
          }
        >
          <Check size={16} />
          Save preferences
        </button>
        <button
          className="quiet danger"
          onClick={() => setClearOpen(!clearOpen)}
        >
          <Trash2 size={15} />
          Clear history
        </button>
      </div>
      {clearOpen && (
        <div className="clear-confirm">
          <p>
            Remove unpinned clipboard history from this device? Pinned items
            will be kept.
          </p>
          <button
            className="secondary danger"
            disabled={busy}
            onClick={() =>
              void act(() => api.clear(true), "History cleared").then(() =>
                setClearOpen(false),
              )
            }
          >
            Clear unpinned history
          </button>
          <button className="quiet" onClick={() => setClearOpen(false)}>
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}
function Toggle({
  label,
  description,
  value,
  disabled,
  onChange,
}: {
  label: string;
  description: string;
  value: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="toggle-row">
      <span>
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
      <input
        type="checkbox"
        role="switch"
        checked={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
    </label>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <>
      {new URLSearchParams(location.search).get("mode") === "quick" ? (
        <QuickPicker />
      ) : (
        <App />
      )}
    </>
  </React.StrictMode>,
);
