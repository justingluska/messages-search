// Per-viewer UI preferences in localStorage. Every access is wrapped: storage
// can be unavailable, and the app must behave the same without it.

import { useSyncExternalStore } from "react";

export type HighlightStyle = "glow" | "focus" | "marker";

export const HIGHLIGHT_STYLES: { id: HighlightStyle; label: string; about: string }[] = [
  { id: "glow", label: "Glow", about: "The message glows briefly, then settles. Matched words are tinted." },
  { id: "focus", label: "Focus", about: "Other messages dim until you scroll or click. Matched words are tinted." },
  { id: "marker", label: "Marker", about: "A bar beside the message and a Match label. Matched words are bold." },
];

const KEY_HIGHLIGHT = "ms.highlightStyle";
const KEY_HELP_SEEN = "ms.helpSeen";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable (private mode, blocked): the choice lasts this session only.
  }
}

let highlight: HighlightStyle = (() => {
  const v = read(KEY_HIGHLIGHT);
  return HIGHLIGHT_STYLES.some((s) => s.id === v) ? (v as HighlightStyle) : "glow";
})();
const subs = new Set<() => void>();

export function setHighlightStyle(v: HighlightStyle): void {
  highlight = v;
  write(KEY_HIGHLIGHT, v);
  subs.forEach((f) => f());
}

export function useHighlightStyle(): HighlightStyle {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => highlight,
  );
}

export const helpSeen = () => read(KEY_HELP_SEEN) === "1";
export const markHelpSeen = () => write(KEY_HELP_SEEN, "1");
