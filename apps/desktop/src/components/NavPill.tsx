export type View = "search" | "insights" | "storage";

const ITEMS: { id: View; label: string; key: string }[] = [
  { id: "search", label: "Search", key: "1" },
  { id: "insights", label: "Insights", key: "2" },
  { id: "storage", label: "Storage", key: "3" },
];

/** Search · Insights · Storage, in the Resend segment style (⌘1 to ⌘3). */
export function NavPill({ view, onView }: { view: View; onView: (v: View) => void }) {
  return (
    <nav className="nav-pill" aria-label="Sections">
      {ITEMS.map((it) => (
        <button
          key={it.id}
          className={`nav-item${view === it.id ? " is-active" : ""}`}
          aria-current={view === it.id ? "page" : undefined}
          title={`${it.label} (⌘${it.key})`}
          onClick={() => onView(it.id)}
        >
          {it.label}
        </button>
      ))}
    </nav>
  );
}
