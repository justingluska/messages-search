#!/usr/bin/env python3
"""Build synthetic, schema-accurate Messages databases for tests and benchmarks.

    python3 scripts/make-fixture.py            # fixtures/chat.db (~25k) + fixtures/chat-small.db (~500)
    python3 scripts/make-fixture.py --seed 7   # a different (still deterministic) history

Everything is fictional: 555-01xx numbers, .example email domains, made-up
people, places and links. The schema is the empty test database shipped with the
`imessage-database` crate (copied from the cargo registry, never modified there).

Realism the app's reader has to cope with, on purpose:
  * ~70% of rows have `text` NULL and the body only in `attributedBody` (an
    NSArchiver typedstream of NSMutableAttributedString, produced by the real
    Foundation archiver through scripts/typedstream-helper.swift, so macOS only).
    The older ~30% have plain `text`; a third of those also carry a body blob.
  * tapbacks (2000-2006, removals 3000-3006, custom emoji), threaded replies,
    edited and unsent messages (`message_summary_info` plists), attachments
    (image/video/voice memo/pdf, U+FFFC placeholders with transfer GUIDs), rich
    link previews (URLBalloonProvider + payload_data), group rename/join events,
    SMS rows inside iMessage chats, a person with both a phone and an email
    handle, unnamed groups with display_name '' (not NULL).
  * outgoing rows in 1:1 chats mostly carry the recipient's handle_id (as real
    chat.db does), some carry 0; outgoing rows in groups carry 0. Use is_from_me.

Also writes fixtures/<name>.needles.json: planted facts (gate code, wedding
hotel, flight numbers, ...) with their message GUIDs and paraphrased queries,
plus the fictional contact names for each handle (chat.db has no names).
"""

from __future__ import annotations

import argparse
import base64
import datetime as dt
import glob
import json
import os
import plistlib
import random
import shutil
import sqlite3
import subprocess
import sys
from zoneinfo import ZoneInfo

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HELPER_SRC = os.path.join(ROOT, "scripts", "typedstream-helper.swift")
HELPER_BIN = os.path.join(ROOT, "target", "fixture-tools", "typedstream-helper")
TZ = ZoneInfo("America/New_York")
APPLE_EPOCH = dt.datetime(2001, 1, 1, tzinfo=dt.timezone.utc)
OBJ = "￼"

ME_PHONE = "+12125550100"
ME_EMAIL = "alex.parker@icloud.example"

# ------------------------------------------------------------------ people ---
# key: (name, phone, email or None, style) ; style: lower = mostly lowercase, emoji = emoji rate
PEOPLE = {
    "maya": ("Maya Chen", "+12125550101", "maya.chen@icloud.example", dict(lower=0.7, emoji=0.25)),
    "leo": ("Leo Alvarez", "+15125550102", None, dict(lower=0.4, emoji=0.1)),
    "priya": ("Priya Shah", "+14155550103", "priya.shah@mail.example", dict(lower=0.2, emoji=0.3)),
    "sam": ("Sam Okafor", "+13125550104", None, dict(lower=0.8, emoji=0.05)),
    "nora": ("Nora Lindqvist", "+16175550105", None, dict(lower=0.1, emoji=0.15)),
    "diego": ("Diego Ramírez", "+13055550106", None, dict(lower=0.5, emoji=0.2)),
    "jonah": ("Jonah Weiss", "+15035550107", None, dict(lower=0.6, emoji=0.05)),
    "ava": ("Ava Thompson", "+12065550108", None, dict(lower=0.3, emoji=0.3)),
    "mom": ("Linda Parker", "+19145550109", "linda.parker@mail.example", dict(lower=0.0, emoji=0.35)),
    "dad": ("Tom Parker", "+19145550110", None, dict(lower=0.0, emoji=0.05)),
    "ben": ("Ben Parker", "+17185550111", None, dict(lower=0.8, emoji=0.1)),
    "grace": ("Grace Kim", "+13235550112", None, dict(lower=0.3, emoji=0.2)),
    "omar": ("Omar Haddad", "+17735550113", None, dict(lower=0.5, emoji=0.1)),
    "tessa": ("Tessa Novak", "+16465550114", None, dict(lower=0.2, emoji=0.2)),
    "ray": ("Ray Castillo", "+12015550115", None, dict(lower=0.0, emoji=0.0)),
}

# name, members, display_name (None = 1:1, '' = unnamed group), handle key, service,
# first/last year-month, weight, language
CHATS = [
    dict(key="maya", members=["maya"], start=(2019, 1), end=(2026, 9), weight=5.0),
    dict(key="mom", members=["mom"], start=(2019, 1), end=(2026, 9), weight=3.5),
    dict(key="leo", members=["leo"], start=(2019, 2), end=(2026, 9), weight=2.5),
    dict(key="ben", members=["ben"], start=(2019, 1), end=(2026, 9), weight=2.5),
    dict(key="priya", members=["priya"], start=(2020, 3), end=(2026, 9), weight=2.0, via_email=True),
    dict(key="sam", members=["sam"], start=(2019, 1), end=(2023, 5), weight=1.5),
    dict(key="nora", members=["nora"], start=(2021, 4), end=(2026, 9), weight=1.3),
    dict(key="diego", members=["diego"], start=(2019, 6), end=(2026, 9), weight=1.8, lang="es"),
    dict(key="jonah", members=["jonah"], start=(2019, 3), end=(2024, 8), weight=1.0),
    dict(key="ava", members=["ava"], start=(2022, 1), end=(2026, 9), weight=1.4),
    dict(key="grace", members=["grace"], start=(2019, 9), end=(2026, 9), weight=1.0),
    dict(key="ray", members=["ray"], start=(2021, 7), end=(2024, 6), weight=0.3, service="SMS"),
    dict(key="lake", members=["maya", "leo", "ben", "grace", "omar"], name="Lake House 🏠",
         start=(2020, 5), end=(2026, 9), weight=2.8, email_for={"maya"}),
    dict(key="family", members=["mom", "dad", "ben"], name="", start=(2019, 1), end=(2026, 9), weight=2.6),
    dict(key="books", members=["nora", "tessa", "priya", "ava"], name="Book Club 📚",
         start=(2022, 2), end=(2026, 9), weight=1.2),
]

