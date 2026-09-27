// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QuickPicker } from "./QuickPicker";
import { ShortcutInput } from "./ShortcutInput";

const mocked = vi.hoisted(() => ({
  copy: vi.fn(),
  history: vi.fn(),
  snapshot: vi.fn(),
  workspace: vi.fn(),
  clear: vi.fn(),
  quit: vi.fn(),
  hide: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: mocked.hide }),
}));
vi.mock("./api", async (original) => ({
  ...(await original<typeof import("./api")>()),
  api: { ...mocked, image: vi.fn() },
}));
beforeEach(() => {
  vi.clearAllMocks();
  Element.prototype.scrollIntoView = vi.fn();
  mocked.copy.mockResolvedValue(undefined);
  mocked.workspace.mockResolvedValue(undefined);
  mocked.clear.mockResolvedValue(undefined);
  mocked.snapshot.mockResolvedValue({
    capabilities: { monitoring: true },
    status: { phase: "offline", peers: [], pairings: [], paused: false },
  });
  mocked.history.mockResolvedValue({
    items: [1, 2].map((id) => ({
      id,
      kind: "text",
      preview_text: `Copy ${id}`,
      created_at: 1,
      source_name: "Editor",
      pinned: false,
    })),
    total: 2,
    has_more: false,
  });
});
afterEach(cleanup);
it("uses the original compact behavior: select once, copy on double-click", async () => {
  render(<QuickPicker />);
  const row = await screen.findByText("Copy 2");
  fireEvent.click(row);
  expect(mocked.copy).not.toHaveBeenCalled();
  fireEvent.doubleClick(row);
  await waitFor(() => expect(mocked.copy).toHaveBeenCalledWith(2));
  expect(mocked.workspace).not.toHaveBeenCalled();
});
it("keeps digits usable in search and supports arrows, Enter, and Escape", async () => {
  render(<QuickPicker />);
  await screen.findByText("Copy 1");
  const input = screen.getByLabelText("Search clipboard");
  fireEvent.keyDown(input, { key: "2" });
  expect(mocked.copy).not.toHaveBeenCalled();
  fireEvent.keyDown(input, { key: "ArrowDown" });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(mocked.copy).toHaveBeenCalledWith(2));
  fireEvent.keyDown(input, { key: "Escape" });
  expect(mocked.hide).toHaveBeenCalledOnce();
});
it("provides separate sharing and preferences entry points", async () => {
  render(<QuickPicker />);
  fireEvent.click(screen.getByRole("button", { name: "Device sharing" }));
  await waitFor(() => expect(mocked.workspace).toHaveBeenCalledWith("sharing"));
  fireEvent.click(screen.getByRole("button", { name: "Preferences" }));
  await waitFor(() =>
    expect(mocked.workspace).toHaveBeenCalledWith("settings"),
  );
});
it("keeps the original footer actions", async () => {
  render(<QuickPicker />);
  await screen.findByText("2 items");
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  await waitFor(() => expect(mocked.clear).toHaveBeenCalledWith(true));
  fireEvent.click(screen.getByRole("button", { name: "Quit ClipVault" }));
  expect(mocked.quit).toHaveBeenCalledOnce();
});
it("records user shortcuts and allows cancelling without changing the binding", () => {
  const onChange = vi.fn();
  render(<ShortcutInput value="Super+Shift+V" onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Record" }));
  fireEvent.keyDown(screen.getByRole("button", { name: "Press keys…" }), {
    key: "k",
    code: "KeyK",
    ctrlKey: true,
    altKey: true,
  });
  expect(onChange).toHaveBeenCalledWith("Control+Alt+K");
  onChange.mockClear();
  fireEvent.click(screen.getByRole("button", { name: "Record" }));
  fireEvent.keyDown(screen.getByRole("button", { name: "Press keys…" }), {
    key: "Escape",
    code: "Escape",
  });
  expect(onChange).not.toHaveBeenCalled();
});
