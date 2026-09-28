import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Avatar, PeopleAvatar } from "../../components/Avatar";
import { api, asCommandError } from "../../lib/api";
import { formatBytes, formatCount } from "../../lib/format";
import { quoteValue } from "../../lib/query";
import type { DayCount, Insights } from "../../lib/types";

const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric", year: "numeric" });
const monthFmt = new Intl.DateTimeFormat(undefined, { month: "short" });
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const pad = (n: number) => String(n).padStart(2, "0");
const keyOf = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const parseDay = (k: string) => new Date(`${k}T12:00:00`);

/** GitHub-style stats over a year or all time. */
export function InsightsPage({ onSearch, onStorage }: { onSearch: (q: string) => void; onStorage: () => void }) {
  const [year, setYear] = useState<number | null>(null);
  const [data, setData] = useState<Insights | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    const my = ++seq.current;
    api.insights(year).then(
      (d) => {
        if (my !== seq.current) return;
        setData(d);
        setError(null);
      },
      (e) => my === seq.current && setError(asCommandError(e).message),
    );
  }, [year]);

  if (error && !data) return <div className="page-empty">{error}</div>;
  if (!data) return <div className="page-empty">Loading</div>;

  return (
    <div className="page-scroll v-scroll">
      <div className="page-col">
        <header className="page-head">
          <div>
            <h1 className="page-title">Insights</h1>
            <p className="page-sub">
              {formatCount(data.totalMessages)} messages
              {data.year === null ? " in all" : ` in ${data.year}`}
            </p>
          </div>
          <div className="segments" role="radiogroup" aria-label="Year">
            {[null, ...data.years].map((y) => (
              <button
                key={y ?? "all"}
                role="radio"
                aria-checked={year === y}
                className={`segment${year === y ? " is-active" : ""}`}
                onClick={() => setYear(y)}
              >
                {y ?? "All time"}
              </button>
            ))}
          </div>
        </header>

        <section className="tiles">
          <Tile label="Messages" value={formatCount(data.totalMessages)} sub={`${formatCount(data.sent)} sent · ${formatCount(data.received)} received`} />
          <Tile label="Conversations" value={formatCount(data.chats)} />
          <Tile
            label="Busiest day"
            value={data.busiestDay ? formatCount(data.busiestDay.count) : "None"}
            sub={data.busiestDay ? dayFmt.format(parseDay(data.busiestDay.day)) : undefined}
          />
          <Tile
            label="Longest streak"
            value={formatCount(data.longestStreak)}
            unit={data.longestStreak === 1 ? "day" : "days"}
            sub={`Current: ${formatCount(data.currentStreak)} ${data.currentStreak === 1 ? "day" : "days"}`}
          />
          <Tile label="Attachments" value={formatBytes(data.attachmentsBytes)} sub={`${formatCount(data.attachmentsCount)} files`} onClick={onStorage} />
        </section>

        <section className="card">
          <h2 className="card-title">Activity</h2>
          <Heatmap days={data.days} year={data.year} lastMs={data.lastMs} />
        </section>

        <div className="card-row">
          <section className="card">
            <h2 className="card-title">Busiest hours</h2>
            <Bars values={data.byHour} labels={(i) => (i % 6 === 0 ? hourLabel(i) : "")} title={(i, v) => `${formatCount(v)} messages at ${hourLabel(i)}`} />
          </section>
          <section className="card">
            <h2 className="card-title">Weekdays</h2>
            <Bars values={data.byWeekday} labels={(i) => WEEKDAYS[i]} title={(i, v) => `${formatCount(v)} messages on ${WEEKDAYS[i]}`} />
          </section>
        </div>

        <div className="card-row">
          <section className="card">
            <h2 className="card-title">Top people</h2>
            {data.topPeople.length === 0 && <p className="card-note">No one-to-one conversations in this period.</p>}
            {data.topPeople.slice(0, 10).map((p) => {
              const name = p.person.name ?? p.person.address;
              return (
                <button key={p.person.handleId} className="rank-row" onClick={() => onSearch(`with:${quoteValue(name)}`)} title={`Search messages with ${name}`}>
                  <Avatar name={name} src={p.person.avatar} size={28} />
                  <span className="rank-main">
                    <span className="rank-top">
                      <span className="rank-name">{name}</span>
                      <span className="rank-total">{formatCount(p.total)}</span>
                    </span>
                    <Split sent={p.sent} received={p.received} />
                  </span>
                </button>
              );
            })}
          </section>
          <section className="card">
            <h2 className="card-title">Top group chats</h2>
            {data.topGroups.length === 0 && <p className="card-note">No group chats in this period.</p>}
            {data.topGroups.slice(0, 10).map((g) => (
              <button key={g.chatId} className="rank-row" onClick={() => onSearch(`in:${quoteValue(g.title)}`)} title={`Search in ${g.title}`}>
                <PeopleAvatar people={g.people} fallback={g.title} size={28} />
                <span className="rank-main">
                  <span className="rank-top">
                    <span className="rank-name">{g.title}</span>
                    <span className="rank-total">{formatCount(g.total)}</span>
                  </span>
                  <span className="rank-sub">{g.people.length} people</span>
                </span>
              </button>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}

function hourLabel(h: number): string {
  if (h === 0) return "12a";
  if (h === 12) return "12p";
  return h < 12 ? `${h}a` : `${h - 12}p`;
}

function Tile({ label, value, unit, sub, onClick }: { label: string; value: string; unit?: string; sub?: string; onClick?: () => void }) {
  // Long numbers step the size down instead of truncating.
  const long = value.length + (unit ? unit.length + 1 : 0) > 8;
  const body = (
    <>
      <span className="tile-label">{label}</span>
      <span className={`tile-value${long ? " is-long" : ""}`}>
        {value}
        {unit && <span className="tile-unit"> {unit}</span>}
      </span>
      {sub && <span className="tile-sub">{sub}</span>}
    </>
  );
  return onClick ? (
    <button className="tile is-link" onClick={onClick}>
      {body}
    </button>
  ) : (
    <div className="tile">{body}</div>
  );
}

/** Sent vs received as one thin two-tone bar. */
function Split({ sent, received }: { sent: number; received: number }) {
  const total = Math.max(1, sent + received);
  return (
    <span className="split" title={`${formatCount(sent)} sent · ${formatCount(received)} received`}>
      <span className="split-bar">
        <span className="split-sent" style={{ width: `${(sent / total) * 100}%` }} />
      </span>
      <span className="split-text">
        {formatCount(sent)} sent · {formatCount(received)} received
      </span>
    </span>
  );
}

/** Plain bar chart: HTML columns, so labels never stretch with the card. */
const Bars = memo(function Bars({ values, labels, title }: { values: number[]; labels: (i: number) => string; title: (i: number, v: number) => string }) {
  const max = Math.max(1, ...values);
  return (
    <div className={`bars${values.length > 12 ? " is-dense" : ""}`} role="img" aria-label="Bar chart">
      {values.map((v, i) => (
        <div key={i} className="bar-col" title={title(i, v)}>
          <div className="bar-track">
            <div className="bar-fill" style={{ height: `${v > 0 ? Math.max(2, (v / max) * 100) : 0}%` }} />
          </div>
          <span className="bar-label">{labels(i)}</span>
        </div>
      ))}
    </div>
  );
});

const CELL = 11;
const GAP = 3;
const STEP = CELL + GAP;

/** 53 weeks by 7 days, Sunday at the top, like GitHub. */
const Heatmap = memo(function Heatmap({ days, year, lastMs }: { days: DayCount[]; year: number | null; lastMs: number | null }) {
  const [tip, setTip] = useState<{ x: number; y: number; text: string } | null>(null);

  const { cells, months, weeks } = useMemo(() => {
    const counts = new Map(days.map((d) => [d.day, d.count]));
    // Range: the selected year, or the 53 weeks up to the last message.
    let end: Date;
    let start: Date;
    if (year !== null) {
      start = new Date(year, 0, 1, 12);
      end = new Date(year, 11, 31, 12);
    } else {
      end = lastMs ? new Date(lastMs) : new Date();
      end.setHours(12, 0, 0, 0);
      start = new Date(end);
      start.setDate(start.getDate() - 7 * 52 - end.getDay());
    }
    const first = new Date(start);
    first.setDate(first.getDate() - first.getDay()); // back to Sunday
    // Levels from quartiles of the non-zero days, so one huge day doesn't wash out the rest.
    const nz = days.map((d) => d.count).sort((a, b) => a - b);
    const q = (p: number) => nz[Math.min(nz.length - 1, Math.floor(p * nz.length))] ?? 1;
    const t1 = q(0.25);
    const t2 = q(0.5);
    const t3 = q(0.75);
    const level = (c: number) => (c <= 0 ? 0 : c <= t1 ? 1 : c <= t2 ? 2 : c <= t3 ? 3 : 4);

    const cells: { x: number; y: number; key: string; count: number; level: number; out: boolean }[] = [];
    const months: { x: number; label: string }[] = [];
    const d = new Date(first);
    let w = 0;
    let lastMonth = -1;
    while (d <= end || d.getDay() !== 0) {
      if (d > end && d.getDay() === 0) break;
      const k = keyOf(d);
      const c = counts.get(k) ?? 0;
      const out = d < start || d > end;
      cells.push({ x: w * STEP, y: d.getDay() * STEP, key: k, count: c, level: level(c), out });
      if (!out && d.getDate() <= 7 && d.getDay() === 0 && d.getMonth() !== lastMonth) {
        months.push({ x: w * STEP, label: monthFmt.format(d) });
        lastMonth = d.getMonth();
      }
      if (d.getDay() === 6) w++;
      d.setDate(d.getDate() + 1);
    }
    return { cells, months, weeks: w + 1 };
  }, [days, year, lastMs]);

  const width = weeks * STEP;
  const hostRef = useRef<HTMLDivElement>(null);
  return (
    <div className="heatmap" ref={hostRef} onMouseLeave={() => setTip(null)}>
      <div className="heatmap-scroll">
      <div className="heatmap-days" aria-hidden="true">
        <span style={{ top: 1 * STEP }}>Mon</span>
        <span style={{ top: 3 * STEP }}>Wed</span>
        <span style={{ top: 5 * STEP }}>Fri</span>
      </div>
      <div className="heatmap-body">
        <svg width={width} height={7 * STEP + 18} role="img" aria-label="Messages per day">
          {months.map((m) => (
            <text key={`${m.x}`} className="heatmap-month" x={m.x} y={10}>
              {m.label}
            </text>
          ))}
          <g
            transform="translate(0 18)"
            onMouseMove={(e) => {
              const t = e.target as SVGElement;
              const k = t.getAttribute("data-day");
              if (!k) return;
              const n = Number(t.getAttribute("data-count"));
              const r = t.getBoundingClientRect();
              const host = hostRef.current!.getBoundingClientRect();
              setTip({
                // Clamped so the bubble never hangs off the card's edges.
                x: Math.min(host.width - 130, Math.max(130, r.left - host.left + CELL / 2)),
                y: r.top - host.top,
                text: `${n === 0 ? "No" : formatCount(n)} ${n === 1 ? "message" : "messages"} on ${dayFmt.format(parseDay(k))}`,
              });
            }}
          >
            {cells.map((c) =>
              c.out ? null : (
                <rect key={c.key} className={`hm hm-${c.level}`} x={c.x} y={c.y} width={CELL} height={CELL} rx={2.5} data-day={c.key} data-count={c.count} />
              ),
            )}
          </g>
        </svg>
        <div className="heatmap-legend" aria-hidden="true">
          <span>Less</span>
          {[0, 1, 2, 3, 4].map((l) => (
            <span key={l} className={`hm-swatch hm-${l}`} />
          ))}
          <span>More</span>
        </div>
      </div>
      </div>
      {tip && (
        <div className="tooltip" style={{ left: tip.x, top: tip.y }}>
          {tip.text}
        </div>
      )}
    </div>
  );
});