# -------------------------------------------------------------- vocabulary ---
S = dict(
    food=["tacos", "ramen", "pizza", "sushi", "thai", "pho", "bbq", "dumplings", "burgers", "indian", "a poke bowl",
          "pasta", "bagels", "falafel", "curry", "wings", "brunch"],
    rest=["Little Fern", "Bodega Blue", "Sal's Pizzeria", "Golden Lantern", "The Copper Pot", "Nami Ramen",
          "Kettle & Crumb", "Olive Street Kitchen", "Mesa Verde Cantina", "Harbor Noodle Co", "Juniper Hall",
          "Two Birds Diner", "Saffron Table"],
    day=["tonight", "tomorrow", "Saturday", "Sunday", "Friday", "this weekend", "next week", "Thursday",
         "Monday", "after work"],
    time=["6", "6:30", "7", "7:30", "8", "noon", "11ish", "5:45", "9"],
    show=["The Long Field", "Night Harbor", "Paper Crowns", "Ironwood", "Silver Tide", "The Quiet Coast",
          "Northern Lights Bakery", "Hollow Creek", "Blue Meridian"],
    city=["Chicago", "Denver", "Seattle", "Boston", "Nashville", "Miami", "Philly", "San Diego", "Montreal"],
    thing=["charger", "umbrella", "sunglasses", "keys", "jacket", "headphones", "water bottle", "book"],
    chore=["laundry", "the dishes", "taxes", "the car registration", "the recycling", "my inbox"],
    work=["the Henderson deck", "the quarterly review", "standup", "my 1:1", "the offsite", "the launch",
          "the budget spreadsheet", "the client call", "interviews"],
    feeling=["exhausted", "so tired", "wiped", "running on fumes", "actually pretty good", "stressed"],
    sport=["the game", "the playoffs", "the match", "the race", "the fight"],
    store=["Trader Joe's", "Target", "Costco", "the farmers market", "CVS", "the hardware store"],
    grocery=["oat milk", "eggs", "bananas", "coffee", "bread", "paper towels", "limes", "cilantro", "tortillas",
             "parmesan", "spinach", "yogurt"],
    weather=["pouring", "freezing", "gorgeous out", "so humid", "snowing", "windy af"],
    pet_dog="Biscuit", pet_cat="Miso",
    amount=["12", "18.50", "24", "30", "45", "60", "15", "9.75"],
    num=["2", "3", "4", "5", "10", "20"],
)

ES = dict(
    comida=["arepas", "empanadas", "paella", "tacos", "ceviche", "sancocho", "tamales"],
    dia=["mañana", "el sábado", "el domingo", "esta noche", "el viernes", "la próxima semana"],
    hora=["6", "7", "8", "7:30", "mediodía"],
)

# A topic is a list of turns: (role, [alternatives]). Roles: A/B are the two
# sides (randomly me/them); C is another group member. A role prefixed with '?'
# is optional (50%). Alternatives are str.format templates over the slots.
EN_TOPICS = [
    [("A", ["want to get {food} {day}?", "{food} {day}? I'm craving it", "dinner {day}?", "are you free {day} for {food}"]),
     ("B", ["yes!!", "I'm in", "omg yes", "can we do {time} instead?", "maybe, what time", "yesss"]),
     ("A", ["{rest} at {time}?", "how about {rest}", "{rest}? they finally reopened", "I'll book {rest} for {time}"]),
     ("?B", ["perfect", "see you there", "👍", "works for me", "i'll be like 10 min late fyi"])],
    [("A", ["did you watch {show} yet", "ok you HAVE to watch {show}", "started {show} last night"]),
     ("B", ["not yet no spoilers", "which episode are you on", "omg the ending of ep 3", "is it good? everyone keeps talking about it"]),
     ("A", ["it's so good", "episode 4 is where it gets good", "I stayed up till 2 watching it", "the soundtrack alone"]),
     ("?B", ["ok starting tonight", "adding it to the list", "lol fine"])],
    [("A", ["how was your day", "how's it going", "how was work", "hows your week going"]),
     ("B", ["{feeling}", "long. {work} ran over", "honestly {feeling}, {work} was a lot", "not bad! you?"]),
     ("A", ["ugh sorry", "same tbh", "want to talk about it", "hang in there", "glad it's almost friday"]),
     ("?B", ["thanks 🙏", "it's fine lol", "yeah i just need sleep"])],
    [("A", ["can you grab {grocery} and {grocery2} on your way home", "we're out of {grocery}", "going to {store}, need anything?"]),
     ("B", ["yep", "on it", "anything else?", "which kind", "can't today sorry", "sure, also getting {grocery2}"]),
     ("?A", ["the usual kind", "also {grocery3} if they have it", "thank youuu"])],
    [("A", ["it's {weather} here", "have you been outside, it's {weather}", "{weather} again"]),
     ("B", ["same here", "ugh", "honestly love it", "staying in all day"])],
    [("A", ["did I leave my {thing} at your place", "have you seen my {thing}", "I think I left my {thing} in your car"]),
     ("B", ["yeah it's on the counter", "let me check", "nope haven't seen it", "found it! I'll bring it {day}"]),
     ("?A", ["lifesaver", "phew", "thank you!!"])],
    [("A", ["are you watching {sport}", "{sport} tonight?", "did you see {sport}"]),
     ("B", ["yesss", "that last play 😱", "can't believe that call", "missed it, what happened", "we're so back"]),
     ("?A", ["refs were blind", "unreal", "next week is gonna be wild"])],
    [("A", ["I'm going to be in {city} {day}", "thinking about a trip to {city}", "any recs for {city}?"]),
     ("B", ["no way! for work?", "you have to eat at {rest}", "I lived there for a year, will send a list", "jealous"]),
     ("?A", ["yeah work thing", "please send", "just for fun for once"])],
    [("A", ["I owe you ${amount} for last night", "venmo'd you ${amount}", "what do I owe you for the tickets"]),
     ("B", ["got it thanks", "${amount}", "don't worry about it", "you can get the next one"])],
    [("A", ["finally did {chore}", "I have to do {chore} today ugh", "procrastinating on {chore}"]),
     ("B", ["proud of you", "lol same", "do it now!!", "reward yourself after"])],
    [("A", ["running late, be there in {num} min", "omw", "stuck in traffic", "train is delayed"]),
     ("B", ["no worries", "ok", "take your time", "we're at the bar in the back"])],
    [("A", ["happy friday!!", "we made it to friday", "weekend plans?"]),
     ("B", ["nothing and I'm thrilled", "hiking {day} maybe", "sleeping", "going to see my parents"]),
     ("?A", ["love that", "jealous", "have fun!"])],
    [("A", ["remind me what time {day}", "what time are we meeting {day}", "still on for {day}?"]),
     ("B", ["{time}", "{time} at my place", "yep!", "can we push to {time}"]),
     ("?A", ["👍", "cool", "perfect"])],
    [("A", ["look at this", "lmao", "this is so us", "you'll appreciate this"]),
     ("B", ["😂😂", "STOP", "i'm crying", "why is this so accurate", "lol"])],
    [("A", ["my back is killing me", "I think I'm getting sick", "woke up with a sore throat"]),
     ("B", ["oh no", "feel better!!", "drink water and sleep", "do you need anything"]),
     ("?A", ["I'll live", "soup delivery would be nice lol", "thanks"])],
    [("A", ["new phone who dis", "got a new phone, text me so I have your number"]), ("B", ["lol hi", "welcome back"])],
    [("A", ["how's {pet_dog}", "how's {pet_cat} doing"]),
     ("B", ["chaotic as always", "she ate a sock again", "sleeping on my laptop rn", "so cute, sending pics"])],
    [("A", ["can you send me that recipe", "what was in that pasta you made"]),
     ("B", ["garlic, lemon, parmesan, a ton of butter", "I'll send it later", "it's from my grandma, don't share lol"]),
     ("?A", ["making it {day}", "thank you!"])],
    [("A", ["gym {day}?", "want to go for a run {day}", "yoga at {time}?"]),
     ("B", ["yes need it", "my legs still hurt from last time", "can't, maybe next week", "down"])],
    [("A", ["I'm so bored at work", "this meeting could have been an email", "{work} is going to be the death of me"]),
     ("B", ["lol same", "hang in there", "at least it's almost lunch", "quit and let's open a bakery"])],
    [("A", ["what are you getting your mom for her birthday", "any gift ideas for my dad?"]),
     ("B", ["flowers + dinner?", "a nice candle", "no clue, help", "that cookbook she mentioned"])],
    [("A", ["ok I booked the flights", "found cheap flights to {city}", "flights are so expensive right now"]),
     ("B", ["yay!!", "when do we land", "ugh I know", "what airline"])],
    [("A", ["are you coming to the party {day}", "you going to the thing {day}?"]),
     ("B", ["yeah probably", "depends how I feel", "wouldn't miss it", "I'll come late"])],
    [("A", ["thank you for tonight, that was so fun", "had the best time", "thanks for dinner!!"]),
     ("B", ["same!! let's do it again soon", "anytime ❤️", "so fun", "we need to do this more"])],
    [("A", ["did you hear about the new place on the corner", "they're opening a {food} spot near me"]),
     ("B", ["no!! we have to try it", "finally", "I heard it's overpriced", "let's go {day}"])],
    [("A", ["call me when you can", "can you talk?", "free to call later?"]),
     ("B", ["calling now", "in 10?", "in a meeting, after 5?", "yep"])],
    [("A", ["good morning ☀️", "morning!", "coffee first then words"]),
     ("B", ["morning!!", "need more coffee", "why are you up so early", "gm"])],
    [("A", ["did you get my email", "sent you the doc", "check your inbox"]),
     ("B", ["got it", "looking now", "will read tonight", "which one lol I have 400 unread"])],
]

