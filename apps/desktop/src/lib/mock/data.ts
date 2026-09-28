// A fictional Messages history for the mock backend: ~11 chats, scripted
// scenes (a lake house gate code, an Austin restaurant, flights, files,
// photos) plus a few thousand generated filler messages. Everyone here is
// fictional: 555-01xx numbers and .example addresses only.

import type { AttachmentKind, AttachmentView, ChatSummary, MessageKind, MessageView, Person, ReactionView } from "../types";
import { BROKEN_IMAGE, SCENES, portrait, sceneImage, type Scene } from "./images";

// ---------------------------------------------------------------- random ---

function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = mulberry32(20240704);
const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(rand() * xs.length)];
const chance = (p: number) => rand() < p;

// ---------------------------------------------------------------- people ---

type Key = "maya" | "jordan" | "sam" | "priya" | "leo" | "nora" | "mom" | "dad" | "ava" | "ethan" | "olivia" | "diego" | "courier";

export const PEOPLE: Record<Key, Person> = {
  maya: { handleId: 1, address: "+14155550117", name: "Maya Patel", avatar: null },
  jordan: { handleId: 2, address: "+15125550142", name: "Jordan Reyes", avatar: null },
  sam: { handleId: 3, address: "sam.whitaker@icloud.example", name: "Sam Whitaker", avatar: null },
  priya: { handleId: 4, address: "+17375550163", name: "Priya Nair", avatar: null },
  leo: { handleId: 5, address: "leo.brandt@work.example", name: "Leo Brandt", avatar: null },
  nora: { handleId: 6, address: "+16465550188", name: "Nora Kim", avatar: null },
  mom: { handleId: 7, address: "+19145550104", name: "Mom", avatar: null },
  dad: { handleId: 8, address: "+19145550109", name: "Dad", avatar: null },
  ava: { handleId: 9, address: "+19145550131", name: "Ava Gold", avatar: null },
  ethan: { handleId: 10, address: "+13055550176", name: "Ethan Cole", avatar: null },
  olivia: { handleId: 11, address: "+13055550122", name: "Olivia Hart", avatar: null },
  diego: { handleId: 12, address: "+12125550155", name: "Diego Alvarez", avatar: null },
  courier: { handleId: 13, address: "+13125550199", name: null, avatar: null },
};

// About half the contacts have a photo; the rest show initials.
PEOPLE.maya.avatar = portrait(["#f7d6c4", "#e9a98b"], "#c98c6a", "#2b1a12", "#3b82f6");
PEOPLE.jordan.avatar = portrait(["#c7ddf2", "#8fb6dd"], "#8d5a3b", "#1c1411", "#2f855a");
PEOPLE.priya.avatar = portrait(["#f4e3b5", "#e2c071"], "#a8704d", "#0f0b0a", "#b83280");
PEOPLE.nora.avatar = portrait(["#d9d2f3", "#a99be0"], "#e8c0a0", "#3a2418", "#1f2937");
PEOPLE.mom.avatar = portrait(["#cfe8d8", "#93c9a8"], "#e0b08e", "#8a8a8a", "#c05621");
PEOPLE.ethan.avatar = portrait(["#f2d0d0", "#dc9a9a"], "#f0c7a4", "#9c6b3a", "#4a5568");

const display = (p: Person) => p.name ?? p.address;

// ----------------------------------------------------------------- chats ---

interface ChatDef {
  id: number;
  name: string | null;
  members: Key[];
  service: string;
  /** Relative weight of generated filler bursts. */
  weight: number;
  bank: string[];
}

const GENERIC = [
  "haha yes", "lol", "omg", "on my way", "running 10 min late, sorry!", "sounds good", "perfect", "ok see you soon",
  "what time works for you?", "tomorrow works", "can't wait", "yesss", "wait really?", "that's amazing", "no way",
  "call you in a bit", "just saw this", "sorry, was in a meeting", "love that", "same", "totally", "ugh", "ha",
  "did you see that?", "let me check and get back to you", "good morning!", "night!", "thank you!!", "anytime",
  "I'm so tired today", "want to grab coffee this week?", "how was your weekend?", "pretty chill, you?",
  "it was so good to see you", "let's do it", "maybe next week?", "I'll text you when I'm close", "here!",
  "parking now", "be there in 5", "that's hilarious", "I can't stop laughing", "wait what", "exactly",
  "I forgot to tell you", "you'll never guess who I ran into", "text me when you get home", "home safe",
  "happy friday", "need a vacation", "ok that's fair", "good call", "honestly yes", "I'm down", "noted",
  "brb", "sorry just saw this", "yep", "nope", "maybe", "is it raining there?", "so hot today",
  "what are you up to tonight?", "watching the game", "cooking dinner", "early night for me", "hope you feel better",
];

