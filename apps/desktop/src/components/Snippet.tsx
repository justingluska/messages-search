/** Render U+0002…U+0003 match markers as <mark>. */
export function Snippet({ text }: { text: string }) {
  if (!text.includes("\u0002")) return <>{text}</>;
  const out: React.ReactNode[] = [];
  const re = /\u0002([\s\S]*?)\u0003/g;
  let last = 0;
  let k = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(<mark key={k++}>{m[1]}</mark>);
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}