ES_TOPICS = [
    [("A", ["¿quieres comer {comida} {dia}?", "¿vamos por {comida} {dia}?", "tengo antojo de {comida}"]),
     ("B", ["¡sí!", "claro que sí", "¿a qué hora?", "no puedo {dia}, ¿otro día?"]),
     ("A", ["a las {hora}", "¿te parece a las {hora}?", "paso por ti a las {hora}"]),
     ("?B", ["perfecto", "dale", "nos vemos", "👍"])],
    [("A", ["¿cómo estás?", "¿qué tal tu semana?", "¿cómo te fue hoy?"]),
     ("B", ["bien, cansado", "todo bien, ¿y tú?", "mucho trabajo", "súper bien, gracias"]),
     ("?A", ["me alegro", "igual yo", "ánimo"])],
    [("A", ["¿hablaste con mamá?", "¿cómo sigue la abuela?", "¿cómo está tu familia?"]),
     ("B", ["sí, está mejor", "mañana la llamo", "todos bien, gracias por preguntar", "la abuela te manda saludos"])],
    [("A", ["¿viste el partido?", "qué partidazo anoche", "¿quién ganó?"]),
     ("B", ["¡increíble!", "no lo vi, ¿qué pasó?", "ganamos 2-1", "el árbitro estaba ciego"])],
    [("A", ["feliz cumpleaños 🎉", "¡feliz día!", "que tengas un día increíble"]),
     ("B", ["¡gracias!", "muchas gracias ❤️", "gracias hermano"])],
    [("A", ["llego tarde, lo siento", "ya voy en camino", "hay mucho tráfico"]),
     ("B", ["no te preocupes", "tranquilo", "aquí te espero"])],
    [("A", ["mira esto jaja", "no puedo con esto 😂", "jajaja esto eres tú"]),
     ("B", ["jajaja", "😂😂", "qué risa", "no puede ser"])],
    [("A", ["¿me pasas la receta de las {comida}?", "¿cómo se hacen las {comida}?"]),
     ("B", ["te la mando luego", "es de mi abuela, no se la des a nadie jaja", "es fácil, te llamo y te explico"])],
]

FILLERS = ["lol", "haha", "omg", "wait what", "😂", "true", "yeah", "nice", "ok", "same", "ugh", "🙌", "lmao",
           "right??", "no way", "hmm", "yes", "ha", "100%", "exactly", "wow", "❤️", "k", "sure", "totally", "oof",
           "i know", "love that", "hahaha", "for real", "yep", "nooo", "brb", "😭"]
ES_FILLERS = ["jaja", "sí", "dale", "ok", "jajaja", "claro", "qué bien", "😂", "vale", "exacto", "no manches", "👍"]
CAPTIONS = ["look at this", "view from the deck", "this guy", "", "", "", "lol", "the setup", "dinner tonight",
            "sunset was unreal", "found this in my camera roll", "", "matching!"]
EMOJI_TAIL = [" 😂", " 🙃", " ❤️", " 🎉", " 😭", " 👀", " 🙏", " ✨", " 😅", " 🔥"]

# ---------------------------------------------------------------- needles ---
# (chat, local datetime, [(sender, text)], queries). sender: 'me' or a person key.
NEEDLES = [
    ("lake", (2023, 6, 28, 18, 5), [
        ("maya", "ok logistics for the lake house this weekend"),
        ("maya", "gate code is 4471# (keypad is on the left post). wifi is LakeHouse_Guest, password loons2023"),
        ("omar", "saving this"),
        ("me", "what time is everyone getting there"),
        ("grace", "we'll be there by 4 friday")],
        ["what's the code to get in the gate", "lake house gate code", "wifi password at the lake house"]),
    ("leo", (2024, 3, 15, 20, 12), [
        ("me", "heading to Austin next month for a conference, any food recs?"),
        ("leo", "if you're in Austin you HAVE to go to Casa Lumbre on East Cesar Chavez. get the brisket tacos and the elote"),
        ("leo", "and get breakfast tacos at Pepper & Pine, go early the line gets crazy"),
        ("me", "adding both, thank you!")],
        ["restaurant recommendation in Austin", "where should I eat in Austin", "brisket tacos place"]),
    ("priya", (2025, 11, 2, 11, 40), [
        ("priya", "SAVE THE DATE!! Dev and I are getting married June 13, 2026 in Santa Barbara 💍"),
        ("me", "AHHH congratulations!!! 😭❤️"),
        ("priya", "room block is at the Hotel Marisol, book by May 1 with code SHAHRAO26"),
        ("me", "booking tonight")],
        ["when is Priya's wedding", "wedding hotel room block code", "what hotel for the wedding"]),
    ("mom", (2024, 11, 26, 9, 15), [
        ("mom", "I land Thursday at 6:40pm, United 1423 from Denver"),
        ("me", "I'll pick you up at arrivals"),
        ("mom", "Thank you sweetheart. I'll have one bag")],
        ["what flight is mom on", "when does mom land", "UA 1423"]),
    ("ben", (2025, 12, 19, 22, 3), [
        ("me", "my flight is DL 2210, gets into LGA at 11:05 tomorrow"),
        ("ben", "cool i'll grab you, text me when you land"),
        ("ben", "garage code at my building is 8812 btw if I'm not down yet")],
        ["what's Ben's garage code", "which flight am I taking to LGA"]),
    ("jonah", (2022, 8, 7, 14, 30), [
        ("jonah", "we moved! new place is 1180 Alder Street, Apt 3B, Portland OR 97205"),
        ("me", "congrats!! sending a housewarming thing"),
        ("jonah", "you're the best")],
        ["where did Jonah move", "Jonah's new address", "address in Portland"]),
    ("family", (2024, 3, 2, 10, 0), [
        ("mom", "Reminder: Grandpa's birthday is March 9th. Card is on the counter, everyone sign it"),
        ("dad", "Got it"),
        ("ben", "will sign when I'm over sunday")],
        ["when is grandpa's birthday", "birthday card reminder"]),
    ("ava", (2025, 4, 18, 8, 50), [
        ("ava", "you can park in my spot while I'm away, it's #214 on level P2"),
        ("me", "amazing thank you")],
        ["what's Ava's parking spot number", "where can I park"]),
    ("books", (2025, 1, 12, 19, 30), [
        ("nora", "next pick: The Glass Orchard https://books.example/the-glass-orchard"),
        ("tessa", "ooh I've heard good things"),
        ("priya", "meeting Feb 9 at mine?")],
        ["what's the next book club book", "book club meeting"]),
    ("sam", (2022, 10, 21, 23, 10), [
        ("sam", "venmo me $42.50 for the concert tickets when you get a sec"),
        ("me", "sent!")],
        ["how much do I owe Sam for tickets", "concert tickets money"]),
    ("diego", (2024, 5, 6, 17, 20), [
        ("diego", "la fiesta de cumpleaños de la abuela es el sábado 11 a las 7, en su casa en Calle Olmo 45"),
        ("me", "¡ahí estaré! ¿llevo algo?"),
        ("diego", "trae el flan, a ella le encanta")],
        ["grandma's birthday party", "fiesta de cumpleaños de la abuela", "what should I bring to the party"]),
    ("grace", (2023, 2, 13, 12, 5), [
        ("grace", "Biscuit's vet appointment is Tuesday at 9:15 at Riverside Animal Clinic, can you drive us?"),
        ("me", "yes! I'll pick you up at 8:50")],
        ["when is the vet appointment", "dog doctor appointment"]),
    ("ray", (2023, 9, 11, 9, 2), [
        ("ray", "Hi, this is Ray. The plumber will come by Wednesday between 10 and 12 to fix the kitchen sink."),
        ("me", "Thanks Ray, I'll be home")],
        ["when is the plumber coming", "kitchen sink repair"]),
    ("maya", (2021, 10, 30, 21, 45), [
        ("maya", "my new number at work is 212-555-0199 if you ever need it"),
        ("me", "saved")],
        ["Maya's work phone number"]),
]

