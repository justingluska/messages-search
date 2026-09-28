// A single, static toast line at the bottom of the window ("Saved to Downloads · Show").

import { useSyncExternalStore } from "react";

export interface Toast {
  id: number;
  text: string;
  action?: { label: string; run: () => void };
  tone?: "error";
}

let current: Toast | null = null;
let seq = 0;
let timer = 0;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

export function showToast(text: string, opts: { action?: Toast["action"]; tone?: Toast["tone"]; ms?: number } = {}): void {
  current = { id: ++seq, text, action: opts.action, tone: opts.tone };
  window.clearTimeout(timer);
  timer = window.setTimeout(dismissToast, opts.ms ?? 4500);
  emit();
}

export function dismissToast(): void {
  current = null;
  emit();
}

export function useToast(): Toast | null {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => current,
  );
}
