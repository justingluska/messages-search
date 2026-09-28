// Tiny generated placeholder "photos" (SVG data URIs) for mock attachments.
// Each is a simple scene so the transcript reads like a real conversation.

export type Scene = "lake" | "dock" | "sunset" | "city" | "forest" | "food" | "dog" | "beach";

function svg(w: number, h: number, body: string): string {
  const s = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${body}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(s)}`;
}

function sky(id: string, top: string, bottom: string): string {
  return `<defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient></defs>`;
}

const scenes: Record<Scene, () => string> = {
  lake: () =>
    svg(640, 480, `${sky("s", "#7fb8e6", "#d9ecf7")}<rect width="640" height="480" fill="url(#s)"/>
      <path d="M0 250 L120 170 L230 240 L360 150 L500 235 L640 180 L640 300 L0 300Z" fill="#4f7a5a"/>
      <path d="M0 280 L160 230 L320 275 L470 225 L640 270 L640 310 L0 310Z" fill="#3c6147"/>
      <rect y="300" width="640" height="180" fill="#3f86b5"/>
      <rect y="300" width="640" height="14" fill="#6aa8cf" opacity=".6"/>
      <rect x="80" y="360" width="200" height="3" fill="#bfe0f2" opacity=".6"/><rect x="340" y="410" width="220" height="3" fill="#bfe0f2" opacity=".5"/>`),
  dock: () =>
    svg(480, 640, `${sky("s", "#f3c58f", "#9fc6e3")}<rect width="480" height="640" fill="url(#s)"/>
      <path d="M0 260 L140 200 L300 250 L480 190 L480 300 L0 300Z" fill="#46604d"/>
      <rect y="300" width="480" height="340" fill="#2f6f99"/>
      <path d="M200 640 L230 330 L250 330 L280 640Z" fill="#8a6a4d"/>
      <path d="M215 640 L236 330 L244 330 L265 640Z" fill="#a4815f"/>
      <circle cx="360" cy="150" r="34" fill="#fff4d6" opacity=".9"/>`),
  sunset: () =>
    svg(640, 480, `${sky("s", "#f07c5b", "#fbd38d")}<rect width="640" height="480" fill="url(#s)"/>
      <circle cx="320" cy="300" r="70" fill="#ffe7a8"/>
      <rect y="300" width="640" height="180" fill="#7a4b6b"/>
      <rect x="220" y="320" width="200" height="4" fill="#ffd08a" opacity=".8"/><rect x="250" y="345" width="140" height="4" fill="#ffd08a" opacity=".6"/>
      <path d="M0 300 L90 270 L180 300Z" fill="#3b2a3f"/>`),
  city: () =>
    svg(480, 640, `${sky("s", "#1f2c4d", "#5d6f9e")}<rect width="480" height="640" fill="url(#s)"/>
      <rect x="20" y="300" width="70" height="340" fill="#18203a"/><rect x="100" y="220" width="90" height="420" fill="#222c4b"/>
      <rect x="200" y="330" width="60" height="310" fill="#18203a"/><rect x="270" y="180" width="100" height="460" fill="#1c2644"/>
      <rect x="380" y="280" width="80" height="360" fill="#222c4b"/>
      <g fill="#f5d77a" opacity=".85"><rect x="120" y="250" width="8" height="10"/><rect x="150" y="290" width="8" height="10"/><rect x="290" y="210" width="8" height="10"/><rect x="330" y="260" width="8" height="10"/><rect x="300" y="330" width="8" height="10"/><rect x="400" y="320" width="8" height="10"/></g>`),
  forest: () =>
    svg(640, 480, `${sky("s", "#b9d7c9", "#eaf2e3")}<rect width="640" height="480" fill="url(#s)"/>
      <g fill="#2f5d44"><path d="M60 400 L110 220 L160 400Z"/><path d="M170 420 L230 180 L290 420Z"/><path d="M330 410 L380 230 L430 410Z"/><path d="M450 430 L520 170 L590 430Z"/></g>
      <rect y="400" width="640" height="80" fill="#6d8f4e"/><path d="M0 470 Q320 380 640 470 L640 480 L0 480Z" fill="#c9b27c"/>`),
  food: () =>
    svg(560, 560, `<rect width="560" height="560" fill="#e9dfd0"/>
      <circle cx="280" cy="280" r="200" fill="#ffffff"/><circle cx="280" cy="280" r="170" fill="#f6efe4"/>
      <path d="M170 300 Q280 170 390 300 Q280 350 170 300Z" fill="#e7b85e"/>
      <path d="M190 300 Q280 210 370 300" stroke="#a0522d" stroke-width="16" fill="none"/>
      <circle cx="240" cy="270" r="10" fill="#6a9e4a"/><circle cx="300" cy="255" r="9" fill="#d6453d"/><circle cx="330" cy="282" r="9" fill="#6a9e4a"/>
      <rect x="440" y="120" width="16" height="320" rx="8" fill="#b0b5bb"/>`),
  dog: () =>
    svg(560, 560, `${sky("s", "#f2e6d8", "#e2d2bf")}<rect width="560" height="560" fill="url(#s)"/>
      <ellipse cx="280" cy="420" rx="150" ry="110" fill="#c98b4f"/>
      <circle cx="280" cy="260" r="110" fill="#d49a5c"/>
      <ellipse cx="185" cy="230" rx="38" ry="80" fill="#9c6436" transform="rotate(18 185 230)"/>
      <ellipse cx="375" cy="230" rx="38" ry="80" fill="#9c6436" transform="rotate(-18 375 230)"/>
      <circle cx="245" cy="250" r="12" fill="#2b1d12"/><circle cx="315" cy="250" r="12" fill="#2b1d12"/>
      <ellipse cx="280" cy="305" rx="26" ry="18" fill="#2b1d12"/><path d="M280 322 Q280 345 300 350" stroke="#2b1d12" stroke-width="5" fill="none"/>`),
  beach: () =>
    svg(640, 480, `${sky("s", "#8fd0f0", "#e6f6fb")}<rect width="640" height="480" fill="url(#s)"/>
      <rect y="250" width="640" height="110" fill="#2aa3c9"/><rect y="340" width="640" height="140" fill="#f0dcaa"/>
      <path d="M0 345 Q160 325 320 345 T640 345 L640 360 L0 360Z" fill="#ffffff" opacity=".8"/>
      <circle cx="520" cy="110" r="44" fill="#fff6c4"/>
      <path d="M120 470 L150 330" stroke="#6b4b2f" stroke-width="6"/><path d="M60 330 Q150 290 240 330Z" fill="#e8574b"/>`),
};

const cache = new Map<Scene, string>();
export function sceneImage(s: Scene): string {
  let v = cache.get(s);
  if (!v) {
    v = scenes[s]();
    cache.set(s, v);
  }
  return v;
}

/** A path WebKit can't decode (like a HEIC): exercises the file-tile fallback. */
export const BROKEN_IMAGE = "data:image/heic;base64,AAAAHGZ0eXBoZWljAAAAAG1pZjFoZWlj";

export const SCENES: Scene[] = ["lake", "dock", "sunset", "city", "forest", "food", "dog", "beach"];

/** A small generated "contact photo": a portrait silhouette on a soft background. */
export function portrait(bg: [string, string], skin: string, hair: string, shirt: string): string {
  return svg(
    96,
    96,
    `<defs><linearGradient id="p" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${bg[0]}"/><stop offset="1" stop-color="${bg[1]}"/></linearGradient></defs>
    <rect width="96" height="96" fill="url(#p)"/>
    <path d="M12 96c3-19 17-29 36-29s33 10 36 29z" fill="${shirt}"/>
    <rect x="41" y="54" width="14" height="14" rx="5" fill="${skin}"/>
    <ellipse cx="48" cy="42" rx="17" ry="19" fill="${skin}"/>
    <path d="M30 42c-2-16 7-26 18-26s21 9 18 26c-3-9-10-13-18-13s-15 4-18 13z" fill="${hair}"/>`,
  );
}