TAPBACKS = [(2000, "Loved", "Removed a heart from"), (2001, "Liked", "Removed a like from"),
            (2002, "Disliked", "Removed a dislike from"), (2003, "Laughed at", "Removed a laugh from"),
            (2004, "Emphasized", "Removed an exclamation from"), (2005, "Questioned", "Removed a question mark from")]
TAPBACK_WEIGHTS = [40, 20, 2, 25, 8, 5]
CUSTOM_EMOJI = ["🎉", "🔥", "🥹", "👀", "💯", "🙏", "😮"]


# ------------------------------------------------------------------ helpers ---
def apple_ns(when: dt.datetime) -> int:
    return int((when - APPLE_EPOCH).total_seconds()) * 1_000_000_000 + when.microsecond * 1000


def new_guid(rng: random.Random) -> str:
    h = "%032X" % rng.getrandbits(128)
    return f"{h[:8]}-{h[8:12]}-4{h[13:16]}-{h[16:20]}-{h[20:]}"


def utf16_len(s: str) -> int:
    return len(s.encode("utf-16-le")) // 2


def styled(rng, text, person_key):
    if person_key is None or person_key == "me":
        style = dict(lower=0.4, emoji=0.1)
    else:
        style = PEOPLE[person_key][3]
    if rng.random() < style["lower"] and not text.startswith("http"):
        text = text[:1].lower() + text[1:]
    if rng.random() < style["emoji"] * 0.4 and len(text) > 8:
        text += rng.choice(EMOJI_TAIL)
    return text


def fill_slots(rng, lang):
    if lang == "es":
        return {k: rng.choice(v) for k, v in ES.items()}
    slots = {k: (rng.choice(v) if isinstance(v, list) else v) for k, v in S.items()}
    g = rng.sample(S["grocery"], 3)
    slots.update(grocery=g[0], grocery2=g[1], grocery3=g[2])
    return slots


def year_month(ym, end=False):
    y, m = ym
    if end:
        m += 1
        if m > 12:
            y, m = y + 1, 1
        return dt.datetime(y, m, 1, tzinfo=TZ) - dt.timedelta(days=1)
    return dt.datetime(y, m, 1, tzinfo=TZ)


def clamp_to_waking(rng, t: dt.datetime) -> dt.datetime:
    # People mostly text 7:30am-1am local.
    if 1 <= t.hour < 7:
        t = t.replace(hour=rng.choice([7, 8, 8, 9, 10, 12]), minute=rng.randrange(60))
    return t