const DEFS: ChatDef[] = [
  { id: 1, name: null, members: ["maya"], service: "iMessage", weight: 14, bank: [
    "want to get dinner Thursday?", "that new ramen place near you looks good", "how's the new job going?",
    "I finally finished that book you lent me", "sending you the playlist", "your plants are thriving btw",
    "wanna do a hike Saturday morning?", "the trail by the reservoir?", "yoga at 6?", "I owe you a coffee",
    "did you book the dentist yet", "my landlord is fixing the heater tomorrow finally",
  ] },
  { id: 2, name: "Lake House 🏠", members: ["maya", "jordan", "sam", "priya"], service: "iMessage", weight: 10, bank: [
    "who's bringing the grill stuff?", "I can drive, have room for 3", "someone remind me to pack sunscreen",
    "the kayaks are in the shed", "what's the checkout time again?", "I'll do a grocery run on the way",
    "we should do a bonfire saturday", "does anyone have a speaker?", "boat rental opens at 9",
    "I'm splitting the grocery bill, venmo me $38", "found a board game shop in town",
  ] },
  { id: 3, name: null, members: ["mom"], service: "iMessage", weight: 12, bank: [
    "Did you eat?", "Call me when you have a minute", "Your dad says hi", "Biscuit got into the trash again",
    "Are you coming for the holidays?", "I sent you the recipe", "Love you!", "Don't forget your aunt's birthday",
    "Took Biscuit to the vet, he's fine", "How's the weather there?", "Proud of you", "Wear a jacket, it's cold",
  ] },
  { id: 4, name: "Family", members: ["mom", "dad", "ava"], service: "iMessage", weight: 8, bank: [
    "who took the good scissors", "dinner at 6 on Sunday", "Ava can you pick up bread", "the wifi is down again",
    "grandma called, she wants a photo of everyone", "happy birthday dad!!", "who's watching the game",
    "I'm bringing dessert", "the car needs an oil change", "family photo day is the 12th",
  ] },
  { id: 5, name: null, members: ["jordan"], service: "iMessage", weight: 12, bank: [
    "did you watch the game last night", "that ending was unreal", "gym at 7?", "leg day, pray for me",
    "trying that new bbq spot on Saturday", "fantasy draft is Sunday", "I'm starting Hurts this week",
    "concert tickets go on sale Friday at 10", "want to split an uber?", "new bike day 🚲",
  ] },
  { id: 6, name: null, members: ["priya"], service: "iMessage", weight: 9, bank: [
    "Austin is so hot right now", "the food trucks on East 6th are elite", "how's the job hunt?",
    "sending you a pic of the new apartment", "we should plan a trip", "coffee at Figment again?",
    "I started running again", "my sister is visiting next weekend", "brunch Sunday?",
  ] },
  { id: 7, name: null, members: ["leo"], service: "iMessage", weight: 8, bank: [
    "Can you look at the deck before 3?", "Standup moved to 10:30", "Shipping the release tonight",
    "The dashboard numbers look off", "Pushed a fix for the export bug", "Customer call went well",
    "Can you review my PR when you get a sec?", "Budget review is on Thursday", "OOO tomorrow, back Monday",
    "Nice work on the launch", "Who owns the onboarding flow now?", "Let's sync after lunch",
  ] },
  { id: 8, name: null, members: ["nora"], service: "iMessage", weight: 9, bank: [
    "the light this morning was perfect", "new film roll just came back", "want to shoot downtown saturday?",
    "golden hour at the pier?", "I edited the photos from the wedding", "gallery opening is on the 21st",
    "check out this print", "the museum is free on Thursdays",
  ] },
  { id: 9, name: "Run Club 🏃", members: ["ethan", "olivia", "diego", "nora"], service: "iMessage", weight: 8, bank: [
    "long run Sunday: 10 miles, 8:30 pace, meet at the bridge 7am", "tempo tuesday is on", "track workout moved to 6:15",
    "who's doing the half in March?", "my knee is acting up, taking it easy", "PR!!! 1:42 half",
    "coffee after the run?", "it's going to be 90 degrees, bring water", "easy 5 tomorrow",
  ] },
  { id: 10, name: null, members: ["diego", "olivia", "ethan"], service: "iMessage", weight: 4, bank: [
    "Nora's birthday is on the 14th, surprise dinner?", "I can book a table for 8", "don't tell her!!",
    "should we get her the camera strap she wanted", "I'll venmo whoever buys the gift", "cake from the bakery on 5th?",
  ] },
  { id: 11, name: null, members: ["courier"], service: "SMS", weight: 0, bank: [] },
];

