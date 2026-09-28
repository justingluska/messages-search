import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Icon } from "./Icon";

export interface MenuItem {
  id: string;
  label: string;
  detail?: string;
  checked?: boolean;
  /** Draw a separator above this item. */
  divider?: boolean;
}

/**
 * A macOS-style pop-up menu anchored under its trigger. Keyboard: type to
 * filter (when `searchable`), ↑/↓ to move, Enter to pick, Esc to close.
 */
export function Menu({
  items,
  onPick,
  onClose,
  searchable,
  placeholder,
  keepOpen,
}: {
  items: MenuItem[];
  onPick: (item: MenuItem) => void;
  onClose: () => void;
  searchable?: boolean;
  placeholder?: string;
  /** Multi-select menus stay open after a pick. */
  keepOpen?: boolean;
}) {
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);

  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase();
    return f ? items.filter((i) => i.label.toLowerCase().includes(f) || i.detail?.toLowerCase().includes(f)) : items;
  }, [items, filter]);

  useEffect(() => setActive(0), [filter]);
  useEffect(() => {
    (searchable ? input.current : root.current)?.focus();
  }, [searchable]);
  useLayoutEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) onClose();
    };
    // Esc closes the menu wherever focus is: a pick in a multi-select menu
    // hands focus back to the search field, which would otherwise take the Esc.
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      onClose();
    };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [onClose]);

  const pick = (item: MenuItem | undefined) => {
    if (!item) return;
    onPick(item);
    if (!keepOpen) onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, shown.length - 1));
    else if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
    else if (e.key === "Enter") pick(shown[active]);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  return (
    <div className="menu" ref={root} tabIndex={-1} onKeyDown={onKeyDown} role="menu">
      {searchable && (
        <div className="menu-search">
          <Icon name="search" size={14} />
          <input
            ref={input}
            className="menu-filter"
            value={filter}
            placeholder={placeholder ?? "Filter"}
            onChange={(e) => setFilter(e.target.value)}
            spellCheck={false}
            autoCorrect="off"
          />
        </div>
      )}
      <div className="menu-list" ref={list}>
        {shown.length === 0 && <div className="menu-empty">No matches</div>}
        {shown.map((item, i) => (
          <div key={item.id}>
            {item.divider && !filter && <div className="menu-sep" />}
            <div
              data-i={i}
              role="menuitemcheckbox"
              aria-checked={!!item.checked}
              className={`menu-item${i === active ? " is-active" : ""}${item.checked ? " is-checked" : ""}`}
              onMouseMove={() => i !== active && setActive(i)}
              onClick={() => pick(item)}
            >
              <span className="menu-main">
                <span className="menu-label">{item.label}</span>
                {item.detail && <span className="menu-detail">{item.detail}</span>}
              </span>
              {item.checked && (
                <span className="menu-check">
                  <Icon name="check" size={14} />
                </span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
