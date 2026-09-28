import { useEffect, useState } from "react";
import { PeopleAvatar } from "../../components/Avatar";
import { Icon } from "../../components/Icon";
import { api } from "../../lib/api";
import { shortName } from "../../lib/format";
import type { ChatSummary, SearchHit } from "../../lib/types";
import { Transcript } from "./Transcript";

const chatCache = new Map<number, ChatSummary | null>();

function subtitle(chat: ChatSummary): string {
  if (chat.isGroup) {
    const names = chat.participants.map((p) => (p.name ? shortName(p.name) : p.address));
    return names.length <= 1 ? names.join("") : `${names.slice(0, -1).join(", ")} & ${names[names.length - 1]}`;
  }
  const p = chat.participants[0];
  return p && p.name ? p.address : "";
}

/** The conversation beside the results, jumped to the hit. */
export function ConversationPanel({
  hit,
  highlight,
  version,
  scrollerRef,
  onClose,
  onEscape,
}: {
  hit: SearchHit;
  highlight: RegExp | null;
  version: number;
  scrollerRef: React.RefObject<HTMLDivElement | null>;
  onClose: () => void;
  /** Esc / ← inside the transcript: back to the results list. */
  onEscape: () => void;
}) {
  const chatId = hit.chatId;
  const [chat, setChat] = useState<ChatSummary | null>(chatId != null ? chatCache.get(chatId) ?? null : null);

  useEffect(() => {
    if (chatId == null) return;
    const cached = chatCache.get(chatId);
    if (cached !== undefined) {
      setChat(cached);
      return;
    }
    let live = true;
    api.getChat(chatId).then((c) => {
      chatCache.set(chatId, c);
      if (live) setChat(c);
    });
    return () => {
      live = false;
    };
  }, [chatId]);

  const title = chat?.title ?? hit.chatTitle;
  const sub = chat ? subtitle(chat) : "";

  return (
    <section className="panel" aria-label={`Conversation: ${title}`}>
      <header className="panel-head">
        <button className="icon-btn" onClick={onClose} aria-label="Close conversation" title="Close (Esc)">
          <Icon name="x" size={16} />
        </button>
        <div className="panel-title-wrap">
          <PeopleAvatar people={hit.people ?? []} fallback={title} size={28} />
          <div className="panel-titles">
            <div className="panel-title">{title}</div>
            {sub && <div className="panel-sub">{sub}</div>}
          </div>
        </div>
        {chatId != null && (
          <button className="btn" onClick={() => void api.openInMessages(chatId)} title="Open in Messages (⌘O)">
            Open in Messages
            <Icon name="external" size={16} />
          </button>
        )}
      </header>
      {chatId == null ? (
        <div className="panel-empty">This message isn't linked to a conversation.</div>
      ) : (
        <Transcript
          chatId={chatId}
          hitId={hit.messageId}
          isGroup={chat?.isGroup ?? hit.isGroup}
          highlight={highlight}
          version={version}
          scrollerRef={scrollerRef}
          onEscape={onEscape}
        />
      )}
    </section>
  );
}
