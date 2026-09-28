import { memo, useState } from "react";
import { attachmentSrc } from "../lib/api";
import { initials } from "../lib/format";
import type { Person } from "../lib/types";

/**
 * A contact photo when one is known, else a monogram. A photo that fails to
 * load (missing file, outside the asset scope) falls back to the monogram.
 */
export const Avatar = memo(function Avatar({ name, src, size = 40 }: { name: string; src?: string | null; size?: number }) {
  const [failed, setFailed] = useState(false);
  const style = { width: size, height: size, fontSize: Math.round(size * 0.4) };
  if (src && !failed) {
    return (
      <img
        className="avatar avatar-photo"
        style={style}
        src={attachmentSrc(src)}
        alt=""
        draggable={false}
        decoding="async"
        onError={() => setFailed(true)}
      />
    );
  }
  const text = initials(name);
  return (
    <span className="avatar" style={style} aria-hidden="true">
      {text || <PersonGlyph size={size} />}
    </span>
  );
});

const label = (p: Person) => p.name ?? p.address;

/** One person: their photo or monogram. Two or more: two overlapping avatars, like Messages. */
export const PeopleAvatar = memo(function PeopleAvatar({ people, fallback, size = 40 }: { people: Person[]; fallback: string; size?: number }) {
  if (people.length <= 1) {
    const p = people[0];
    return <Avatar name={p ? label(p) : fallback} src={p?.avatar} size={size} />;
  }
  const small = Math.round(size * 0.62);
  return (
    <span className="avatar-group" style={{ width: size, height: size }} aria-hidden="true">
      <span className="avatar-group-b">
        <Avatar name={label(people[1])} src={people[1].avatar} size={small} />
      </span>
      <span className="avatar-group-a">
        <Avatar name={label(people[0])} src={people[0].avatar} size={small} />
      </span>
    </span>
  );
});

function PersonGlyph({ size }: { size: number }) {
  const s = Math.round(size * 0.62);
  return (
    <svg width={s} height={s} viewBox="0 0 24 24" fill="currentColor">
      <circle cx="12" cy="8.5" r="4.2" />
      <path d="M3.8 20.5c.9-4 4.2-6.3 8.2-6.3s7.3 2.3 8.2 6.3c.1.4-.2.8-.7.8H4.5c-.5 0-.8-.4-.7-.8Z" />
    </svg>
  );
}
