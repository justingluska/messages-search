import { useCallback, useMemo, useState } from "react";
import { Icon } from "../../components/Icon";
import { Menu, type MenuItem } from "../../components/Menu";
import { hasFilterOf, opsIn, quoteValue, setGroup, type FilterGroup } from "../../lib/query";
import type { ChatSummary, HasFilter, Stats } from "../../lib/types";

/**
 * Person | Conversation | Date | Has, as one grouped pill (Rift's "Round trip |
 * Passengers | Economy"). Each segment rewrites the matching operators in the
 * query string; the string stays the source of truth, so typing `from:maya`
 * by hand lights up "Person" too. A pick runs the search.
 */
export function FilterPill({
  query,
  onQuery,
  chats,
  onNeedChats,
  stats,
}: {
  query: string;
  onQuery: (q: string) => void;
  chats: ChatSummary[];
  /** Asks the parent to load the chat list (lazily, when a menu that needs it opens). */
  onNeedChats: () => void;
  stats: Stats | null;
}) {
  const [open, setOpen] = useState<FilterGroup | null>(null);
  const close = useCallback(() => setOpen(null), []);

  const person = opsIn(query, "person");
  const chat = opsIn(query, "chat");
  const date = opsIn(query, "date");
  const has = opsIn(query, "has");

  const people = useMemo(() => {
    const seen = new Map<string, { label: string; detail?: string }>();
    for (const c of chats)
      for (const p of c.participants) {
        const label = p.name ?? p.address;
        if (!seen.has(label)) seen.set(label, { label, detail: p.name ? p.address : undefined });
      }
    // Named people first, then bare numbers and addresses.
    const bare = (s: string) => /^[+\d]/.test(s) || s.includes("@");
    return [...seen.values()].sort((a, b) => Number(bare(a.label)) - Number(bare(b.label)) || a.label.localeCompare(b.label));
  }, [chats]);

  const personValue = person[0]?.value;
  const chatValue = chat[0]?.value;
  const hasValues = has.map((t) => hasFilterOf(t.value)).filter((x): x is HasFilter => !!x);

  const personItems: MenuItem[] = [
    { id: "me", label: "Me", checked: personValue?.toLowerCase() === "me" },
    ...people.map((p, i) => ({ id: `p:${p.label}`, label: p.label, detail: p.detail, checked: personValue === p.label, divider: i === 0 })),
  ];
  const chatItems: MenuItem[] = chats.map((c) => ({
    id: `c:${c.id}`,
    label: c.title,
    detail: c.isGroup ? `${c.participants.length} people` : undefined,
    checked: chatValue === c.title,
  }));
  const dateItems = useMemo(() => datePresets(stats), [stats]);
  const dateLabel = date.length ? labelForDate(date.map((t) => `${t.key}:${t.value}`), dateItems) : null;
  const hasItems: MenuItem[] = HAS_OPTIONS.map((o) => ({ id: o.value, label: o.label, checked: hasValues.includes(o.value) }));

  return (
    <div className="filter-pill" role="toolbar" aria-label="Filters">
      <Filter
        label="Person"
        value={personValue ? (personValue.toLowerCase() === "me" ? "Me" : personValue) : null}
        open={open === "person"}
        onOpen={() => {
          onNeedChats();
          setOpen(open === "person" ? null : "person");
        }}
        onClear={() => onQuery(setGroup(query, "person", []))}
      >
        <Menu
          searchable
          placeholder="Find a person"
          items={personItems}
          onClose={close}
          onPick={(item) =>
            onQuery(setGroup(query, "person", item.checked ? [] : [`from:${quoteValue(item.id === "me" ? "me" : item.label)}`]))
          }
        />
      </Filter>
      <Filter
        label="Conversation"
        value={chatValue ?? null}
        open={open === "chat"}
        onOpen={() => {
          onNeedChats();
          setOpen(open === "chat" ? null : "chat");
        }}
        onClear={() => onQuery(setGroup(query, "chat", []))}
      >
        <Menu
          searchable
          placeholder="Find a conversation"
          items={chatItems}
          onClose={close}
          onPick={(item) => onQuery(setGroup(query, "chat", item.checked ? [] : [`in:${quoteValue(item.label)}`]))}
        />
      </Filter>
      <Filter
        label="Date"
        value={dateLabel}
        open={open === "date"}
        onOpen={() => setOpen(open === "date" ? null : "date")}
        onClear={() => onQuery(setGroup(query, "date", []))}
      >
        <Menu
          items={dateItems.map((d) => ({ ...d, checked: d.label === dateLabel }))}
          onClose={close}
          onPick={(item) => onQuery(setGroup(query, "date", item.checked ? [] : (item as DateItem).ops))}
        />
      </Filter>
      <Filter
        label="Has"
        value={hasValues.length ? hasValues.map((h) => HAS_OPTIONS.find((o) => o.value === h)!.label).join(", ") : null}
        open={open === "has"}
        onOpen={() => setOpen(open === "has" ? null : "has")}
        onClear={() => onQuery(setGroup(query, "has", []))}
      >
        <Menu
          keepOpen
          items={hasItems}
          onClose={close}
          onPick={(item) => {
            const v = item.id as HasFilter;
            const next = hasValues.includes(v) ? hasValues.filter((h) => h !== v) : [...hasValues, v];
            onQuery(setGroup(query, "has", next.map((h) => `has:${h}`)));
          }}
        />
      </Filter>
    </div>
  );
}

