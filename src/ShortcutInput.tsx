import { useState } from "react";
import { Keyboard } from "lucide-react";

export function ShortcutInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  return (
    <div className="shortcut-setting">
      <label htmlFor="quick-shortcut">Quick clipboard shortcut</label>
      <div className="shortcut-field">
        <input
          id="quick-shortcut"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          aria-describedby="shortcut-help"
        />
        <button
          type="button"
          className={`secondary ${recording ? "recording" : ""}`}
          onClick={() => setRecording(!recording)}
          onBlur={() => setRecording(false)}
          onKeyDown={(event) => {
            if (!recording) return;
            event.preventDefault();
            event.stopPropagation();
            if (event.key === "Escape") {
              setRecording(false);
              return;
            }
            if (["Meta", "Control", "Alt", "Shift"].includes(event.key)) return;
            const modifiers = [
              event.metaKey && "Super",
              event.ctrlKey && "Control",
              event.altKey && "Alt",
              event.shiftKey && "Shift",
            ].filter(Boolean);
            let key = event.code.replace(/^Key/, "").replace(/^Digit/, "");
            if (!key || key === "Unidentified") return;
            if (key === "Space") key = "Space";
            onChange([...modifiers, key].join("+"));
            setRecording(false);
          }}
        >
          <Keyboard size={15} />
          {recording ? "Press keys…" : "Record"}
        </button>
      </div>
      <p className="muted" id="shortcut-help">
        Opens the small clipboard picker. Record a combination or type one, then
        save. Escape cancels recording. Your desktop may reserve some shortcuts;
        Wayland may ask you to confirm the binding.
      </p>
    </div>
  );
}