# --------------------------------------------------------------- generation ---
class Gen:
    def __init__(self, seed: int, scale: float, end_date: dt.datetime):
        self.rng = random.Random(seed)
        self.scale = scale
        self.end_date = end_date
        self.msgs: list[dict] = []  # every row in `message`, unsorted
        self.needles_out = []

    # -- a message row before ROWIDs exist
    def add(self, chat, when, sender, text, **kw):
        m = dict(chat=chat["key"], when=when, sender=sender, text=text, guid=new_guid(self.rng),
                 service=chat.get("service", "iMessage"), attach=[], links=[], **kw)
        self.msgs.append(m)
        return m

    def attachment_msg(self, chat, when, sender, lang):
        r = self.rng
        roll = r.random()
        n_img = 1
        if roll < 0.68:
            kind = "image"
            n_img = 1 if r.random() < 0.85 else r.randint(2, 4)
        elif roll < 0.80:
            kind = "video"
        elif roll < 0.93:
            kind = "audio"
        else:
            kind = "pdf"
        caption = "" if kind == "audio" else r.choice(CAPTIONS if lang != "es" else ["mira", "", "", "jaja"])
        count = n_img if kind == "image" else 1
        text = OBJ * count + caption
        m = self.add(chat, when, sender, text, is_audio=(kind == "audio"))
        for i in range(count):
            m["attach"].append(self.make_attachment(kind, m, i, when, sender))
        return m

    def make_attachment(self, kind, msg, idx, when, sender):
        r = self.rng
        n = r.randint(1000, 9999)
        if kind == "image":
            ext, mime, uti, name, size = ("HEIC", "image/heic", "public.heic", f"IMG_{n}.HEIC", r.randint(900_000, 4_500_000))
            if r.random() < 0.15:
                ext, mime, uti, name, size = ("png", "image/png", "public.png", f"Screenshot {when:%Y-%m-%d} at {when:%H.%M.%S}.png", r.randint(200_000, 2_000_000))
            elif r.random() < 0.1:
                ext, mime, uti, name, size = ("jpeg", "image/jpeg", "public.jpeg", f"IMG_{n}.jpeg", r.randint(150_000, 900_000))
        elif kind == "video":
            mime, uti, name, size = "video/quicktime", "com.apple.quicktime-movie", f"IMG_{n}.MOV", r.randint(3_000_000, 60_000_000)
        elif kind == "audio":
            mime, uti, name, size = "audio/x-caf", "com.apple.coreaudio-format", "Audio Message.caf", r.randint(20_000, 400_000)
        else:
            name = r.choice(["Itinerary.pdf", "lease_2023.pdf", "Boarding Pass.pdf", "menu.pdf", "receipt.pdf", "RSVP card.pdf"])
            mime, uti, size = "application/pdf", "com.adobe.pdf", r.randint(40_000, 900_000)
        a_guid = f"at_{idx}_{msg['guid']}"
        folder = "%02x/%02d" % (r.randrange(256), r.randrange(100))
        return dict(guid=a_guid, filename=f"~/Library/Messages/Attachments/{folder}/{a_guid}/{name}", mime=mime, uti=uti,
                    transfer_name=name, bytes=size, outgoing=sender == "me", created=when)

    def link_msg(self, chat, when, sender):
        r = self.rng
        url = r.choice([
            "https://recipes.example.com/lemon-garlic-pasta", "https://tickets.example.org/event/%d" % r.randint(10000, 99999),
            "https://maps.example.com/?q=%s" % r.choice(["Little+Fern", "Juniper+Hall", "Two+Birds+Diner"]),
            "https://news.example.net/%d/%02d/city-council-votes-on-bike-lanes" % (when.year, when.month),
            "https://www.stays.example/rooms/%d" % r.randint(1000000, 9999999),
            "https://video.example.com/watch?v=%s" % "".join(r.choice("abcdefghijkXYZ0123456789") for _ in range(11)),
            "https://shop.example.com/products/ceramic-mug-set", "https://blog.example.org/best-hikes-near-the-lake",
        ])
        m = self.add(chat, when, sender, url)
        m["links"].append((0, utf16_len(url), url))
        # Rich previews (URLBalloonProvider) since iOS 10; SMS rows never have them.
        if chat.get("service", "iMessage") == "iMessage" and r.random() < 0.8:
            m["balloon"] = "com.apple.messages.URLBalloonProvider"
            m["payload"] = link_payload(url, r)
        return m

    def run_chat(self, chat, target):
        r = self.rng
        lang = chat.get("lang", "en")
        topics = ES_TOPICS if lang == "es" else EN_TOPICS
        fillers = ES_FILLERS if lang == "es" else FILLERS
        members = chat["members"]
        start = year_month(chat["start"])
        end = min(year_month(chat["end"], end=True), self.end_date)
        span = (end - start).total_seconds()
        per_burst = 9.0  # running estimate, so the history spans the whole range
        mean_gap = span / max(1.0, target / per_burst)
        t = start + dt.timedelta(seconds=r.expovariate(1 / mean_gap) * 0.3)
        produced = bursts = 0
        while produced < target and t < end:
            t = clamp_to_waking(r, t)
            produced += self.burst(chat, t, topics, fillers, members, lang)
            bursts += 1
            per_burst = produced / bursts
            left = max(1.0, (target - produced) / per_burst)
            mean_gap = max(600.0, (end - t).total_seconds() / left)
            t = t + dt.timedelta(seconds=r.expovariate(1 / mean_gap))
            t = t.replace(hour=r.choice([8, 9, 11, 12, 13, 15, 17, 18, 19, 20, 20, 21, 21, 22, 23]),
                          minute=r.randrange(60), second=r.randrange(60)) if r.random() < 0.7 else t

    def burst(self, chat, t, topics, fillers, members, lang):
        r = self.rng
        n0 = len(self.msgs)
        is_group = len(members) > 1
        for _ in range(r.choice([1, 1, 1, 2, 2, 3])):
            topic = r.choice(topics)
            slots = fill_slots(r, lang)
            them = r.choice(members)
            me_first = r.random() < 0.45
            roles = {"A": "me" if me_first else them, "B": them if me_first else "me"}
            for role, alts in topic:
                opt = role.startswith("?")
                role = role.lstrip("?")
                if opt and r.random() < 0.5:
                    continue
                if role == "C" or (is_group and roles[role] != "me" and r.random() < 0.4):
                    who = r.choice(members)
                else:
                    who = roles[role]
                text = styled(r, r.choice(alts).format(**slots), who)
                t = self.step(t, same=False)
                self.add(chat, t, who, text)
                # double-texting and reactions in between
                while r.random() < 0.18:
                    t = self.step(t, same=True)
                    self.add(chat, t, who, styled(r, r.choice(fillers), who))
                if r.random() < 0.22:
                    other = "me" if who != "me" else r.choice(members)
                    t = self.step(t, same=False)
                    self.add(chat, t, other, r.choice(fillers))
                if r.random() < 0.045:
                    t = self.step(t, same=True)
                    self.attachment_msg(chat, t, who, lang)
                if r.random() < 0.012 and lang != "es":
                    t = self.step(t, same=True)
                    self.link_msg(chat, t, who)
            if r.random() < 0.12:
                # a pause inside the conversation, sometimes past the 45-minute window gap
                t = t + dt.timedelta(minutes=r.choice([20, 35, 50, 70, 110]))
        return len(self.msgs) - n0

    def step(self, t, same):
        r = self.rng
        if same:
            secs = r.uniform(2, 40)
        else:
            secs = r.choice([r.uniform(8, 90), r.uniform(30, 400), r.uniform(60, 900)])
        return t + dt.timedelta(seconds=secs)

    def plant_needles(self, chats_by_key):
        for chat_key, (y, mo, d, h, mi), lines, queries in NEEDLES:
            chat = chats_by_key[chat_key]
            t = dt.datetime(y, mo, d, h, mi, tzinfo=TZ)
            planted = []
            for who, text in lines:
                t = t + dt.timedelta(seconds=self.rng.uniform(15, 150))
                if text.startswith("next pick"):
                    m = self.add(chat, t, who, text)
                    url = "https://books.example/the-glass-orchard"
                    i = text.index(url)
                    m["links"].append((utf16_len(text[:i]), utf16_len(url), url))
                else:
                    m = self.add(chat, t, who, text)
                m["needle"] = True
                planted.append(m)
            self.needles_out.append(dict(chat=chat_key, messages=planted, queries=queries))

    def system_events(self, chats_by_key):
        # Group creation/rename/join rows (item_type 1 = participant change, 2 = rename).
        for key in ("lake", "books"):
            chat = chats_by_key[key]
            t0 = year_month(chat["start"]) + dt.timedelta(hours=18)
            creator = chat["members"][0]
            self.add(chat, t0, creator, None, item_type=2, group_title=chat["name"])
            joiner = chat["members"][-1]
            self.add(chat, t0 + dt.timedelta(days=3), creator, None, item_type=1, group_action_type=0,
                     other_handle_person=joiner)
        chat = chats_by_key["family"]
        self.add(chat, dt.datetime(2021, 12, 24, 19, 0, tzinfo=TZ), "ben", None, item_type=2, group_title="Parkers 🎄")
        self.add(chat, dt.datetime(2022, 1, 2, 10, 0, tzinfo=TZ), "mom", None, item_type=2, group_title=None)

    # -- post-processing over the sorted timeline
    def decorate(self):
        r = self.rng
        by_chat: dict[str, list[dict]] = {}
        for m in self.msgs:
            by_chat.setdefault(m["chat"], []).append(m)
        extra = []
        edit_era = dt.datetime(2022, 10, 24, tzinfo=TZ)   # iOS 16 / Ventura: edit + unsend
        reply_era = dt.datetime(2020, 9, 16, tzinfo=TZ)   # iOS 14: inline replies
        emoji_era = dt.datetime(2024, 9, 16, tzinfo=TZ)   # iOS 18: any-emoji tapbacks
        for key, msgs in by_chat.items():
            msgs.sort(key=lambda m: m["when"])
            members = [m for m in sorted({x["sender"] for x in msgs}) if m != "me"]  # sorted: set order varies per run
            for i, m in enumerate(msgs):
                if m.get("item_type") or m["text"] is None:
                    continue
                # tapbacks
                p = 0.10 if m["attach"] else 0.055
                if m.get("needle"):
                    p = 0.25
                if r.random() < p:
                    reactor = "me" if m["sender"] != "me" else r.choice(members)
                    if m["sender"] != "me" and len(members) > 1 and r.random() < 0.5:
                        reactor = r.choice([x for x in members if x != m["sender"]])
                    when = m["when"] + dt.timedelta(seconds=r.choice([r.uniform(3, 60), r.uniform(60, 3600)]))
                    if m["when"] >= emoji_era and r.random() < 0.15:
                        kind, verb, emoji = 2006, None, r.choice(CUSTOM_EMOJI)
                        undo_verb = None
                    else:
                        (kind, verb, undo_verb), emoji = r.choices(TAPBACKS, TAPBACK_WEIGHTS)[0], None
                    extra.append(self.tapback(m, reactor, when, kind, verb, emoji))
                    if r.random() < 0.08:
                        undo_when = when + dt.timedelta(seconds=r.uniform(2, 300))
                        extra.append(self.tapback(m, reactor, undo_when, kind + 1000, undo_verb, emoji, removal=True))
                # threaded replies
                if m["when"] >= reply_era and i > 3 and not m["attach"] and not m.get("needle") and r.random() < 0.012:
                    self.make_reply(m, msgs[i - r.randint(2, min(i, 12))])
                # edits / unsends of my own messages
                if self.editable(m, edit_era):
                    roll = r.random()
                    if roll < 0.006:
                        self.make_edit(m)
                    elif roll < 0.008:
                        self.make_unsent(m)
                # occasional SMS fallback inside iMessage chats (pre-2023, 1:1 only)
                if m["service"] == "iMessage" and len(members) == 1 and m["when"].year < 2023 and r.random() < 0.02:
                    m["service"] = "SMS"
                    m.pop("balloon", None)
                    m.pop("payload", None)
        self.msgs.extend(extra)

        # Small fixtures still get at least a few of every rare shape.
        mine = sorted((m for m in self.msgs if self.editable(m, edit_era)), key=lambda m: m["when"])
        mine = [m for m in mine if not m.get("thread") and not m.get("edited_from") and m["text"] is not None]
        for _ in range(3 - sum(1 for m in self.msgs if m.get("edited_from") is not None)):
            self.make_edit(mine.pop(r.randrange(len(mine))))
        for _ in range(2 - sum(1 for m in self.msgs if m.get("unsent_text") is not None)):
            self.make_unsent(mine.pop(r.randrange(len(mine))))
        for _ in range(4 - sum(1 for m in self.msgs if m.get("thread"))):
            for _attempt in range(50):
                msgs = by_chat[r.choice(list(by_chat))]
                i = r.randrange(4, len(msgs))
                m, target = msgs[i], msgs[i - r.randint(1, 3)]
                if m["when"] >= reply_era and not m["attach"] and m["text"] and self.make_reply(m, target):
                    break
        have = {k for m in self.msgs for k in [m.get("assoc_type")]}
        pool = [m for m in self.msgs if m["text"] and not m.get("item_type") and not m.get("assoc_type")
                and m["when"] >= emoji_era]
        for kind in (2006, 3006, 3000):
            if kind not in have:
                target = r.choice(pool)
                reactor = "me" if target["sender"] != "me" else next(x for x in sorted({y["sender"] for y in by_chat[target["chat"]]}) if x != "me")
                emoji = r.choice(CUSTOM_EMOJI) if kind != 3000 else None
                verb = "Loved" if kind == 3000 else None
                t = target["when"] + dt.timedelta(seconds=40)
                if kind != 2006:
                    self.msgs.append(self.tapback(target, reactor, t, kind - 1000, verb, emoji))
                    t += dt.timedelta(seconds=30)
                    verb = "Removed a heart from" if kind == 3000 else None
                self.msgs.append(self.tapback(target, reactor, t, kind, verb, emoji, removal=kind >= 3000))
                have.add(kind)

    @staticmethod
    def editable(m, edit_era):
        return (m["sender"] == "me" and m["when"] >= edit_era and not m["attach"] and not m.get("needle")
                and not m.get("balloon") and not m.get("item_type") and not m.get("assoc_type") and m["text"] is not None)

    def make_edit(self, m):
        m["edited_from"] = m["text"]
        m["text"] = edit_text(self.rng, m["text"])
        m["date_edited"] = m["when"] + dt.timedelta(seconds=self.rng.uniform(10, 600))

    def make_unsent(self, m):
        m["unsent_text"] = m["text"]
        m["text"] = None
        m["date_edited"] = m["when"] + dt.timedelta(seconds=self.rng.uniform(5, 110))

    def make_reply(self, m, target):
        if target["text"] and not target.get("item_type") and not target.get("assoc_type") and target["when"] < m["when"]:
            m["thread"] = target["guid"]
            m["thread_part"] = "0:0:%d" % utf16_len(target["text"])
            return True
        return False

    def tapback(self, target, reactor, when, kind, verb, emoji, removal=False):
        if target["attach"] and not target["text"].strip(OBJ):
            kind = target["attach"][0]["mime"].split("/")[0]
            noun = {"image": "an image", "video": "a movie", "audio": "an audio message"}.get(kind, "an attachment")
            quoted = noun
        else:
            body = target["text"].replace(OBJ, "")
            quoted = "“%s”" % (body if len(body) <= 120 else body[:117] + "…")
        if kind in (2006, 3006):
            text = ("Removed %s from %s" if removal else "Reacted %s to %s") % (emoji, quoted)
        else:
            text = "%s %s" % (verb, quoted)
        m = dict(chat=target["chat"], when=when, sender=reactor, text=text, guid=new_guid(self.rng),
                 service=target["service"], attach=[], links=[],
                 assoc_guid="p:0/%s" % target["guid"], assoc_type=kind, assoc_emoji=emoji,
                 assoc_len=utf16_len(target["text"] or ""))
        return m


