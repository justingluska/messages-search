import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { PeopleAvatar } from "../../components/Avatar";
import { Icon } from "../../components/Icon";
import { Modal } from "../../components/Modal";
import { Select } from "../../components/Select";
import { api, asCommandError, attachmentSrc } from "../../lib/api";
import { formatBytes, formatCount, listDate } from "../../lib/format";
import { openLightbox, viewable, type LightboxItem } from "../../lib/lightbox";
import { showToast } from "../../lib/toast";
import type { AttachmentFilter, AttachmentKind, AttachmentRow, SearchHit, StorageSummary } from "../../lib/types";
import { ConversationPanel } from "../conversation/ConversationPanel";

const PAGE = 200;
const ROW_H = 56;

const KIND_LABEL: Record<AttachmentKind, string> = { image: "Images", video: "Videos", audio: "Audio", sticker: "Stickers", file: "Files" };
const KIND_ICON: Record<AttachmentKind, "image" | "play" | "wave" | "doc"> = { image: "image", video: "play", audio: "wave", sticker: "image", file: "doc" };
const SIZES: { id: string; label: string; bytes: number | null }[] = [
  { id: "any", label: "Any size", bytes: null },
  { id: "1", label: "Over 1 MB", bytes: 1_000_000 },
  { id: "10", label: "Over 10 MB", bytes: 10_000_000 },
  { id: "50", label: "Over 50 MB", bytes: 50_000_000 },
  { id: "100", label: "Over 100 MB", bytes: 100_000_000 },
];

type Filter = Omit<AttachmentFilter, "limit" | "offset">;