function Filter({
  label,
  value,
  open,
  onOpen,
  onClear,
  children,
}: {
  label: string;
  value: string | null;
  open: boolean;
  onOpen: () => void;
  onClear: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="filter">
      <div className={`filter-ctl${value ? " is-set" : ""}${open ? " is-open" : ""}`}>
        <button
          className="filter-btn"
          onMouseDown={(e) => e.preventDefault()}
          onClick={onOpen}
          aria-haspopup="menu"
          aria-expanded={open}
          title={value ? `${label}: ${value}` : label}
        >
          <span className="filter-text">{value ?? label}</span>
          {!value && <Icon name="chevron" size={14} className="filter-chevron" />}
        </button>
        {value && (
          <button className="filter-clear" aria-label={`Clear ${label}`} onMouseDown={(e) => e.preventDefault()} onClick={onClear}>
            <Icon name="x" size={12} />
          </button>
        )}
      </div>
      {open && <div className="filter-pop">{children}</div>}
    </div>
  );
}

const HAS_OPTIONS: { value: HasFilter; label: string }[] = [
  { value: "link", label: "Links" },
  { value: "photo", label: "Photos" },
  { value: "video", label: "Videos" },
  { value: "audio", label: "Audio" },
  { value: "file", label: "Files" },
  { value: "attachment", label: "Any attachment" },
];

interface DateItem extends MenuItem {
  ops: string[];
}

function ymd(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function datePresets(stats: Stats | null): DateItem[] {
  const now = new Date();
  const daysAgo = (n: number) => {
    const d = new Date(now);
    d.setDate(d.getDate() - n);
    return d;
  };
  // `after:` is exclusive of the day given, so "past 7 days" starts the day before.
  const items: DateItem[] = [
    { id: "7d", label: "Past week", ops: [`after:${ymd(daysAgo(8))}`] },
    { id: "30d", label: "Past month", ops: [`after:${ymd(daysAgo(31))}`] },
    { id: "1y", label: "Past year", ops: [`after:${ymd(daysAgo(366))}`] },
  ];
  const newest = now.getFullYear();
  const oldest = stats?.oldestMs ? new Date(stats.oldestMs).getFullYear() : newest - 3;
  for (let y = newest; y >= oldest; y--) items.push({ id: `y${y}`, label: String(y), ops: [`during:${y}`], divider: y === newest });
  return items;
}

function labelForDate(ops: string[], presets: DateItem[]): string {
  const key = ops.join(" ");
  const preset = presets.find((p) => p.ops.join(" ") === key);
  if (preset) return preset.label;
  return ops
    .map((o) => {
      const [k, v] = [o.slice(0, o.indexOf(":")), o.slice(o.indexOf(":") + 1)];
      return k === "during" || k === "on" || k === "in_year" ? v : `${k[0].toUpperCase()}${k.slice(1)} ${v}`;
    })
    .join(", ");
}
