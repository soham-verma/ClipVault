import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";

function bridge() {
  let changed:
    ((selection: unknown, type: number, source: object) => void) | null = null;
  let types = ["text/plain"],
    content = new TextEncoder().encode("hello"),
    deferred = false;
  const callbacks: (() => void)[] = [];
  const bytes = (data: Uint8Array) => ({
    get_size: () => data.length,
    get_data: () => data,
  });
  const selection = {
    connect: (_name: string, callback: typeof changed) => {
      changed = callback;
      return 1;
    },
    disconnect: vi.fn(),
  };
  const external = (mime: string, data: Uint8Array) => {
    types = [mime];
    content = data;
    changed?.(selection, 1, {});
  };
  const clipboard = {
    get_mimetypes: () => types,
    get_content: vi.fn(
      (
        _type: number,
        _mime: string,
        callback: (c: unknown, b: unknown) => void,
      ) => {
        const value = bytes(content);
        const call = () => callback(clipboard, value);
        if (deferred) callbacks.push(call);
        else call();
      },
    ),
    set_text: (_type: number, text: string) =>
      external("text/plain", new TextEncoder().encode(text)),
    set_content: (
      _type: number,
      mime: string,
      data: ReturnType<typeof bytes>,
    ) => external(mime, data.get_data()),
  };
  const exported = { export: vi.fn(), unexport: vi.fn(), emit_signal: vi.fn() };
  const Gio = {
    BusType: { SESSION: 1 },
    BusNameWatcherFlags: { NONE: 0 },
    BusNameOwnerFlags: { NONE: 0 },
    DBus: { session: {} },
    DBusExportedObject: { wrapJSObject: () => exported },
    bus_watch_name: (
      _type: unknown,
      _name: unknown,
      _flags: unknown,
      callback: Function,
    ) => {
      callback(null, null, ":1.42");
      return 1;
    },
    bus_own_name: () => 1,
    bus_unwatch_name: vi.fn(),
    bus_unown_name: vi.fn(),
    File: {
      new_for_uri: (uri: string) => ({
        get_path: () => decodeURI(uri.replace("file://", "")),
      }),
      new_for_path: (path: string) => ({
        get_uri: () => `file://${encodeURI(path)}`,
      }),
    },
  };
  const GLib = {
    Variant: class {
      constructor(
        public type: string,
        public value: unknown,
      ) {}
    },
    Bytes: class {
      constructor(private data: Uint8Array) {}
      get_size() {
        return this.data.length;
      }
      get_data() {
        return this.data;
      }
    },
    base64_encode: (b: Uint8Array) => Buffer.from(b).toString("base64"),
    base64_decode: (s: string) => new Uint8Array(Buffer.from(s, "base64")),
  };
  const Extension = class {
    getSettings() {
      return { set_strv: vi.fn() };
    }
  };
  const context: any = {
    Gio,
    GLib,
    St: {
      Icon: class {},
      Clipboard: { get_default: () => clipboard },
      ClipboardType: { CLIPBOARD: 1 },
    },
    Meta: {
      SelectionType: { SELECTION_CLIPBOARD: 1 },
      KeyBindingFlags: { NONE: 0 },
    },
    Shell: {
      ActionMode: { NORMAL: 1 },
      WindowTracker: { get_default: () => ({ get_window_app: () => null }) },
    },
    Main: {
      wm: { addKeybinding: vi.fn(), removeKeybinding: vi.fn() },
      panel: { addToStatusArea: vi.fn() },
    },
    Clutter: {
      EVENT_STOP: true,
      EVENT_PROPAGATE: false,
      KEY_Return: 13,
      KEY_space: 32,
    },
    PanelMenu: {
      Button: class {
        add_child() {}
        connect() {}
        destroy() {}
      },
    },
    Extension,
    TextEncoder,
    TextDecoder,
    global: { display: { get_selection: () => selection, focus_window: null } },
  };
  const source = readFileSync(
    new URL("../extensions/clipvault/extension.js", import.meta.url),
    "utf8",
  )
    .replace(/^import .*;\s*$/gm, "")
    .replace("export default class ClipVaultBridge", "class ClipVaultBridge");
  runInNewContext(`${source}\nglobalThis.Bridge = ClipVaultBridge;`, context);
  const instance = new context.Bridge();
  instance.enable();
  function invoke(method: string, args: unknown[], sender = ":1.42") {
    const call = {
      get_sender: () => sender,
      return_value: vi.fn(),
      return_dbus_error: vi.fn(),
    };
    instance[method](args, call);
    return call;
  }
  return {
    instance,
    exported,
    invoke,
    external,
    clipboard,
    callbacks,
    setDeferred: () => {
      deferred = true;
    },
    text: (s: string) => new TextEncoder().encode(s),
  };
}
describe("GNOME clipboard bridge", () => {
  it("opens the quick picker through the panel and shortcut signal", () => {
    const b = bridge();
    b.instance._toggle();
    expect(b.exported.emit_signal).toHaveBeenCalledWith(
      "ToggleRequested",
      null,
    );
    b.instance.disable();
  });
  it("refuses clipboard access from an unauthorized bus peer", () => {
    const b = bridge();
    const call = b.invoke("ReadAsync", [""], ":1.99");
    expect(call.return_dbus_error).toHaveBeenCalled();
    expect(b.clipboard.get_content).not.toHaveBeenCalled();
    b.instance.disable();
  });
  it("filters sensitive MIME types before reading their bytes", () => {
    const b = bridge();
    b.external("x-kde-passwordManagerHint", b.text("secret"));
    const call = b.invoke("ReadAsync", [""]);
    const snapshot = JSON.parse(call.return_value.mock.calls[0][0].value[0]);
    expect(snapshot.clip.sensitive).toBe(true);
    expect(snapshot.clip.text).toBe("");
    expect(b.clipboard.get_content).not.toHaveBeenCalled();
    b.instance.disable();
  });
  it("marks own writes, then captures the next external copy without a loop", () => {
    const b = bridge();
    b.invoke("WriteAsync", [
      JSON.stringify({ kind: "text", text: "remote copy" }),
    ]);
    const own = b.invoke("ReadAsync", [""]);
    expect(JSON.parse(own.return_value.mock.calls[0][0].value[0]).owned).toBe(
      true,
    );
    b.external("text/plain", b.text("local copy"));
    const call = b.invoke("ReadAsync", [""]);
    const result = JSON.parse(call.return_value.mock.calls[0][0].value[0]);
    expect(result.owned).toBe(false);
    expect(result.clip.text).toBe("local copy");
    b.instance.disable();
  });
  it("discards an asynchronous read if the clipboard changes before completion", () => {
    const b = bridge();
    b.setDeferred();
    const call = b.invoke("ReadAsync", [""]);
    b.external("text/plain", b.text("new selection"));
    b.callbacks[0]();
    expect(call.return_value.mock.calls[0][0].value).toEqual([""]);
    b.instance.disable();
  });
});
