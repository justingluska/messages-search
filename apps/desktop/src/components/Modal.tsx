import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

/**
 * Resend-style modal: a 24px-radius panel on the page background with an
 * interactive hairline, over a dimmed backdrop. Esc or the backdrop closes it.
 */
export function Modal({
  title,
  onClose,
  children,
  width = 640,
}: {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  width?: number;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);
  const body = useRef<HTMLDivElement>(null);

  useEffect(() => {
    opener.current = document.activeElement;
    // Always open at the top: focusing without preventScroll can scroll the
    // body when the window is short.
    panel.current?.focus({ preventScroll: true });
    body.current?.scrollTo({ top: 0 });
    return () => {
      if (opener.current instanceof HTMLElement) opener.current.focus({ preventScroll: true });
    };
  }, []);

  return createPortal(
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="modal"
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{ maxWidth: width }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          }
        }}
      >
        <header className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <Icon name="x" size={16} />
          </button>
        </header>
        <div className="modal-body v-scroll" ref={body}>{children}</div>
      </div>
    </div>,
    document.body,
  );
}

/** A modal section: a 14px/600 heading over ruled rows or text. */
export function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="modal-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

/** A ruled label/value row. */
export function Row({ label, children, detail }: { label: React.ReactNode; children?: React.ReactNode; detail?: React.ReactNode }) {
  return (
    <div className="ruled-row">
      <div className="ruled-label">
        <span>{label}</span>
        {detail && <span className="ruled-detail">{detail}</span>}
      </div>
      {children !== undefined && <div className="ruled-value">{children}</div>}
    </div>
  );
}

/** Keycap. */
export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}
