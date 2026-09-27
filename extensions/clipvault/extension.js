import Gio from "gi://Gio";
import GLib from "gi://GLib";
import St from "gi://St";
import Meta from "gi://Meta";
import Shell from "gi://Shell";
import Clutter from "gi://Clutter";
import { Extension } from "resource:///org/gnome/shell/extensions/extension.js";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import * as PanelMenu from "resource:///org/gnome/shell/ui/panelMenu.js";

const IFACE = `<node><interface name="org.gnome.Shell.Extensions.ClipVault1">
<method name="Version"><arg direction="out" type="u"/></method>
<method name="Read"><arg direction="in" type="s"/><arg direction="out" type="s"/></method>
<method name="Write"><arg direction="in" type="s"/></method>
<method name="SetShortcut"><arg direction="in" type="s"/></method>
<signal name="ToggleRequested"/>
</interface></node>`;
const TYPE = St.ClipboardType.CLIPBOARD;
const MAX_TEXT = 1048576,
  MAX_IMAGE = 20 * MAX_TEXT;

export default class ClipVaultBridge extends Extension {
  enable() {
    this._generation = 1;
    this._owner = null;
    this._ownedSource = null;
    this._currentSource = null;
    this._clipboard = St.Clipboard.get_default();
    this._selection = global.display.get_selection();
    this._changed = this._selection.connect(
      "owner-changed",
      (_selection, type, source) => {
        if (type === Meta.SelectionType.SELECTION_CLIPBOARD) {
          this._currentSource = source;
          this._generation++;
        }
      },
    );
    this._watch = Gio.bus_watch_name(
      Gio.BusType.SESSION,
      "com.soham.ClipVault",
      Gio.BusNameWatcherFlags.NONE,
      (_connection, _name, owner) => {
        this._owner = owner;
        if (this._indicator) this._indicator.visible = true;
      },
      () => {
        this._owner = null;
        if (this._indicator) this._indicator.visible = false;
      },
    );
    this._object = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
    this._object.export(
      Gio.DBus.session,
      "/org/gnome/Shell/Extensions/ClipVault",
    );
    this._bus = Gio.bus_own_name(
      Gio.BusType.SESSION,
      "org.gnome.Shell.Extensions.ClipVault",
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
      null,
    );
    this._settings = this.getSettings();
    this._indicator = new PanelMenu.Button(0, "ClipVault", true);
    this._indicator.add_child(
      new St.Icon({
        icon_name: "edit-paste-symbolic",
        style_class: "system-status-icon",
      }),
    );
    this._indicator.connect("button-press-event", () => {
      this._toggle();
      return Clutter.EVENT_STOP;
    });
    this._indicator.connect("key-press-event", (_actor, event) => {
      if (
        [Clutter.KEY_Return, Clutter.KEY_space].includes(event.get_key_symbol())
      ) {
        this._toggle();
        return Clutter.EVENT_STOP;
      }
      return Clutter.EVENT_PROPAGATE;
    });
    Main.panel.addToStatusArea(this.uuid, this._indicator);
    this._indicator.visible = !!this._owner;
    Main.wm.addKeybinding(
      "toggle-clipvault",
      this._settings,
      Meta.KeyBindingFlags.NONE,
      Shell.ActionMode.NORMAL,
      () => this._toggle(),
    );
  }
  disable() {
    this._indicator?.destroy();
    this._indicator = null;
    Main.wm.removeKeybinding("toggle-clipvault");
    if (this._changed) this._selection.disconnect(this._changed);
    if (this._watch) Gio.bus_unwatch_name(this._watch);
    if (this._bus) Gio.bus_unown_name(this._bus);
    this._object?.unexport();
    this._object = null;
    this._owner = null;
    this._clipboard = null;
    this._settings = null;
  }
  _toggle() {
    if (this._owner) this._object.emit_signal("ToggleRequested", null);
  }
  Version() {
    return 1;
  }
  _authorize(invocation) {
    if (this._owner && invocation.get_sender() === this._owner) return true;
    invocation.return_dbus_error(
      "org.freedesktop.DBus.Error.AccessDenied",
      "Only the running ClipVault application may use this bridge.",
    );
    return false;
  }
  ReadAsync([previous], invocation) {
    if (!this._authorize(invocation)) return;
    const generation = `${this._generation}`;
    if (previous === generation) {
      invocation.return_value(new GLib.Variant("(s)", [""]));
      return;
    }
    const owned =
      this._ownedSource !== null && this._currentSource === this._ownedSource;
    const snapshot = { generation, owned, clip: null };
    const finish = () => {
      // An asynchronous read must never label an older payload as the new selection.
      if (!this._object || generation !== `${this._generation}`) {
        invocation.return_value(new GLib.Variant("(s)", [""]));
        return;
      }
      invocation.return_value(
        new GLib.Variant("(s)", [JSON.stringify(snapshot)]),
      );
    };
    if (owned) {
      finish();
      return;
    }
    const types = this._clipboard.get_mimetypes(TYPE) ?? [];
    const app = global.display.focus_window
      ? Shell.WindowTracker.get_default().get_window_app(
          global.display.focus_window,
        )
      : null;
    const clip = {
      kind: "text",
      text: "",
      data: "",
      source_app: app?.get_id() ?? null,
      source_name: app?.get_name() ?? null,
      sensitive: false,
    };
    if (
      types.some((t) =>
        [
          "x-kde-passwordManagerHint",
          "application/x-keepassxc-secret",
          "org.nspasteboard.ConcealedType",
          "org.nspasteboard.TransientType",
          "org.nspasteboard.AutoGeneratedType",
        ].includes(t),
      )
    ) {
      clip.sensitive = true;
      snapshot.clip = clip;
      finish();
      return;
    }
    const mime = [
      "text/uri-list",
      "image/png",
      "text/plain;charset=utf-8",
      "UTF8_STRING",
      "text/plain",
    ].find((t) => types.includes(t));
    if (!mime) {
      finish();
      return;
    }
    this._clipboard.get_content(TYPE, mime, (_clipboard, bytes) => {
      if (!bytes) {
        finish();
        return;
      }
      const limit = mime === "image/png" ? MAX_IMAGE : MAX_TEXT;
      if (bytes.get_size() > limit) {
        snapshot.error = "Clipboard content exceeds the size limit";
        finish();
        return;
      }
      const data = bytes.get_data();
      if (mime === "image/png") {
        clip.kind = "image";
        clip.text = null;
        clip.data = GLib.base64_encode(data);
      } else {
        try {
          clip.text = new TextDecoder("utf-8", { fatal: true }).decode(data);
        } catch (_) {
          snapshot.error = "Clipboard text is not UTF-8";
          finish();
          return;
        }
        if (mime === "text/uri-list") {
          const paths = clip.text
            .split(/\r?\n/)
            .filter((s) => s.startsWith("file:"))
            .map((s) => Gio.File.new_for_uri(s).get_path())
            .filter(Boolean);
          if (!paths.length) {
            finish();
            return;
          }
          clip.kind = "fileList";
          clip.text = paths.join("\n");
        } else if (/^(https?:\/\/|mailto:)\S+$/.test(clip.text))
          clip.kind = "url";
      }
      snapshot.clip = clip;
      finish();
    });
  }
  WriteAsync([json], invocation) {
    if (!this._authorize(invocation)) return;
    try {
      if (json.length > 30 * MAX_TEXT)
        throw new Error("Clipboard content too large");
      const clip = JSON.parse(json);
      if (clip.kind === "image") {
        const bytes = GLib.base64_decode(clip.data);
        if (bytes.length > MAX_IMAGE) throw new Error("Image too large");
        this._clipboard.set_content(TYPE, "image/png", new GLib.Bytes(bytes));
      } else if (clip.kind === "fileList") {
        const uris = clip.text
          .split("\n")
          .filter(Boolean)
          .map((p) => Gio.File.new_for_path(p).get_uri())
          .join("\r\n");
        this._clipboard.set_content(
          TYPE,
          "text/uri-list",
          new GLib.Bytes(new TextEncoder().encode(uris)),
        );
      } else {
        if (
          !["text", "url", "rtf"].includes(clip.kind) ||
          typeof clip.text !== "string" ||
          new TextEncoder().encode(clip.text).length > MAX_TEXT
        )
          throw new Error("Invalid text");
        this._clipboard.set_text(TYPE, clip.text);
      }
      this._ownedSource = this._currentSource;
      this._generation++;
      invocation.return_value(null);
    } catch (error) {
      invocation.return_dbus_error(
        "org.gnome.Shell.Extensions.ClipVault.Error",
        error.message,
      );
    }
  }
  SetShortcutAsync([shortcut], invocation) {
    if (!this._authorize(invocation)) return;
    if (shortcut.length > 128) {
      invocation.return_dbus_error(
        "org.gnome.Shell.Extensions.ClipVault.Error",
        "Invalid shortcut",
      );
      return;
    }
    this._settings.set_strv("toggle-clipvault", [shortcut]);
    invocation.return_value(null);
  }
}
