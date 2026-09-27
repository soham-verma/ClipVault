import { useEffect, useState } from "react";
import {
  Copy,
  Pin,
  Trash2,
  Type,
  Link,
  Image as ImageIcon,
  File,
} from "lucide-react";
import { api, formatBytes, type Item } from "./api";
export function ClipRow({
  item,
  index,
  selected,
  onSelect,
  onCopy,
  onPin,
  onDelete,
}: {
  item: Item;
  index: number;
  selected: boolean;
  onSelect: () => void;
  onCopy: () => void;
  onPin: () => void;
  onDelete: () => void;
}) {
  const [image, setImage] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    if (item.kind === "image")
      void api
        .image(item.id)
        .then((url) => {
          if (active) setImage(url);
        })
        .catch(() => {});
    return () => {
      active = false;
    };
  }, [item.id, item.kind]);
  const Icon = {
    text: Type,
    url: Link,
    image: ImageIcon,
    rtf: Type,
    fileList: File,
  }[item.kind];
  return (
    <div
      role="option"
      id={`clip-${item.id}`}
      aria-selected={selected}
      className={`clip-row ${selected ? "row-selected" : ""}`}
      onClick={onSelect}
      onDoubleClick={onCopy}
    >
      {index < 9 ? (
        <kbd className="row-key">{index + 1}</kbd>
      ) : (
        <span className="row-key-spacer" />
      )}
      <span className={`clip-symbol ${item.kind}`}>
        {image ? (
          <img src={image} alt="Clipboard image preview" />
        ) : (
          <Icon size={20} />
        )}
      </span>
      <div className="clip-text">
        <div className="clip-preview">
          {item.preview_text || "Untitled item"}
        </div>
        <div className="clip-meta">
          {item.pinned && <Pin size={11} />}
          <span>
            {item.origin_device ? "From " : ""}
            {item.source_name || "Clipboard"}
          </span>
          <span>·</span>
          <time dateTime={new Date(item.created_at * 1000).toISOString()}>
            {new Date(item.created_at * 1000).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
          {item.kind === "image" && (
            <>
              <span>·</span>
              <span>{formatBytes(item.byte_size)}</span>
            </>
          )}
        </div>
      </div>
      <div
        className="row-actions"
        onClick={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
      >
        <button title="Copy item" aria-label="Copy item" onClick={onCopy}>
          <Copy size={16} />
        </button>
        <button
          title={item.pinned ? "Unpin item" : "Pin item"}
          aria-label={item.pinned ? "Unpin item" : "Pin item"}
          className={item.pinned ? "pinned" : ""}
          onClick={onPin}
        >
          <Pin size={15} />
        </button>
        <button title="Delete item" aria-label="Delete item" onClick={onDelete}>
          <Trash2 size={15} />
        </button>
      </div>
    </div>
  );
}