def edit_text(r, text):
    return text + r.choice([" tomorrow*", ", nvm 7 works", "!", " actually", "*", " (not friday)"])


def link_payload(url, r):
    """A minimal NSKeyedArchiver plist of LPLinkMetadata, like URL previews carry."""
    from plistlib import UID
    host = url.split("/")[2]
    title = r.choice(["Lemon Garlic Pasta", "Tickets", "Map", "City council votes on bike lanes", "Lakeside cabin",
                      "Video", "Ceramic Mug Set", "The best hikes near the lake"])
    objects = [
        "$null",
        {"metadata": UID(2), "hasCompletedFetch": True, "hasFetchedSubresources": True, "$class": UID(8)},
        {"URL": UID(3), "originalURL": UID(3), "title": UID(5), "siteName": UID(6), "version": 1, "$class": UID(7)},
        {"NS.base": UID(0), "NS.relative": UID(4), "$class": UID(9)},
        url, title, host,
        {"$classes": ["LPLinkMetadata", "NSObject"], "$classname": "LPLinkMetadata"},
        {"$classes": ["LPSharingMetadataWrapper", "NSObject"], "$classname": "LPSharingMetadataWrapper"},
        {"$classes": ["NSURL", "NSObject"], "$classname": "NSURL"},
    ]
    return plistlib.dumps({"$version": 100000, "$archiver": "NSKeyedArchiver", "$top": {"root": UID(1)},
                           "$objects": objects}, fmt=plistlib.FMT_BINARY)


