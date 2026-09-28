// The attachment viewer's state: which items (a conversation's photos and
// videos, or the Storage page's rows) and which one is showing.

import { useSyncExternalStore } from "react";
import type { AttachmentKind } from "./types";

export interface LightboxItem {
  id: number;
  filename: string | null;
  mime: string | null;
  path: string | null;
  bytes: number;
  kind: AttachmentKind;
  sender: string | null;
  fromMe: boolean;
  dateMs: number;
}

export interface LightboxState {
  items: LightboxItem[];
  index: number;
}

let state: LightboxState | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

/** Kinds the viewer shows (others open or reveal instead). */
export const viewable = (k: AttachmentKind) => k === "image" || k === "video" || k === "sticker";

export function openLightbox(items: LightboxItem[], id: number): void {
  const index = items.findIndex((i) => i.id === id);
  if (index < 0) return;
  state = { items, index };
  emit();
}

export function stepLightbox(delta: number): void {
  if (!state) return;
  const index = Math.max(0, Math.min(state.items.length - 1, state.index + delta));
  if (index === state.index) return;
  state = { ...state, index };
  emit();
}

export function closeLightbox(): void {
  state = null;
  emit();
}

export function useLightbox(): LightboxState | null {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => state,
  );
}
