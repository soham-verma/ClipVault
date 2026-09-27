import { invoke, convertFileSrc } from "@tauri-apps/api/core";
export type Kind = "text" | "url" | "image" | "rtf" | "fileList";
export interface Item {
  id: number;
  kind: Kind;
  preview_text: string;
  full_text: string | null;
  source_name: string | null;
  source_app: string | null;
  created_at: number;
  pinned: boolean;
  pixel_width: number;
  pixel_height: number;
  byte_size: number;
  origin_device: string | null;
}
export interface Page {
  items: Item[];
  total: number;
  has_more: boolean;
}
export interface Settings {
  max_history_count: number;
  max_disk_megabytes: number;
  launch_at_login: boolean;
  ignore_password_managers: boolean;
  paste_on_select: boolean;
  excluded_apps: string[];
  shortcut: string;
  device_name: string;
}
export interface Capabilities {
  backend: string;
  monitoring: boolean;
  images: boolean;
  app_exclusions: boolean;
  automatic_paste: boolean;
  helper_required: boolean;
  message: string | null;
}
export interface Pairing {
  id: string;
  device_name: string;
  verification: string;
}
export interface Status {
  phase: string;
  role: "host" | "guest" | null;
  address: string | null;
  join_code: string | null;
  code_expires_at: number | null;
  paused: boolean;
  room_paused: boolean;
  peers: { id: string; name: string; paused: boolean }[];
  pairings: Pairing[];
  error: string | null;
}
export interface Host {
  id: string;
  name: string;
  address: string;
  code: string;
}
export interface Trusted {
  id: string;
  name: string;
  public_key: string;
}
export interface Snapshot {
  settings: Settings;
  capabilities: Capabilities;
  status: Status;
  trusted: Trusted[];
  nearby: Host[];
  interfaces: [string, string][];
}
export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  history: (query: string, kind: Kind | null, offset = 0) =>
    invoke<Page>("history", { query, kind, offset }),
  copy: (id: number) => invoke<void>("copy_item", { id }),
  pin: (id: number, pinned: boolean) =>
    invoke<void>("pin_item", { id, pinned }),
  delete: (id: number) => invoke<void>("delete_item", { id }),
  clear: (keepPinned: boolean) => invoke<void>("clear_history", { keepPinned }),
  image: async (id: number) =>
    convertFileSrc(await invoke<string>("image_path", { id })),
  settings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  host: (bindAddress: string | null) =>
    invoke<void>("host_server", { bindAddress }),
  join: (code: string, address: string | null) =>
    invoke<void>("join_server", { code, address }),
  confirm: (id: string, accepted: boolean) =>
    invoke<void>("confirm_pairing", { id, accepted }),
  pause: (paused: boolean) => invoke<void>("pause_sharing", { paused }),
  disconnect: () => invoke<void>("disconnect"),
  forget: (id: string) => invoke<void>("forget_device", { id }),
  newCode: () => invoke<void>("new_join_code"),
  retry: () => invoke<void>("retry_clipboard"),
  helper: () => invoke<string>("install_gnome_helper"),
  quit: () => invoke<void>("quit"),
  workspace: (view: "history" | "sharing" | "settings") =>
    invoke<void>("open_workspace", { view }),
  workspaceView: () =>
    invoke<"history" | "sharing" | "settings">("workspace_view"),
};
export function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1048576) return `${(value / 1024).toFixed(0)} KB`;
  return `${(value / 1048576).toFixed(1)} MB`;
}
export function connectionLabel(status: Status) {
  if (status.paused || status.room_paused) return "Sharing paused";
  return (
    (
      {
        hosting: "Hosting a server",
        connected: "Connected",
        connecting: "Connecting…",
        reconnecting: "Reconnecting…",
        offline: "Local only",
      } as Record<string, string>
    )[status.phase] ?? status.phase
  );
}