# --------------------------------------------------------------- typedstream ---
def ensure_helper():
    if sys.platform != "darwin":
        sys.exit("make-fixture.py needs macOS (Foundation's NSArchiver builds the attributedBody blobs)")
    if not os.path.exists(HELPER_BIN) or os.path.getmtime(HELPER_BIN) < os.path.getmtime(HELPER_SRC):
        os.makedirs(os.path.dirname(HELPER_BIN), exist_ok=True)
        subprocess.run(["swiftc", "-O", "-suppress-warnings", HELPER_SRC, "-o", HELPER_BIN], check=True)
    return HELPER_BIN


def archive_bodies(requests):
    """requests: list of dicts for the Swift helper -> list of bytes (same order)."""
    if not requests:
        return []
    payload = "".join(json.dumps(q, ensure_ascii=False) + "\n" for q in requests).encode()
    out = subprocess.run([ensure_helper()], input=payload, stdout=subprocess.PIPE, check=True).stdout
    lines = out.decode().splitlines()
    if len(lines) != len(requests):
        raise RuntimeError(f"helper returned {len(lines)} blobs for {len(requests)} requests")
    return [base64.b64decode(l) for l in lines]


# --------------------------------------------------------------------- write ---
def find_template():
    pattern = os.path.expanduser("~/.cargo/registry/src/*/imessage-database-4.3.0/test_data/db/test.db")
    hits = sorted(glob.glob(pattern))
    if not hits:
        sys.exit("imessage-database 4.3.0 is not in the cargo registry; run `cargo fetch` in the workspace first")
    return hits[0]


def empty_db(path):
    for suffix in ("", "-wal", "-shm", "-journal"):
        if os.path.exists(path + suffix):
            os.remove(path + suffix)
    shutil.copyfile(find_template(), path)
    os.chmod(path, 0o644)
    db = sqlite3.connect(path)
    # The template ships a few sample rows; its delete triggers call functions
    # that only exist inside Messages, so register no-ops while clearing it.
    for name, n in (("delete_attachment_path", 1), ("before_delete_attachment_path", 2), ("after_delete_message_plugin", 2)):
        db.create_function(name, n, lambda *a: None)
    tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
    for t in ("message_attachment_join", "chat_message_join", "attachment", "message", *tables):
        db.execute(f"DELETE FROM {t}")
    db.execute("DELETE FROM sqlite_sequence")
    db.commit()
    return db


def write_db(path, gen: Gen, chats):
    r = random.Random(gen.rng.random())
    db = empty_db(path)
    msgs = sorted(gen.msgs, key=lambda m: (m["when"], m["guid"]))

    # handles: phone iMessage for everyone, plus emails, plus SMS twins where used
    handle_ids = {}

    def handle(address, service):
        k = (address, service)
        if k not in handle_ids:
            cur = db.execute("INSERT INTO handle(id, country, service, uncanonicalized_id) VALUES (?,?,?,NULL)",
                             (address, "us", service))
            handle_ids[k] = cur.lastrowid
        return handle_ids[k]

    for key, (_, phone, email, _) in PEOPLE.items():
        handle(phone, "iMessage")
    for key, (_, phone, email, _) in PEOPLE.items():
        if email:
            handle(email, "iMessage")

    def person_handle(person, chat, service):
        _, phone, email, _ = PEOPLE[person]
        if (chat.get("via_email") and person == chat["key"]) or person in chat.get("email_for", ()):
            return handle(email, "iMessage")
        return handle(phone, service)

    account_guid = new_guid(r)
    chat_ids = {}
    for c in chats:
        service = c.get("service", "iMessage")
        is_group = c.get("name") is not None
        if is_group:
            ident = "chat%018d" % r.randrange(10**17, 10**18)
            guid = f"{service};+;{ident}"
            style, room, display = 43, ident, c["name"]
        else:
            p = c["members"][0]
            addr = PEOPLE[p][2] if c.get("via_email") else PEOPLE[p][1]
            ident, guid, style, room, display = addr, f"{service};-;{addr}", 45, None, ""
        cur = db.execute(
            "INSERT INTO chat(guid, style, state, account_id, chat_identifier, service_name, room_name, account_login,"
            " is_archived, last_addressed_handle, display_name, group_id) VALUES (?,?,3,?,?,?,?,?,0,?,?,?)",
            (guid, style, account_guid, ident, service, room, f"E:{ME_EMAIL}", ME_PHONE, display, new_guid(r)))
        chat_ids[c["key"]] = cur.lastrowid
        for p in c["members"]:
            db.execute("INSERT INTO chat_handle_join(chat_id, handle_id) VALUES (?,?)",
                       (cur.lastrowid, person_handle(p, c, service)))
    chats_by_key = {c["key"]: c for c in chats}

    # attributedBody decisions: the oldest 30% keep plain text.
    dated = [m for m in msgs if m["text"] is not None or m.get("unsent_text")]
    cutoff = dated[int(len(dated) * 0.30)]["when"] if dated else None
    requests, owners = [], []
    for m in msgs:
        text = m["text"]
        m["body"] = None
        m["store_text"] = text
        if text is None:
            continue
        old = m["when"] < cutoff
        needs_blob = bool(m["attach"]) or bool(m.get("balloon"))
        if old and not needs_blob:
            if r.random() < 1 / 3:
                requests.append(body_request(m)); owners.append((m, "body"))
        else:
            if not old:
                m["store_text"] = None
            requests.append(body_request(m)); owners.append((m, "body"))
        if m.get("edited_from") is not None:
            requests.append({"t": m["edited_from"], "plain": True}); owners.append((m, "edit0"))
            requests.append({"t": m["text"], "plain": True}); owners.append((m, "edit1"))
    blobs = archive_bodies(requests)
    for (m, slot), blob in zip(owners, blobs):
        m[slot] = blob

    attach_rows = 0
    for m in msgs:
        chat = chats_by_key[m["chat"]]
        is_group = chat.get("name") is not None
        from_me = m["sender"] == "me"
        service = m["service"]
        if from_me:
            if is_group:
                hid = 0
            else:
                hid = person_handle(chat["members"][0], chat, service) if r.random() < 0.85 else 0
        else:
            hid = person_handle(m["sender"], chat, service)
        date = apple_ns(m["when"])
        read = date + int(r.uniform(5, 3600) * 1e9) if not from_me else 0
        delivered = date + int(r.uniform(0.2, 3) * 1e9) if from_me else 0
        summary = None
        if m.get("edited_from") is not None:
            t0 = (m["when"] - APPLE_EPOCH).total_seconds()
            t1 = (m["date_edited"] - APPLE_EPOCH).total_seconds()
            summary = plistlib.dumps({
                "amc": 0, "ust": True, "ep": [0],
                "otr": {"0": {"lo": 0, "le": utf16_len(m["text"])}},
                "ec": {"0": [{"d": t0, "t": m["edit0"]}, {"d": t1, "t": m["edit1"]}]},
            }, fmt=plistlib.FMT_BINARY)
        elif m.get("unsent_text") is not None:
            summary = plistlib.dumps({"amc": 0, "ust": True, "rp": [0],
                                      "otr": {"0": {"lo": 0, "le": utf16_len(m["unsent_text"])}}},
                                     fmt=plistlib.FMT_BINARY)
        date_edited = apple_ns(m["date_edited"]) if m.get("date_edited") else 0
        date_retracted = date_edited if m.get("unsent_text") is not None else 0
        other_handle = person_handle(m["other_handle_person"], chat, service) if m.get("other_handle_person") else 0
        cur = db.execute(
            """INSERT INTO message(guid, text, handle_id, service, account, account_guid, date, date_read, date_delivered,
                 is_delivered, is_finished, is_from_me, is_read, is_sent, attributedBody, item_type, group_title,
                 group_action_type, other_handle, associated_message_guid, associated_message_type,
                 associated_message_emoji, associated_message_range_location, associated_message_range_length,
                 balloon_bundle_id, payload_data, thread_originator_guid, thread_originator_part, reply_to_guid,
                 date_edited, date_retracted, message_summary_info, is_audio_message, destination_caller_id,
                 part_count, version, ck_sync_state)
               VALUES (?,?,?,?,?,?,?,?,?, 1,1,?,?,?, ?,?,?, ?,?,?,?, ?,?,?, ?,?,?,?,?, ?,?,?,?,?, ?,10,1)""",
            (m["guid"], m["store_text"], hid, service,
             (f"E:{ME_EMAIL}" if service == "iMessage" else f"P:{ME_PHONE}"), account_guid,
             date, read, delivered,
             int(from_me), int(not from_me), int(from_me),
             m.get("body"), m.get("item_type", 0), m.get("group_title"),
             m.get("group_action_type", 0), other_handle, m.get("assoc_guid"), m.get("assoc_type", 0),
             m.get("assoc_emoji"), 0, m.get("assoc_len", 0),
             m.get("balloon"), m.get("payload"), m.get("thread"), m.get("thread_part"), m.get("thread"),
             date_edited, date_retracted, summary, int(bool(m.get("is_audio"))), ME_PHONE,
             max(1, len(m["attach"]) + (1 if (m["text"] or "").replace(OBJ, "") else 0))))
        m["rowid"] = cur.lastrowid
        db.execute("INSERT INTO chat_message_join(chat_id, message_id, message_date) VALUES (?,?,?)",
                   (chat_ids[m["chat"]], m["rowid"], date))
        for a in m["attach"]:
            created = int((a["created"] - APPLE_EPOCH).total_seconds())
            cur = db.execute(
                "INSERT INTO attachment(guid, created_date, start_date, filename, uti, mime_type, transfer_state,"
                " is_outgoing, transfer_name, total_bytes, is_sticker, hide_attachment, original_guid, ck_sync_state)"
                " VALUES (?,?,?,?,?,?,5,?,?,?,0,0,?,1)",
                (a["guid"], created, created, a["filename"], a["uti"], a["mime"], int(a["outgoing"]),
                 a["transfer_name"], a["bytes"], a["guid"]))
            db.execute("INSERT INTO message_attachment_join(message_id, attachment_id) VALUES (?,?)",
                       (m["rowid"], cur.lastrowid))
            attach_rows += 1
    db.commit()
    db.execute("VACUUM")
    db.close()
    return msgs, handle_ids, attach_rows


