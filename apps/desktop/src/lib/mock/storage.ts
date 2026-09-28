// Mock storage commands: the attachments in the generated history, filterable
// and trashable (trashed files disappear from later listings).

import type { AttachmentFilter, AttachmentKind, AttachmentPage, AttachmentRow, StorageSummary, TrashResult } from "../types";
import { CHAT_BY_ID, MESSAGES } from "./data";

const trashed = new Set<number>();

function allRows(): AttachmentRow[] {
  const out: AttachmentRow[] = [];
  for (const m of MESSAGES) {
    for (const a of m.attachments) {
      const chat = m.chatId != null ? CHAT_BY_ID.get(m.chatId) : undefined;
      out.push({
        id: a.id,
        messageId: m.id,
        chatId: m.chatId,
        chatTitle: chat?.title ?? "",
        fromMe: m.fromMe,
        sender: m.sender,
        dateMs: m.dateMs,
        filename: a.filename,
        mime: a.mime,
        path: a.path,
        bytes: a.bytes,
        kind: a.kind,
        onDisk: a.path !== null && !trashed.has(a.id),
        inTrash: trashed.has(a.id),
      });
    }
  }
  return out;
}

const KINDS: AttachmentKind[] = ["image", "video", "audio", "sticker", "file"];

export function mockStorageSummary(): StorageSummary {
  const rows = allRows();
  const byKind = KINDS.map((kind) => {
    const r = rows.filter((x) => x.kind === kind);
    return { kind, count: r.length, bytes: r.reduce((s, x) => s + x.bytes, 0) };
  });
  const chats = new Map<number, { count: number; bytes: number }>();
  for (const r of rows) {
    if (r.chatId == null) continue;
    const c = chats.get(r.chatId) ?? { count: 0, bytes: 0 };
    c.count++;
    c.bytes += r.bytes;
    chats.set(r.chatId, c);
  }
  const byChat = [...chats.entries()]
    .map(([chatId, c]) => {
      const chat = CHAT_BY_ID.get(chatId)!;
      return { chatId, title: chat.title, people: chat.participants.slice(0, 4), count: c.count, bytes: c.bytes };
    })
    .sort((a, b) => b.bytes - a.bytes)
    .slice(0, 20);
  return {
    totalBytes: rows.reduce((s, x) => s + x.bytes, 0),
    bytesOnDisk: rows.reduce((s, x) => s + (x.onDisk ? x.bytes : 0), 0),
    totalCount: rows.length,
    byKind,
    byChat,
  };
}

export function mockListAttachments(f: AttachmentFilter): AttachmentPage {
  const rows = allRows().filter(
    (r) => (f.kind === null || r.kind === f.kind) && (f.minBytes === null || r.bytes >= f.minBytes) && (f.chatId === null || r.chatId === f.chatId),
  );
  rows.sort((a, b) => (f.sort === "size" ? b.bytes - a.bytes : b.dateMs - a.dateMs));
  return {
    rows: rows.slice(f.offset, f.offset + f.limit),
    totalCount: rows.length,
    totalBytes: rows.reduce((s, x) => s + x.bytes, 0),
  };
}

export function mockTrash(ids: number[]): TrashResult {
  const rows = new Map(allRows().map((r) => [r.id, r]));
  const failed: TrashResult["failed"] = [];
  let bytes = 0;
  let n = 0;
  for (const id of ids) {
    const r = rows.get(id);
    if (!r) failed.push({ id, reason: "Not found" });
    else if (r.inTrash) failed.push({ id, reason: "Already in the Trash" });
    else if (!r.onDisk) failed.push({ id, reason: "Not downloaded on this Mac" });
    else {
      trashed.add(id);
      bytes += r.bytes;
      n++;
    }
  }
  return { trashed: n, bytes, failed };
}