// --------------------------------------------------------------- records ---

interface Draft {
  chatId: number;
  from: Key | "me";
  dateMs: number;
  text: string | null;
  kind: MessageKind;
  guid: string;
  replyToGuid: string | null;
  edited: boolean;
  unsent: boolean;
  attachments: AttachmentView[];
  reactions: ReactionView[];
}

let guidSeq = 0;
let attSeq = 0;
const drafts: Draft[] = [];

function att(kind: AttachmentKind, filename: string, mime: string, bytes: number, path: string | null): AttachmentView {
  return { id: ++attSeq, filename, mime, path, bytes, kind };
}
function photo(scene: Scene): AttachmentView {
  const n = 1000 + Math.floor(rand() * 8999);
  // About one photo in eight lives only in iCloud (no local file).
  const local = rand() > 0.12;
  return att("image", `IMG_${n}.jpeg`, "image/jpeg", 1_200_000 + Math.floor(rand() * 2_400_000), local ? sceneImage(scene) : null);
}

type Opt = {
  photo?: Scene;
  heic?: boolean;
  file?: { name: string; mime: string; bytes: number; kind?: AttachmentKind };
  react?: [Key | "me", string][];
  edited?: boolean;
  unsent?: boolean;
  /** Reply to the message this many lines earlier in the scene. */
  replyBack?: number;
  system?: boolean;
  app?: boolean;
  /** Minutes after the previous line (default 1-3). */
  gap?: number;
};
type Line = [Key | "me", string | null, Opt?];

function reaction(who: Key | "me", emoji: string): ReactionView {
  return who === "me" ? { emoji, fromMe: true, sender: null, part: 0 } : { emoji, fromMe: false, sender: display(PEOPLE[who]), part: 0 };
}

function scene(chatId: number, start: Date | number, lines: Line[]) {
  let t = typeof start === "number" ? start : start.getTime();
  const made: Draft[] = [];
  for (const [from, text, o = {}] of lines) {
    t += (o.gap ?? 1 + Math.floor(rand() * 3)) * 60_000 + Math.floor(rand() * 40_000);
    const attachments: AttachmentView[] = [];
    if (o.photo) attachments.push(photo(o.photo));
    if (o.heic) attachments.push(att("image", "IMG_2231.HEIC", "image/heic", 2_830_000, BROKEN_IMAGE));
    if (o.file) {
      // Most files are on this Mac; the rest only in iCloud.
      const dir = Math.floor(rand() * 0xffff).toString(16).padStart(4, "0");
      const path = rand() > 0.15 ? `/Users/you/Library/Messages/Attachments/${dir.slice(0, 2)}/${dir}/${o.file.name}` : null;
      attachments.push(att(o.file.kind ?? "file", o.file.name, o.file.mime, o.file.bytes, path));
    }
    const d: Draft = {
      chatId,
      from,
      dateMs: t,
      text,
      kind: o.system ? "system" : o.app ? "app" : "text",
      guid: `MOCK-${(++guidSeq).toString(16).toUpperCase().padStart(6, "0")}`,
      replyToGuid: o.replyBack ? made[made.length - o.replyBack]?.guid ?? null : null,
      edited: !!o.edited,
      unsent: !!o.unsent,
      attachments,
      reactions: (o.react ?? []).map(([w, e]) => reaction(w, e)),
    };
    made.push(d);
    drafts.push(d);
  }
}

const DAY = 86_400_000;
const NOW = Date.now();
const at = (daysAgo: number, h: number, m = 0) => {
  const d = new Date(NOW - daysAgo * DAY);
  d.setHours(h, m, 0, 0);
  return d.getTime();
};

// --------------------------------------------------------- scripted scenes ---