def body_request(m):
    q = {"t": m["text"]}
    if m["attach"]:
        q["a"] = [a["guid"] for a in m["attach"]]
    if m["links"]:
        q["links"] = [list(l) for l in m["links"]]
    return q


def build(out_path, seed, scale):
    end_date = dt.datetime(2026, 9, 20, 23, 0, tzinfo=TZ)
    gen = Gen(seed, scale, end_date)
    chats = CHATS
    total_weight = sum(c["weight"] for c in chats)
    base_total = 24_600 * scale
    for c in chats:
        gen.run_chat(c, int(base_total * c["weight"] / total_weight))
    chats_by_key = {c["key"]: c for c in chats}
    gen.plant_needles(chats_by_key)
    gen.system_events(chats_by_key)
    gen.decorate()
    msgs, handle_ids, attach_rows = write_db(out_path, gen, chats)

    needles = []
    for n in gen.needles_out:
        needles.append(dict(chat=n["chat"], queries=n["queries"],
                            messages=[dict(rowid=m["rowid"], guid=m["guid"], text=m["text"]) for m in n["messages"]]))
    contacts = {}
    for key, (name, phone, email, _) in PEOPLE.items():
        contacts[phone] = name
        if email:
            contacts[email] = name
    manifest = dict(
        generator="scripts/make-fixture.py", seed=seed, me=dict(phone=ME_PHONE, email=ME_EMAIL),
        contacts=contacts, needles=needles,
        chats={c["key"]: dict(name=c.get("name"), members=[PEOPLE[p][0] for p in c["members"]],
                              service=c.get("service", "iMessage"), language=c.get("lang", "en")) for c in chats},
    )
    with open(os.path.splitext(out_path)[0] + ".needles.json", "w") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=1)
    stats(out_path, attach_rows)


def stats(path, attach_rows):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    q = lambda sql: db.execute(sql).fetchone()[0]
    total = q("SELECT count(*) FROM message")
    print(f"{os.path.relpath(path, ROOT)}: {total} messages, {q('SELECT count(*) FROM chat')} chats, "
          f"{q('SELECT count(*) FROM handle')} handles, {attach_rows} attachments, "
          f"{q('SELECT count(*) FROM message WHERE associated_message_type BETWEEN 2000 AND 3006')} tapbacks, "
          f"{q('SELECT count(*) FROM message WHERE text IS NULL AND attributedBody IS NOT NULL')} body-only "
          f"({100 * q('SELECT count(*) FROM message WHERE text IS NULL AND attributedBody IS NOT NULL') / total:.0f}%), "
          f"{q('SELECT count(*) FROM message WHERE thread_originator_guid IS NOT NULL')} replies, "
          f"{q('SELECT count(*) FROM message WHERE date_edited > 0 AND date_retracted = 0')} edited, "
          f"{q('SELECT count(*) FROM message WHERE date_retracted > 0')} unsent, "
          f"{os.path.getsize(path) / 1e6:.1f} MB")
    db.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", type=int, default=20190101)
    ap.add_argument("--out-dir", default=os.path.join(ROOT, "fixtures"))
    ap.add_argument("--only", choices=["full", "small"], help="build just one of the two databases")
    args = ap.parse_args()
    os.makedirs(args.out_dir, exist_ok=True)
    if args.only in (None, "full"):
        build(os.path.join(args.out_dir, "chat.db"), args.seed, 1.0)
    if args.only in (None, "small"):
        build(os.path.join(args.out_dir, "chat-small.db"), args.seed, 0.02)


if __name__ == "__main__":
    main()
