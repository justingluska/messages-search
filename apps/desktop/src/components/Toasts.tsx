import { dismissToast, useToast } from "../lib/toast";
import { Icon } from "./Icon";

export function Toasts() {
  const t = useToast();
  if (!t) return null;
  return (
    <div className={`toast${t.tone === "error" ? " is-error" : ""}`} role="status" aria-live="polite" key={t.id}>
      <span className="toast-text">{t.text}</span>
      {t.action && (
        <button
          className="text-btn"
          onClick={() => {
            t.action!.run();
            dismissToast();
          }}
        >
          {t.action.label}
        </button>
      )}
      <button className="toast-close" onClick={dismissToast} aria-label="Dismiss">
        <Icon name="x" size={12} />
      </button>
    </div>
  );
}