scene(2, new Date(2024, 4, 18, 18, 50), [
  ["maya", "Maya Patel added Priya Nair to the conversation.", { system: true }],
  ["maya", "Maya Patel named the conversation “Lake House 🏠”.", { system: true, gap: 0 }],
  ["maya", "ok it's official, we got the lake house for July 4th weekend!!"],
  ["jordan", "LET'S GO", { react: [["maya", "❤️"]] }],
  ["sam", "wait which one, the one with the dock?"],
  ["maya", "yes the dock one 🙌"],
  ["maya", null, { photo: "lake", react: [["jordan", "‼️"], ["me", "❤️"]] }],
  ["priya", "I'm in. how much per person?"],
  ["maya", "$212 each, venmo me whenever", { react: [["jordan", "👍"]] }],
  ["me", "sending now"],
]);
scene(2, new Date(2024, 6, 3, 16, 30), [
  ["maya", "Heads up for tomorrow: the gate code is 4471# and the lockbox is on the left side of the garage", { react: [["me", "👍"], ["jordan", "‼️"]] }],
  ["maya", "wifi is LakeHouse-Guest / bluegill2024"],
  ["sam", "what time is everyone getting there?"],
  ["jordan", "leaving Austin at 7am so around noon"],
  ["me", "we'll be there by 2"],
  ["priya", "I'll bring the paddleboards"],
]);
scene(2, new Date(2024, 6, 4, 13, 50), [
  ["jordan", "gate code isn't working??"],
  ["maya", "did you hit the # at the end", { replyBack: 1 }],
  ["jordan", "...no", { react: [["me", "😂"], ["sam", "😂"], ["priya", "😂"]] }],
  ["jordan", "we're in 🙃"],
  ["me", null, { photo: "dock", gap: 50 }],
  ["sam", "that view though"],
  ["priya", null, { photo: "sunset", gap: 240, react: [["maya", "❤️"], ["me", "❤️"]] }],
  ["me", "best sunset of the summer"],
]);
scene(2, new Date(2025, 5, 2, 10, 5), [
  ["maya", "same weekend next year?? they have it open July 3-6"],
  ["me", "yes please"],
  ["jordan", "is the gate code still 4471#"],
  ["maya", "they said it'll change, will send the new one", { replyBack: 1 }],
]);
scene(2, at(3, 19, 2), [
  ["sam", "found these for the lake trip"],
  ["sam", "https://www.lakegear.example/paddleboards/inflatable-10ft"],
  ["priya", "ooh the blue one"],
]);

scene(6, new Date(2025, 2, 8, 12, 10), [
  ["priya", "When you're in Austin next week we have to go to Casa Brava on South Congress"],
  ["priya", "their brisket tacos are unreal"],
  ["me", "say less. Thursday dinner?"],
  ["priya", "I'll make a reservation for 7:30"],
  ["priya", "https://casabrava.example/menu"],
  ["me", "the green chile queso 👀", { react: [["priya", "😂"]] }],
]);
scene(6, new Date(2025, 2, 13, 21, 40), [
  ["me", "that was the best meal I've had all year", { react: [["priya", "❤️"]] }],
  ["priya", "told you!!! next time we're doing their brunch", { replyBack: 1 }],
  ["me", null, { photo: "food" }],
]);
scene(6, at(2, 11, 20), [
  ["priya", "are you coming back to Austin for the festival?"],
  ["me", "trying to! flights are wild right now"],
  ["priya", "you can stay with me"],
]);