/** Attachments by size or date, with a Trash for the big ones. */
export function StoragePage() {
  const [summary, setSummary] = useState<StorageSummary | null>(null);
  const [filter, setFilter] = useState<Filter>({ kind: null, minBytes: null, chatId: null, sort: "size" });
  const [rows, setRows] = useState<AttachmentRow[]>([]);
  const [total, setTotal] = useState({ count: 0, bytes: 0 });
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<ReadonlySet<number>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [icloud, setIcloud] = useState(false);
  const [openHit, setOpenHit] = useState<SearchHit | null>(null);
  const [version, setVersion] = useState(0);
  const anchor = useRef<number | null>(null);
  const seq = useRef(0);
  const fetching = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const transcriptRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.storageSummary().then(setSummary, (e) => showToast(asCommandError(e).message, { tone: "error" }));
  }, [version]);

  // A new filter starts over from the first page.
  useEffect(() => {
    const my = ++seq.current;
    setLoading(true);
    fetching.current = true;
    api.listAttachments({ ...filter, limit: PAGE, offset: 0 }).then(
      (p) => {
        if (my !== seq.current) return;
        fetching.current = false;
        setRows(p.rows);
        setTotal({ count: p.totalCount, bytes: p.totalBytes });
        setLoading(false);
      },
      (e) => {
        if (my !== seq.current) return;
        fetching.current = false;
        setLoading(false);
        showToast(asCommandError(e).message, { tone: "error" });
      },
    );
    setSelected(new Set());
    anchor.current = null;
    scrollRef.current?.scrollTo({ top: 0 });
  }, [filter, version]);

  const loadMore = useCallback(() => {
    if (fetching.current || rows.length >= total.count) return;
    fetching.current = true;
    const my = seq.current;
    api.listAttachments({ ...filter, limit: PAGE, offset: rows.length }).then(
      (p) => {
        fetching.current = false;
        if (my === seq.current) setRows((r) => [...r, ...p.rows]);
      },
      () => (fetching.current = false),
    );
  }, [filter, rows.length, total.count]);

  // The table body sits below the summary: tell the virtualizer where it starts.
  const [margin, setMargin] = useState(0);
  useLayoutEffect(() => {
    if (bodyRef.current) setMargin(bodyRef.current.offsetTop);
  }, [summary, loading]);

  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
    scrollMargin: margin,
    getItemKey: (i) => rows[i].id,
  });
  const items = v.getVirtualItems();
  const lastIdx = items[items.length - 1]?.index ?? 0;
  useEffect(() => {
    if (rows.length && lastIdx > rows.length - 30) loadMore();
  }, [lastIdx, rows.length, loadMore]);

  const toggle = useCallback(
    (index: number, shift: boolean) => {
      // Resolve the range before updating: state updaters must stay pure
      // (StrictMode runs them twice), so the anchor ref moves outside.
      const from = shift && anchor.current !== null ? Math.min(anchor.current, index) : index;
      const to = shift && anchor.current !== null ? Math.max(anchor.current, index) : index;
      anchor.current = index;
      const id = rows[index].id;
      setSelected((cur) => {
        const next = new Set(cur);
        const on = !cur.has(id);
        for (let i = from; i <= to; i++) {
          if (rows[i].inTrash) continue;
          if (on) next.add(rows[i].id);
          else next.delete(rows[i].id);
        }
        return next;
      });
    },
    [rows],
  );

  const open = useCallback(
    (r: AttachmentRow) => {
      if (viewable(r.kind)) {
        const gallery: LightboxItem[] = rows
          .filter((x) => viewable(x.kind))
          .map((x) => ({ id: x.id, filename: x.filename, mime: x.mime, path: x.path, bytes: x.bytes, kind: x.kind, sender: x.sender, fromMe: x.fromMe, dateMs: x.dateMs }));
        openLightbox(gallery, r.id);
        return;
      }
      if (r.chatId == null) return;
      setOpenHit({
        messageId: r.messageId,
        chatId: r.chatId,
        chatTitle: r.chatTitle,
        isGroup: false,
        fromMe: r.fromMe,
        sender: r.sender,
        dateMs: r.dateMs,
        snippet: "",
        matchedBy: "filter",
        attachmentCount: 1,
        score: 0,
        people: [],
      });
    },
    [rows],
  );

  const selectedRows = useMemo(() => rows.filter((r) => selected.has(r.id)), [rows, selected]);
  const selectedBytes = selectedRows.reduce((s, r) => s + r.bytes, 0);
  const selectable = useMemo(() => rows.filter((r) => !r.inTrash), [rows]);
  const allLoadedSelected = selectable.length > 0 && selected.size === selectable.length;
  const anyInTrash = useMemo(() => rows.some((r) => r.inTrash), [rows]);
  const showInMessages = useCallback((r: AttachmentRow) => {
    if (r.chatId != null) void api.openInMessages(r.chatId);
  }, []);

  const doTrash = async () => {
    setConfirm(false);
    const ids = selectedRows.filter((r) => r.onDisk).map((r) => r.id);
    try {
      const res = await api.trashAttachments(ids);
      const failed = res.failed.length ? `. ${res.failed.length} could not be moved.` : "";
      showToast(`Moved ${formatCount(res.trashed)} to the Trash, ${formatBytes(res.bytes)} freed${failed}`);
      setVersion((x) => x + 1);
    } catch (e) {
      showToast(asCommandError(e).message, { tone: "error" });
    }
  };

  const chatName = summary?.byChat.find((c) => c.chatId === filter.chatId)?.title ?? null;

  return (
    <div className={`storage-page${openHit ? " has-panel" : ""}`}>
      <div className="page-scroll v-scroll" ref={scrollRef}>
        <div className="page-col is-wide">
          <header className="page-head">
            <div>
              <h1 className="page-title">Storage</h1>
              <p className="page-sub">
                {summary
                  ? `${formatBytes(summary.bytesOnDisk)} on this Mac · ${formatBytes(summary.totalBytes)} total in ${formatCount(summary.totalCount)} attachments`
                  : " "}
              </p>
            </div>
            <button className="btn" onClick={() => setIcloud(true)}>
              <Icon name="cloud" size={16} />
              Delete from iCloud too
            </button>
          </header>

          {summary && (
            <div className="card-row">
              <section className="card">
                <h2 className="card-title">By type</h2>
                <KindBar summary={summary} />
                {summary.byKind
                  .filter((k) => k.count > 0)
                  .map((k) => (
                    <button
                      key={k.kind}
                      className={`rank-row${filter.kind === k.kind ? " is-active" : ""}`}
                      onClick={() => setFilter((f) => ({ ...f, kind: f.kind === k.kind ? null : k.kind }))}
                    >
                      <span className={`kind-dot kind-${k.kind}`} />
                      <span className="rank-name">{KIND_LABEL[k.kind]}</span>
                      <span className="rank-sub">{formatCount(k.count)}</span>
                      <span className="rank-total">{formatBytes(k.bytes)}</span>
                    </button>
                  ))}
              </section>
              <section className="card">
                <h2 className="card-title">Biggest conversations</h2>
                {summary.byChat.slice(0, 6).map((c) => (
                  <button
                    key={c.chatId}
                    className={`rank-row${filter.chatId === c.chatId ? " is-active" : ""}`}
                    onClick={() => setFilter((f) => ({ ...f, chatId: f.chatId === c.chatId ? null : c.chatId }))}
                  >
                    <PeopleAvatar people={c.people} fallback={c.title} size={24} />
                    <span className="rank-name">{c.title}</span>
                    <span className="rank-sub">{formatCount(c.count)}</span>
                    <span className="rank-total">{formatBytes(c.bytes)}</span>
                  </button>
                ))}
              </section>
            </div>
          )}

          <div className="toolbar">
            <Select
              label="Type"
              value={filter.kind ? KIND_LABEL[filter.kind] : null}
              items={[{ id: "all", label: "All types", checked: !filter.kind }, ...(["image", "video", "audio", "sticker", "file"] as AttachmentKind[]).map((k) => ({ id: k, label: KIND_LABEL[k], checked: filter.kind === k }))]}
              onPick={(i) => setFilter((f) => ({ ...f, kind: i.id === "all" ? null : (i.id as AttachmentKind) }))}
            />
            <Select
              label="Size"
              value={SIZES.find((s) => s.bytes === filter.minBytes && s.bytes !== null)?.label ?? null}
              items={SIZES.map((s) => ({ id: s.id, label: s.label, checked: s.bytes === filter.minBytes }))}
              onPick={(i) => setFilter((f) => ({ ...f, minBytes: SIZES.find((s) => s.id === i.id)!.bytes }))}
            />
            <Select
              label="Conversation"
              value={chatName}
              searchable
              placeholder="Find a conversation"
              items={[{ id: "all", label: "All conversations", checked: filter.chatId === null }, ...(summary?.byChat ?? []).map((c) => ({ id: String(c.chatId), label: c.title, detail: formatBytes(c.bytes), checked: filter.chatId === c.chatId }))]}
              onPick={(i) => setFilter((f) => ({ ...f, chatId: i.id === "all" ? null : Number(i.id) }))}
            />
            <div className="segments toolbar-sort" role="radiogroup" aria-label="Sort">
              {(["size", "date"] as const).map((s) => (
                <button key={s} role="radio" aria-checked={filter.sort === s} className={`segment${filter.sort === s ? " is-active" : ""}`} onClick={() => setFilter((f) => ({ ...f, sort: s }))}>
                  {s === "size" ? "Largest" : "Newest"}
                </button>
              ))}
            </div>
            {anyInTrash && <span className="toolbar-note">Empty the Trash to free the space.</span>}
            <span className="toolbar-count">
              {loading ? "Loading" : `${formatCount(total.count)} files · ${formatBytes(total.bytes)}`}
            </span>
          </div>

          <div className="table-head" role="row">
            <span className="th th-check">
              <input
                type="checkbox"
                aria-label="Select all loaded"
                checked={allLoadedSelected}
                ref={(el) => {
                  if (el) el.indeterminate = selected.size > 0 && !allLoadedSelected;
                }}
                onChange={() => setSelected(allLoadedSelected ? new Set() : new Set(selectable.map((r) => r.id)))}
              />
            </span>
            <span className="th th-file">File</span>
            <span className="th th-chat">Conversation</span>
            <span className="th th-from">From</span>
            <span className="th th-date">Date</span>
            <span className="th th-size">Size</span>
            <span className="th th-act" />
          </div>
          <div ref={bodyRef} style={{ height: v.getTotalSize(), position: "relative" }} role="rowgroup">
            {items.map((it) => {
              const r = rows[it.index];
              return (
                <div key={it.key} className="tr-slot" style={{ transform: `translateY(${it.start - margin}px)` }}>
                  <Row r={r} index={it.index} checked={selected.has(r.id)} onToggle={toggle} onOpen={open} onShow={showInMessages} />
                </div>
              );
            })}
          </div>
          {!loading && rows.length === 0 && <div className="table-empty">No attachments match these filters.</div>}
        </div>
      </div>

      {selected.size > 0 && (
        <div className="selection-bar">
          <span>
            {formatCount(selected.size)} selected · {formatBytes(selectedBytes)}
          </span>
          <button className="btn" onClick={() => setSelected(new Set())}>
            Clear
          </button>
          <button className="btn danger" onClick={() => setConfirm(true)}>
            <Icon name="trash" size={16} />
            Move to Trash
          </button>
        </div>
      )}

      {openHit && (
        <ConversationPanel
          key={openHit.chatId ?? openHit.messageId}
          hit={openHit}
          highlight={null}
          version={0}
          scrollerRef={transcriptRef}
          onClose={() => setOpenHit(null)}
          onEscape={() => setOpenHit(null)}
        />
      )}

      {confirm && <TrashConfirm rows={selectedRows} onCancel={() => setConfirm(false)} onConfirm={() => void doTrash()} />}
      {icloud && <ICloudHelp onClose={() => setIcloud(false)} />}
    </div>
  );
}

