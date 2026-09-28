import { useCallback, useState } from "react";
import { Icon } from "./Icon";
import { Menu, type MenuItem } from "./Menu";

/** Resend select trigger (32px, 12px radius) that opens a Resend menu panel. */
export function Select({
  label,
  value,
  items,
  onPick,
  searchable,
  placeholder,
}: {
  label: string;
  /** Shown instead of the label when set. */
  value: string | null;
  items: MenuItem[];
  onPick: (item: MenuItem) => void;
  searchable?: boolean;
  placeholder?: string;
}) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="select">
      <button
        className={`select-btn${value ? " is-set" : ""}${open ? " is-open" : ""}`}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <span className="select-text">{value ?? label}</span>
        <Icon name="chevron" size={14} className="select-chevron" />
      </button>
      {open && (
        <div className="filter-pop">
          <Menu items={items} onPick={onPick} onClose={close} searchable={searchable} placeholder={placeholder} />
        </div>
      )}
    </div>
  );
}