scene(7, new Date(2025, 9, 14, 9, 0), [
  ["leo", "Morning! Here's the draft of the Q3 plan"],
  ["leo", null, { file: { name: "Q3 Planning Draft.pdf", mime: "application/pdf", bytes: 482_000 } }],
  ["leo", "https://docs.example.com/d/q3-plan"],
  ["me", "Thanks, will read before standup"],
  ["me", "One thing: can we move the launch review to Thursday?", { edited: true }],
  ["leo", "Works for me, moving it now", { replyBack: 1 }],
]);
scene(7, at(2, 16, 25), [
  ["leo", "Slides for tomorrow's offsite"],
  ["leo", null, { file: { name: "Offsite Deck.key", mime: "application/x-iwork-keynote-sffkey", bytes: 12_400_000 } }],
  ["me", "👍"],
  ["leo", "Also the budget sheet", { gap: 4 }],
  ["leo", null, { file: { name: "Budget FY27.xlsx", mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", bytes: 96_500 } }],
]);

scene(3, new Date(2025, 10, 27, 15, 0), [
  ["mom", "Don't forget to bring the pie dish back"],
  ["mom", null, { photo: "food" }],
  ["me", "I won't!! it was delicious"],
]);
scene(3, at(40, 10, 2), [
  ["mom", "Biscuit misses you", { photo: "dog", react: [["me", "❤️"]] }],
  ["mom", null, { photo: "dog" }],
  ["me", "omg look at him"],
  ["mom", null, { file: { name: "Audio Message.caf", mime: "audio/x-caf", bytes: 38_000, kind: "audio" } }],
]);
scene(3, at(2, 17, 55), [
  ["mom", "Call me when you land ❤️"],
  ["me", "Just landed, calling in 5", { gap: 190 }],
]);
scene(3, at(0, 9, 5), [
  ["mom", "Happy Sunday! Are you coming for dinner next weekend?"],
  ["mom", "Dad is making his lasagna"],
]);

scene(5, new Date(2025, 7, 21, 14, 2), [
  ["jordan", "my flight is UA 1432, lands at 6:40 at AUS"],
  ["me", "I'll grab you at arrivals"],
  ["jordan", null, { unsent: true }],
  ["jordan", "ignore that lol"],
  ["jordan", null, { app: true, gap: 60 }],
  ["me", "thanks for dinner!!"],
]);
scene(5, at(1, 22, 5), [
  ["jordan", "did you watch the game"],
  ["jordan", "that last drive was insane"],
  ["me", "I was screaming", { react: [["jordan", "😂"]] }],
]);

scene(8, new Date(2025, 3, 5, 8, 10), [
  ["nora", "sunrise shoot was worth it"],
  ["nora", null, { photo: "beach" }],
  ["nora", null, { photo: "city" }],
  ["me", "these are gorgeous", { react: [["nora", "❤️"]] }],
  ["nora", null, { heic: true }],
  ["nora", null, { file: { name: "IMG_4410.MOV", mime: "video/quicktime", bytes: 18_200_000, kind: "video" } }],
]);
scene(8, at(6, 18, 40), [
  ["nora", "forest walk today"],
  ["nora", null, { photo: "forest" }],
]);

scene(4, new Date(2024, 1, 3, 9, 0), [
  ["ava", "Ava Gold named the conversation “Family”.", { system: true }],
  ["ava", "ok now we have a family chat, no more 12 separate texts"],
  ["dad", "Who is this", { react: [["ava", "😂"], ["me", "😂"]] }],
  ["mom", "It's Ava, dear"],
]);
scene(4, at(1, 12, 30), [
  ["dad", "who took the good scissors"],
  ["ava", "not me"],
  ["me", "not me either"],
  ["mom", "They're in the junk drawer"],
]);

scene(9, at(4, 6, 10), [
  ["ethan", "long run Sunday: 10 miles, 8:30 pace, meet at the bridge 7am"],
  ["olivia", "in"],
  ["diego", "https://www.strava.example/activities/88213"],
  ["nora", "I'll bring the coffee after", { react: [["ethan", "❤️"]] }],
]);

scene(10, new Date(2025, 1, 2, 20, 15), [
  ["diego", "Nora's birthday is on the 14th, surprise dinner?"],
  ["olivia", "yes!! I can book a table for 8"],
  ["me", "I'll handle the cake"],
  ["ethan", "don't tell her!!"],
]);

scene(11, new Date(2025, 8, 9, 14, 30), [
  ["courier", "Hi this is Marco from Swift Couriers, I'm outside with your package"],
  ["me", "coming down!"],
  ["courier", "Thanks, have a good one"],
]);

// ------------------------------------------------------- generated filler ---

const START = new Date(2023, 10, 1).getTime();
const LINKS = [
  "https://www.youtube.example/watch?v=dQw4w9", "https://news.example.com/2025/local-parks-reopen",
  "https://www.recipes.example/lemon-pasta", "https://maps.example.com/?q=coffee+near+me",
  "https://www.tickets.example/events/summer-series", "https://open.music.example/playlist/37i9dQ",
];

function filler(total: number) {
  const pool = DEFS.filter((d) => d.weight > 0);
  const sum = pool.reduce((s, d) => s + d.weight, 0);
  let made = 0;
  while (made < total) {
    let r = rand() * sum;
    const def = pool.find((d) => (r -= d.weight) < 0) ?? pool[0];
    // Burst start: anywhere in the range, biased toward recent.
    const age = Math.pow(rand(), 1.6);
    const t0 = NOW - 2 * 3600_000 - age * (NOW - START);
    const hour = 8 + Math.floor(rand() * 15);
    const d = new Date(t0);
    d.setHours(hour, Math.floor(rand() * 60), 0, 0);
    if (d.getTime() > NOW - 3 * 3600_000) continue;
    const n = 3 + Math.floor(rand() * 11);
    const lines: Line[] = [];
    let who: Key | "me" = chance(0.5) ? "me" : pick(def.members);
    for (let i = 0; i < n; i++) {
      if (chance(0.45)) who = who === "me" ? pick(def.members) : chance(0.7) ? "me" : pick(def.members);
      const o: Opt = { gap: chance(0.8) ? Math.floor(rand() * 3) : 2 + Math.floor(rand() * 12) };
      let text: string | null = chance(0.45) ? pick(def.bank) : pick(GENERIC);
      if (chance(0.015)) text = pick(LINKS);
      if (chance(0.02) && def.id !== 7) {
        o.photo = pick(SCENES);
        text = chance(0.6) ? null : text;
      } else if (chance(0.006)) {
        const n = 1000 + Math.floor(rand() * 8999);
        o.file = { name: `IMG_${n}.MOV`, mime: "video/quicktime", bytes: 6_000_000 + Math.floor(rand() * 170_000_000), kind: "video" };
        text = null;
      } else if (chance(0.005)) {
        o.file = { name: "Audio Message.caf", mime: "audio/x-caf", bytes: 20_000 + Math.floor(rand() * 180_000), kind: "audio" };
        text = null;
      } else if (chance(0.004)) {
        const doc = pick(["Lease Agreement.pdf", "Boarding Pass.pdf", "Receipt.pdf", "Itinerary.pdf", "Notes.docx", "Budget.xlsx"]);
        o.file = { name: doc, mime: "application/pdf", bytes: 60_000 + Math.floor(rand() * 9_000_000) };
      }
      if (chance(0.06)) {
        const other: Key | "me" = who === "me" ? pick(def.members) : "me";
        o.react = [[other, pick(["❤️", "❤️", "👍", "😂", "😂", "‼️", "❓", "👎"])]];
      }
      if (chance(0.006)) o.edited = true;
      if (chance(0.04) && lines.length > 2) o.replyBack = 1 + Math.floor(rand() * 2);
      lines.push([who, text, o]);
    }
    scene(def.id, d, lines);
    made += n;
  }
}
filler(3200);

// ----------------------------------------------------------- final tables ---

drafts.sort((a, b) => a.dateMs - b.dateMs);

export const CHAT_DEFS = new Map(DEFS.map((d) => [d.id, d]));
export const MESSAGES: MessageView[] = drafts.map((d, i) => {
  const p = d.from === "me" ? null : PEOPLE[d.from];
  const def = CHAT_DEFS.get(d.chatId)!;
  return {
    id: 100_000 + i,
    guid: d.guid,
    chatId: d.chatId,
    fromMe: d.from === "me",
    sender: p ? display(p) : null,
    senderHandleId: p ? p.handleId : null,
    senderAvatar: p ? p.avatar : null,
    dateMs: d.dateMs,
    text: d.unsent ? null : d.text,
    kind: d.kind,
    service: def.service,
    replyToGuid: d.replyToGuid,
    edited: d.edited,
    unsent: d.unsent,
    attachments: d.attachments,
    reactions: d.reactions,
  };
});

export const BY_ID = new Map(MESSAGES.map((m, i) => [m.id, i]));
export const BY_CHAT = new Map<number, MessageView[]>();
for (const m of MESSAGES) {
  const list = BY_CHAT.get(m.chatId!) ?? [];
  list.push(m);
  BY_CHAT.set(m.chatId!, list);
}

function title(def: ChatDef): string {
  if (def.name) return def.name;
  const names = def.members.map((k) => display(PEOPLE[k]));
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} & ${names[names.length - 1]}`;
}

export const CHATS: ChatSummary[] = DEFS.map((def) => {
  const msgs = BY_CHAT.get(def.id) ?? [];
  const last = msgs[msgs.length - 1];
  return {
    id: def.id,
    title: title(def),
    isGroup: def.members.length > 1,
    participants: def.members.map((k) => PEOPLE[k]),
    lastMs: last?.dateMs ?? null,
    lastText: last?.text ?? null,
    messageCount: msgs.length,
  };
}).sort((a, b) => (b.lastMs ?? 0) - (a.lastMs ?? 0));

export const CHAT_BY_ID = new Map(CHATS.map((c) => [c.id, c]));