function KindBar({ summary }: { summary: StorageSummary }) {
  const total = Math.max(1, summary.totalBytes);
  return (
    <div className="kind-bar" aria-hidden="true">
      {summary.byKind
        .filter((k) => k.bytes > 0)
        .map((k) => (
          <span key={k.kind} className={`kind-${k.kind}`} style={{ width: `${(k.bytes / total) * 100}%` }} />
        ))}
    </div>
  );
}

const Row = memo(function Row({
  r,
  index,
  checked,
  onToggle,
  onOpen,
  onShow,
}: {
  r: AttachmentRow;
  index: number;
  checked: boolean;
  onToggle: (i: number, shift: boolean) => void;
  onOpen: (r: AttachmentRow) => void;
  onShow: (r: AttachmentRow) => void;
}) {
  const name = r.filename ?? KIND_LABEL[r.kind];
  return (
    <div className={`tr${checked ? " is-checked" : ""}${r.inTrash ? " is-trashed" : ""}`} role="row" onClick={() => onOpen(r)}>
      <span className="td td-check" onClick={(e) => e.stopPropagation()}>
        <input
          type="checkbox"
          aria-label={`Select ${name}`}
          checked={checked}
          disabled={r.inTrash}
          onChange={() => {}}
          onClick={(e) => onToggle(index, e.shiftKey)}
        />
      </span>
      <span className="td td-file">
        <Thumb r={r} />
        <span className="td-file-text">
          <span className="td-name">{name}</span>
          {r.inTrash ? (
            <span className="td-cloud">
              <Icon name="trash" size={12} />
              In Trash
            </span>
          ) : (
            !r.onDisk && (
              <span className="td-cloud" title="Only in iCloud">
                <Icon name="cloud" size={12} />
                Not on this Mac
              </span>
            )
          )}
        </span>
      </span>
      <span className="td td-chat">{r.chatTitle}</span>
      <span className="td td-from">{r.fromMe ? "You" : r.sender ?? ""}</span>
      <span className="td td-date">{listDate(r.dateMs)}</span>
      <span className="td td-size">{formatBytes(r.bytes)}</span>
      <span className="td td-act" onClick={(e) => e.stopPropagation()}>
        {r.chatId != null && (
          <button
            className="small-btn"
            onClick={() => onShow(r)}
            title="Opens the conversation. Find it under Details, Photos in Messages, then Delete to remove it everywhere."
          >
            Show in Messages
          </button>
        )}
      </span>
    </div>
  );
});

function Thumb({ r }: { r: AttachmentRow }) {
  const [failed, setFailed] = useState(false);
  if ((r.kind === "image" || r.kind === "sticker") && r.path && !failed) {
    return <img className="thumb" src={attachmentSrc(r.path)} alt="" loading="lazy" decoding="async" draggable={false} onError={() => setFailed(true)} />;
  }
  return (
    <span className="thumb thumb-icon">
      <Icon name={KIND_ICON[r.kind]} size={16} />
    </span>
  );
}

function TrashConfirm({ rows, onCancel, onConfirm }: { rows: AttachmentRow[]; onCancel: () => void; onConfirm: () => void }) {
  const local = rows.filter((r) => r.onDisk);
  const skipped = rows.length - local.length;
  const bytes = local.reduce((s, r) => s + r.bytes, 0);
  return (
    <Modal title="Move to Trash?" onClose={onCancel} width={480}>
      <p className="modal-note">
        {formatCount(local.length)} {local.length === 1 ? "attachment" : "attachments"} ({formatBytes(bytes)}) will go to the Trash.
      </p>
      <p className="modal-note">
        Frees space on this Mac only. The files stay in iCloud and on your other devices, and Messages may not download them again.
        You can restore them from the Trash until you empty it.
      </p>
      {skipped > 0 && (
        <p className="modal-note">
          {formatCount(skipped)} selected {skipped === 1 ? "file is" : "files are"} only in iCloud and will be skipped.
        </p>
      )}
      <ul className="confirm-list">
        {local.slice(0, 5).map((r) => (
          <li key={r.id}>
            <span>{r.filename ?? KIND_LABEL[r.kind]}</span>
            <span>{formatBytes(r.bytes)}</span>
          </li>
        ))}
        {local.length > 5 && <li className="confirm-more">and {formatCount(local.length - 5)} more</li>}
      </ul>
      <div className="modal-actions is-end">
        <button className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button className="btn danger" onClick={onConfirm} disabled={local.length === 0}>
          Move to Trash
        </button>
      </div>
    </Modal>
  );
}

/** Why this app can't delete from iCloud, and Apple's tool that can. */
function ICloudHelp({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="Delete from iCloud too" onClose={onClose} width={480}>
      <p className="modal-note">Apple doesn't let other apps delete messages or attachments from iCloud.</p>
      <p className="modal-note">
        To remove files everywhere, use Messages in System Settings, General, Storage. Deletions there reach iCloud and your other
        devices.
      </p>
      <p className="modal-note">For a single photo, use Show in Messages, then Details, Photos, and Delete.</p>
      <div className="modal-actions is-end">
        <button className="btn" onClick={onClose}>
          Close
        </button>
        <button
          className="btn primary"
          onClick={() => {
            void api.openStorageSettings().catch((e) => showToast(asCommandError(e).message, { tone: "error" }));
            onClose();
          }}
        >
          Open Messages Storage in System Settings
        </button>
      </div>
    </Modal>
  );
}
