// The film, as cues on a clock (seconds after the key press, which is tape time). Cues fire once,
// in order, as the clock passes them; spans run every frame between two times. Picture beats are
// pinned to the words of the tape (tape.js), so the cut follows the voice exactly. Everything here
// only calls the world's verbs, so the same timeline drives WebGPU, Canvas 2D, capture and reduced
// motion.
import { easeInOut, smooth, clamp, mix } from "./springs.js"
import { BAR_H, settingsGeom } from "./content.js"
import { WORDS, TAPE } from "./tape.js"
import { ALBUM, albumGrid, scrollOn } from "./library.js"
import { walkerAt } from "./feeds.js"
import { sitePageGeom } from "./sitepage.js"

// this cut has no visitor's turn: Control+1/2/3 never take over the film
export const HOLD = [-10, -9]

const norm = (s) => s.toLowerCase().replace(/[^a-z0-9']/g, "")
const starts = (phrase) => {
  const ws = phrase.split(/\s+/).map(norm)
  const out = []
  for (let i = 0; i < WORDS.length; i++) if (ws.every((x, j) => WORDS[i + j] && norm(WORDS[i + j][0]) === x)) out.push(WORDS[i][1])
  return out
}
// When a phrase starts on the tape. Lookups read the tape in order: each takes the next match within
// half a minute of the previous lookup, else the nearest one before it, so a repeated phrase ("I
// closed two", "Side by side") resolves to the right occurrence without hard-coded times. A numeric
// `after` takes the first match from that time instead.
let cursor = 0
export function word(phrase, after) {
  const all = starts(phrase)
  if (after !== undefined) {
    const t = all.find((t) => t >= after)
    if (t === undefined) throw new Error("tape has no " + phrase + " after " + after)
    return t
  }
  const next = all.find((t) => t >= cursor - 1.5 && t < cursor + 30)
  if (next !== undefined) {
    cursor = Math.max(cursor, next)
    return next
  }
  const prev = all.filter((t) => t < cursor).at(-1)
  if (prev === undefined) throw new Error("tape has no " + phrase)
  return prev
}
// when a phrase ends
export function wordEnd(phrase, after) {
  const t = word(phrase, after)
  const n = phrase.split(/\s+/).length
  const i = WORDS.findIndex((w) => w[1] === t)
  return WORDS[i + n - 1][2]
}
const LAST = WORDS[WORDS.length - 1]
// the film ends on seven, a held second after the last word (the strings at their crest), then a hard
// cut to black
export const END = TAPE.marks.end ?? LAST[2] + 1.0
// the plate (the study, in through the screen) is a fixed length
const INTRO = 7.382
// the closest the camera gets: 94% of the screen in view
const PUSH_MIN = 0.94


// A year ago: forty windows, give or take, in a heap. Work stays on one; three of them are messages.
const R01 = (i, k) => {
  const x = Math.sin(i * 127.1 + k * 311.7) * 43758.5453
  return x - Math.floor(x)
}
const BROWSE = [
  ["Gutter guards: which kind?", "homedesk.example/gutter-guards"],
  ["Pinecrest: collection days", "pinecrest.example/bins"],
  ["Q3 benchmarks — industry", "benchmarks.example/q3"],
  ["How fast does privet grow?", "gardenly.example/privet"],
  ["Cheap flights in January", "flights.example/jan"],
  ["Best soda bread", "bakes.example/soda-bread"],
  ["Renewals dashboard", "intranet.example/renewals"],
  ["Weather — Pinecrest", "weather.example/pinecrest"],
  ["Spaceman — tiling for Mac", "anoma.ly/spaceman"],
]
// an ordinary list; the drill comes back much later, on Alan's workspace
const NOTE = [{ t: "Groceries" }, { t: "Fix porch light" }, { t: "Return Alan's drill" }]
export function pile(SW, SH) {
  const W = (f) => f * SW
  const H = (f) => f * SH
  const tall = SH > SW
  const F = (x, y, w, h) => (tall ? [W(y * 0.9), H(0.06 + x * 0.8), W(Math.min(0.92, h * 1.5)), H(w * 0.62)] : [W(x), H(y), W(w), H(h)])
  const spot = (i, w, h) => F(0.02 + R01(i, 1) * (0.96 - w), 0.05 + R01(i, 2) * (0.55 - h * 0.6), w, h)
  // the four that stay come first (tile order), the clutter on top of them
  const defs = []
  defs.push({ id: "q3", app: "paper", kind: "paper", title: "Q3 report — draft", data: { heading: "Q3 report", sub: "Draft 4", lines: Q3 } })
  defs.push({ id: "notes", app: "jot", kind: "jot", title: "To Do", data: { heading: "To Do", edited: "Edited March 2", lines: NOTE, check: true } })
  defs.push({ id: "postbox", app: "postbox", kind: "mail", title: "Inbox — 1,206 messages", data: mail() })
  defs.push({ id: "group", app: "chatter", kind: "chat", title: "Pinecrest Neighbours", data: { who: "Pinecrest Neighbours · 9", when: "Today 8:02 PM", msgs: [{ from: "Alan (10)", t: "Whose is the blue car?" }, { from: "Ruth (8)", t: "Mine, sorry! Moving it now" }] } })
  defs.push({ id: "daybook", app: "daybook", kind: "daybook", title: "Calendar", data: { date: "Tuesday, November 25", events: [{ t: "Bins out", at: 19, len: 0.7, color: "#d9e3cf" }, { t: "Renewals review", at: 9, len: 1, color: "#d6dff0" }] } })
  for (const i of [1, 3, 6]) {
    const [heading, url] = BROWSE[i]
    defs.push({ id: "b" + i, app: "browsy", kind: "browser", title: heading, data: { tabs: 6 + Math.floor(R01(i, 3) * 30), url, heading, tab: heading } })
  }
  defs.push({ id: "notes1", app: "jot", kind: "jot", title: "To Do (1)", data: { heading: "To Do", edited: "Edited March 14", lines: NOTE, check: true } })
  defs.push({ id: "notes2", app: "jot", kind: "jot", title: "To Do — real", data: { heading: "To Do", edited: "Edited April 30", lines: NOTE, check: true } })
  return defs.map((d, i) => ({ ...d, at: spot(i + 3, 0.34 + R01(i, 4) * 0.16, 0.42 + R01(i, 5) * 0.2) }))
}
const Q3 = ["Renewals rose 4% in Q2.", "Costs were flat."]

function mail() {
  const list = [
    { from: "Pinecrest HOA", subject: "Reminder: hedge and fence heights", when: "Feb 20" },
    { from: "Orders", subject: "Your order has shipped", when: "Feb 18" },
    { from: "City of Pinecrest", subject: "Collection day: Thursday", when: "Feb 2" },
    { from: "Weekly", subject: "Your week in photos", when: "Jan 31" },
    { from: "Alan (No. 10)", subject: "re: drill", when: "Jan 24" },
  ]
  return { list, sel: -1, count: "1,206" }
}

// The grade follows the story: a warm, tired night; clean light once things have places; mornings;
// the months cooling; a cold November night; six.
const LK = {
  night: { exposure: 0.93, warmth: 0.35, saturation: 0.92, contrast: 1.02, vignette: 0.3, bloom: 0.16, g: [0, 0, 0] },
  clean: { exposure: 1.0, warmth: 0.12, saturation: 1.0, contrast: 1.03, vignette: 0.22, bloom: 0.16, g: [0, 0, 0] },
  morning: { exposure: 1.04, warmth: 0.04, saturation: 1.02, contrast: 1.03, vignette: 0.2, bloom: 0.18, g: [0, 0, 0] },
  house: { exposure: 0.97, warmth: 0.26, saturation: 0.96, contrast: 1.03, vignette: 0.26, bloom: 0.16, g: [0, 0, 0] },
  hedge: { exposure: 1.02, warmth: 0.1, saturation: 0.98, contrast: 1.03, vignette: 0.24, bloom: 0.16, g: [0, 0, 0] },
  cool: { exposure: 0.95, warmth: -0.2, saturation: 0.82, contrast: 1.05, vignette: 0.28, bloom: 0.16, g: [0, 0.2, 0.05] },
  nov: { exposure: 0.9, warmth: -0.3, saturation: 0.82, contrast: 1.05, vignette: 0.3, bloom: 0.17, g: [0.05, 0.35, 0.08] },
  six: { exposure: 0.84, warmth: -0.5, saturation: 0.7, contrast: 1.08, vignette: 0.38, bloom: 0.18, g: [0.14, 0.5, 0.1] },
  room: { exposure: 1.0, warmth: 0.05, saturation: 0.95, contrast: 1.02, vignette: 0.32, bloom: 0.16, g: [0, 0, 0] },
}
// The grade follows the story: a warm, tired night; clean light once things have places; mornings;
// the months cooling; a cold November night; six.
let LOOKS = null
function looks(T) {
  return [
    [0, LK.room], [T.intro - 1.5, LK.room], [T.intro, LK.night], [T.tiled, LK.night], [T.tiled + 0.8, LK.clean], [T.late - 0.4, LK.clean], [T.late + 0.6, LK.cool],
    [T.house - 0.3, LK.cool], [T.house + 0.5, LK.house], [T.hedge - 0.3, LK.house], [T.hedge + 0.8, LK.hedge], [T.bins - 0.3, LK.hedge], [T.bins + 0.6, LK.nov],
    [T.still - 1.2, LK.nov], [T.still + 0.4, LK.six], [1e4, LK.six],
  ]
}

function applyLook(S, t) {
  if (!LOOKS) return
  let i = 0
  while (i + 2 < LOOKS.length && LOOKS[i + 1][0] <= t) i++
  const [t0, a] = LOOKS[i]
  const [t1, b] = LOOKS[i + 1]
  const u = smooth((t - t0) / (t1 - t0))
  for (const k of ["exposure", "warmth", "saturation", "contrast", "vignette", "bloom"]) S.fx[k] = mix(a[k], b[k], u)
  for (let j = 0; j < 3; j++) S.grade[j] = mix(a.g[j], b.g[j], u)
}

// A camera move between two views as one zoom about the point they agree on (no dog-legs).
function zoomView(A, B, e) {
  const lk = Math.log(B.D / A.D)
  if (Math.abs(lk) < 0.02) return { cx: mix(A.cx, B.cx, e), cy: mix(A.cy, B.cy, e), D: mix(A.D, B.D, e) }
  const inv = 1 / A.D - 1 / B.D
  const px = (A.cx / A.D - B.cx / B.D) / inv
  const py = (A.cy / A.D - B.cy / B.D) / inv
  const D = A.D * Math.exp(lk * e)
  return { cx: px - ((px - A.cx) * D) / A.D, cy: py - ((py - A.cy) * D) / A.D, D }
}

// A hand on a keyboard: uneven gaps, a hesitation before the first key.
function rhythm(n, seed = 1) {
  const out = []
  let t = 0
  for (let i = 0; i < n; i++) {
    const h = Math.abs(Math.sin((i + 1) * 12.9898 * seed) * 43758.5453) % 1
    t += i === 0 ? 0 : 0.075 + h * 0.08 + (i === 1 ? 0.05 : 0)
    out.push(t)
  }
  return out
}


export function build(S) {
  const { world: w } = S
  const cues = []
  const spans = []
  const at = (t, fn) => cues.push({ t, fn })
  const span = (t0, t1, fn) => spans.push({ t0, t1, fn })
  const R = (id) => w.wins.get(id)
  const closeBtn = (id) => {
    const x = R(id)
    if (!x) throw new Error("no window " + id)
    return [x.x.t + 14, x.y.t + BAR_H / 2]
  }
  const titleOf = (id) => {
    const x = R(id)
    return [x.x.t + x.w.t * 0.42, x.y.t + BAR_H / 2]
  }
  // The cursor sets off in time to arrive a beat before the click, never cutting short the last one.
  let lastClick = -10
  // kind: the cursor it turns into as it arrives (a hand on a link, an I-beam on a field)
  const clickAt = (t, id, where = titleOf, fn, maxLead = 1.2, kind) => {
    const lead = Math.min(maxLead, t - lastClick - 0.08)
    lastClick = t
    at(t - lead, () => {
      const p = typeof where === "function" ? where(id) : where
      w.cursorKind("arrow")
      w.hideCursor(false)
      w.cursorTo(p[0], p[1], t - 0.2)
    })
    if (kind) at(t - 0.24, () => w.cursorKind(kind))
    at(t, () => {
      w.click()
      S.sfx("tap")
      fn?.()
    })
  }
  const key = (t, label, fn) =>
    at(t, () => {
      w.keyCast(label)
      S.sfx("key")
      fn?.()
    })
  const sw = (t, n, clock, silent) =>
    at(t, () => {
      if (clock) w.clock = clock
      w.switchTo(n)
      if (!silent) S.sfx("key", 0.9)
    })
  const type = (t0, s, fn, seed = 1) =>
    rhythm(s.length, seed).forEach((dt, i) =>
      at(t0 + dt, () => {
        fn(s.slice(0, i + 1), i === s.length - 1)
        S.sfx("key", 0.85)
      }),
    )
  // The camera stays on the whole desktop: a framing is at most a gentle push toward its subject.
  const fit = (x, y, ww, hh, margin) => {
    const D = Math.min(1, Math.max(PUSH_MIN, ww / S.SW, (hh / S.SH) * margin, (ww / S.SW) * margin))
    // keep the view inside the screen: never show what's past its edge
    const hx = (S.SW * D) / 2
    const hy = (S.SH * D) / 2
    return { cx: clamp(x + ww / 2, hx, S.SW - hx), cy: clamp(y + hh / 2, hy, S.SH - hy), D }
  }
  const frameOf = (id, margin = 1.1) => {
    const x = R(id)
    return fit(x.x.t, x.y.t, x.w.t, x.h.t, margin)
  }
  // two windows framed together
  const frameBoth = (a, b, margin = 1.08) => {
    const A = R(a)
    const B = R(b)
    const x0 = Math.min(A.x.t, B.x.t)
    const y0 = Math.min(A.y.t, B.y.t)
    const x1 = Math.max(A.x.t + A.w.t, B.x.t + B.w.t)
    const y1 = Math.max(A.y.t + A.h.t, B.y.t + B.h.t)
    return fit(x0, y0, x1 - x0, y1 - y0, margin)
  }
  // One camera move from wherever it is to a framing, over [t0, t1]: one eased progress value.
  const push = (t0, t1, target, focus) => {
    let A = null
    let B = null
    span(t0, t1, (u) => {
      if (!A) {
        A = { ...S.cam }
        B = typeof target === "function" ? target() : target
        if (focus !== undefined) S.look.focus = focus
      }
      const e = S.reduced ? (u < 0.5 ? 0 : 1) : easeInOut(u)
      Object.assign(S.cam, zoomView(A, B, e))
    })
  }
  const home = () => ({ cx: S.SW / 2, cy: S.SH / 2, D: 1 })
  const openMany = (t, ws, defs) =>
    at(t, () => {
      w.openMany(defs.map((d) => ({ ws, ...d })))
    })
  // a float in the middle of the screen (a page, a calendar), and its frame
  const mid = (fw, fh, dy = 0) => {
    const ww = Math.min(S.SW * 0.9, fw)
    const hh = Math.min(S.SH * 0.78, fh)
    return [S.SW / 2 - ww / 2, (S.SH - 60) / 2 - hh / 2 + 10 + dy, ww, hh]
  }
  const float = (t, def) =>
    at(t, () => {
      w.open({ float: true, ...def, at: typeof def.at === "function" ? def.at() : def.at })
    })
  // a window closing makes no sound of its own
  const shut = (t, id) => at(t, () => w.close(id))

  // a camera window in Vigil; its stamp is set per scene by feedsAt
  const CAM = (id, src, title, data) => ({ id, app: "vigil", kind: "feed", title, dark: 1, data: { src, name: title.toUpperCase(), us: 1, ...data } })
  // every camera's burned-in clock, as of now in the story (date mm-dd-yyyy, seconds after midnight)
  const feedsAt = (date, secs, wd) => {
    for (const x of w.wins.values()) if (x.kind === "feed") w.set(x.id, { data: { date, clock: secs - w.time, wd } })
  }
  const hms = (h, m, s = 0) => h * 3600 + m * 60 + s

  span(0, END, (u, t) => applyLook(S, t))

  // A camera move through the room: a → b over [t0, t1], eased (cut at the middle when reduced).
  const move = (t0, t1, a, b, e = easeInOut) =>
    span(t0, t1, (u) => {
      if (b === "screen" && u >= 1) return S.flat()
      S.shot(a, b, S.reduced ? (u < 0.5 ? 0 : 1) : e(u))
    })

  // ── 0 · The study at night; the tone; the screen wakes; in through it ──
  cursor = 0
  const intro = INTRO
  at(0, () => {
    w.bar.scale.x = w.bar.scale.t = 1.5
    S.three.glow = 0
    S.shot("CamChair", "CamChair", 0)
    w.veiled.add(7)
    w.clock = "Tue Nov 25  11:48 PM"
  })
  span(0.42, 0.95, (u) => (S.three.glow = smooth(u) * (1 + 0.06 * Math.sin(u * 9) * (1 - u))))
  at(0.42, () => S.sfx("key", 0.9))
  move(1.2, intro, "CamChair", "screen", (u) => easeInOut(u) ** 1.15)
  at(intro, () => S.flat())
  // his study: a fridge two rooms away, the night outside; under everything, heard only in the gaps
  at(intro, () => S.bed("room", 3.5, 2))
  cursor = TAPE.marks.head

  // ── "A year ago I had forty windows open and couldn't find anything." ──
  const forty = word("forty windows")
  push(intro + 0.05, forty + 0.6, { cx: S.SW * 0.5, cy: S.SH * 0.48, D: 0.9 })
  at(intro + 0.2, () => w.cursorTo(S.SW * 0.55, S.SH * 0.55, forty))
  // hunting: one window raised, then another, then another
  const find = word("couldn't find")
  clickAt(find - 0.35, "notes1", titleOf, () => w.setFocus("notes1"), 0.6)
  clickAt(find + 0.2, "b3", titleOf, () => w.setFocus("b3"), 0.5)
  clickAt(find + 0.75, "notes2", titleOf, () => w.setFocus("notes2"), 0.5)
  clickAt(find + 1.3, "b6", titleOf, () => w.setFocus("b6"), 0.5)
  // ── Spaceman: he goes to anoma.ly/spaceman in the Safari window in front and clicks Download on its name;
  // on "It tiles" it's running (its item in the menu bar) and everything goes side by side ──
  const install = word("Then I installed")
  const named = word("Spaceman")
  const tiled = word("It tiles")
  push(install - 0.1, install + 0.5, () => frameOf("b6", 1.15), "b6")
  const field = (id) => {
    const x = R(id)
    return [x.x.t + x.w.t / 2 - 20, x.y.t + BAR_H / 2]
  }
  const download = (id) => {
    const x = R(id)
    const g = sitePageGeom(x.w.t, x.h.t, BAR_H + 28)
    return [x.x.t + g.bx + g.bw * 0.46, x.y.t + g.by + g.bh * 0.55]
  }
  const tField = install + 0.12
  clickAt(tField, "b6", field, () => {
    w.setFocus("b6")
    w.set("b6", { data: { edit: "", sel: true } })
  }, 0.6, "ibeam")
  // a quick, sure hand: he's typed it before
  const URL_ = "anoma.ly/spaceman"
  const keys = rhythm(URL_.length, 7).map((dt) => dt * 0.55)
  const tType = tField + 0.2
  at(tType + 0.02, () => w.hideCursor(true))
  keys.forEach((dt, i) =>
    at(tType + dt, () => {
      w.set("b6", { data: { edit: URL_.slice(0, i + 1), sel: false } })
      S.sfx("key", 0.85)
    }),
  )
  const tGo = tType + keys.at(-1) + 0.16
  at(tGo, () => {
    w.set("b6", { data: { edit: null, url: URL_, full: true, load: 0.04 } })
    S.sfx("key", 0.9)
  })
  span(tGo, tGo + 0.42, (u) => w.set("b6", { data: { load: 0.04 + 0.96 * smooth(u) } }))
  at(tGo + 0.2, () => w.set("b6", { title: "Spaceman", data: { page: "spaceman", tab: "Spaceman — For your windows.", heading: "Spaceman" } }))
  at(tGo + 0.44, () => w.set("b6", { data: { load: null } }))
  // Download for macOS, as he says its name; the toolbar's download button drops in and fills
  const tDl = Math.max(tGo + 0.8, named - 0.1)
  clickAt(tDl, "b6", download, () => w.set("b6", { data: { dl: 0 } }), 0.7, "hand")
  at(tDl + 0.45, () => w.cursorKind("arrow"))
  span(tDl - 0.03, tDl + 0.2, (u) => w.set("b6", { data: { press: Math.sin(Math.PI * u) } }))
  span(tDl + 0.02, tDl + 0.75, (u) => w.set("b6", { data: { dl: u } }))
  push(tDl + 0.3, tiled - 0.02, home, null)
  // running: its item in the menu bar as "It tiles" lands (the bar only comes up on a switch)
  at(tiled - 0.12, () => {
    w.status = true
    w.statusIn = w.time
  })
  // and as it starts: ⌘W, ⌘W, ⌘W, the duplicates go, front to back, until four are left to tile
  const clutter = ["b6", "notes2", "b3", "notes1", "b1", "daybook"]
  const sweep = Math.max(named + 0.3, tDl + 0.5)
  const gap = 0.15
  at(sweep - 0.05, () => w.keyCast(["⌘", "W"], clutter.length * gap + 0.2))
  clutter.forEach((id, i) =>
    at(sweep + i * gap, () => {
      w.close(id)
      S.sfx("key", 0.85)
      const next = [...w.wins.values()].filter((x) => x.ws === 1 && !x.leaving && !x.float).sort((p, q) => q.z - p.z)[0]
      if (next) w.setFocus(next.id)
    }),
  )
  at(tiled + 0.05, () => {
    w.tileNow()
  })
  // two more open, and the others move over
  const sideBy = word("side by side")
  at(sideBy - 0.35, () => {
    w.open({ id: "b9", app: "browsy", kind: "browser", title: "Start Page", data: { tabs: 1, url: "", heading: "New Tab", tab: "New Tab", lines: [] }, at: [S.SW * 0.3, S.SH * 0.3, S.SW * 0.3, S.SH * 0.3] })
  })
  at(word("nothing on top") - 0.1, () => {
    w.open({ id: "u48", app: "jot", kind: "jot", title: "Untitled 48", data: { heading: "Untitled 48", lines: [] }, at: [S.SW * 0.3, S.SH * 0.3, S.SW * 0.3, S.SH * 0.3] })
  })

  // ── "It also gives you numbered workspaces. If something's in the wrong place, you don't close it —
  // you just move it to another number. My work went on one, and my messages on two." ──
  const numbered = word("numbered workspaces")
  at(numbered - 0.1, () => {
    w.ensureCell(3)
    w.showBar()
  })
  // the two that don't belong with his work go to two as he says it
  const another = word("another number")
  ;["postbox", "group"].forEach((id, i) => {
    const t = another + 0.15 + i * 0.35
    at(t - 0.02, () => w.setFocus(id))
    key(t, ["⌥", "⇧", "2"], () => w.move(id, 2))
  })
  // the rule: Messages opens on 2
  const settingsAt = () => {
    const ww = Math.min(S.SW * 0.9, 860)
    const hh = Math.min(S.SH * 0.66, 450)
    return [S.SW / 2 - ww / 2, S.SH * 0.44 - hh / 2, ww, hh]
  }
  const settingsFrame = () => {
    const [x, y, ww, hh] = settingsAt()
    return fit(x, y, ww, hh, 1.14)
  }
  // Settings › Rules, the workspace map: Add on workspace 2's tile, then Messages from the menu
  const inSettings = (f) => () => {
    const [x, y, ww, hh] = settingsAt()
    const [px, py] = f(settingsGeom(ww, hh))
    return [x + px, y + py]
  }
  const myWork = word("My work")
  float(myWork + 0.05, { id: "settings", app: "spaceman", kind: "settings", title: "Spaceman Settings", at: settingsAt, data: { rules: [], active: [1, 2] } })
  push(myWork + 0.1, myWork + 0.75, settingsFrame, "settings")
  at(myWork + 0.42, () => w.set("settings", { data: { hover: 2 } }))
  clickAt(myWork + 0.6, "settings", inSettings((g) => g.add(2)), () => w.set("settings", { data: { menu: 2 } }), 0.5)
  const msgs2 = word("messages on two")
  const pick = Math.max(word("two") + 0.02, msgs2 + 0.5)
  at(pick - 0.2, () => w.set("settings", { data: { menuHover: 0 } }))
  clickAt(pick, "settings", inSettings((g) => g.menuItem(0)), () => w.set("settings", { data: { menu: 0, menuHover: null, hover: null, hl: 2, rules: [{ app: "Messages", icon: "chatter", ws: 2 }] } }), 0.7)
  const camera = word("I had a camera")
  push(camera - 0.35, camera + 0.25, home, null)
  clickAt(camera - 0.2, "settings", closeBtn, () => w.close("settings"), 0.45)
  // a new Messages window: Juliana, a year ago, saying hello. It goes straight to two.
  at(camera + 0.05, () => {
    w.openMany([{ ws: 2, id: "joan", app: "chatter", kind: "chat", title: "Juliana (No. 16)", data: { who: "Juliana (No. 16)", when: "Nov 26, 2025", msgs: [{ t: "Hi! Welcome to the street 🙂" }, { t: "Just putting mine out Wednesdays now, hope that's ok!" }], unread: { "Pinecrest Neighbours · 9": 1 } } }])
    w.pulse(2)
  })

  // ── "I had a camera on my front door, and that's only another window, so I put it on three." ──
  const door = word("front door")
  // the clip's own clock: she steps into the frame half a second in, and walks across in about six
  const walker = { t0: 0, dur: 6, x0: -0.07, x1: 0.855, y: 0.86, h: 0.33 }
  const walkClip = { name: "walker", n: 24, fps: 4, t0: 0, end: 2 }
  at(door - 0.25, () => {
    w.open({ id: "door", ...CAM("door", "st-08", "Front Door", { q: 0.45, seed: 17, seq: walkClip }), at: [S.SW * 0.3, S.SH * 0.3, S.SW * 0.3, S.SH * 0.3] })
    feedsAt("11-26-2025", hms(23, 52, 10))
  })
  push(door + 0.0, door + 0.8, () => frameOf("door", 1.35), "door")
  const onThree = word("put it on three")
  push(onThree - 0.7, onThree - 0.05, home, null)
  clickAt(onThree - 0.15, "door", titleOf, () => w.setFocus("door"), 0.6)
  key(onThree + 0.45, ["⌥", "⇧", "3"], () => w.move("door", 3))

  // ── "You can float a window too, if you want to keep an eye on it. I found it pleasing." ──
  // He tries to work, with the front door floating over the corner of his report (a float rule for
  // the camera app: floats sit above the tiles on their workspace and are never tiled).
  const late = onThree + 0.75
  at(late, () => {
    w.clock = "Thu Dec 4  2:13 AM"
    feedsAt("12-04-2025", hms(2, 13, 40))
    w.setFocus("q3")
  })
  // (after door has flown to three, so the tiles have settled)
  push(late + 0.2, late + 0.8, () => frameOf("q3", 1.3), "q3")
  const floatT = word("You can float") + 0.35
  float(floatT, {
    id: "peek", ...CAM("peek", "st-08", "Front Door", { q: 0.45, seed: 17, seq: walkClip }), pin: true,
    at: () => {
      const p = R("q3")
      const fw = Math.min(p.w.t * 0.44, 340)
      const fh = fw * 0.6 + BAR_H
      return [p.x.t + p.w.t - fw - 20, p.y.t + p.h.t - fh - 20, fw, fh]
    },
  })
  at(floatT + 0.02, () => {
    feedsAt("12-04-2025", hms(2, 13, 41))
    w.setFocus("q3")
  })
  at(late + 0.1, () => w.set("q3", { data: { caret: 1 } }))
  type(late + 0.45, "In the third quar", (s) => w.set("q3", { data: { lines: [...Q3, s] } }), 3)
  const typed = late + 0.45 + rhythm(17, 3).at(-1)
  // he stops mid-word. Three. One. Three.
  const s3 = typed + 0.55
  push(s3 - 0.4, s3 - 0.02, home, null)
  sw(s3, 3)
  sw(s3 + 0.5, 1)
  sw(s3 + 1.15, 3)
  // she comes into the picture; he finds her, zooms, follows, zooms further, keeps the frame
  const her = s3 + 1.45
  walker.t0 = walkClip.t0 = her - 0.45
  const zf = () => R("door")
  const z1 = her + 0.85
  const z2 = z1 + 1.3
  const save = z2 + 0.55
  const nm = "dog walker 2"
  const named2 = save + 0.4 + rhythm(nm.length, 5).at(-1) + 0.3
  // the view: zoom in on her, follow her, zoom further, and keep following until the still is named
  const zoomAt = (t) => {
    if (t < z1) return null
    const k = walkerAt(walker, Math.min(t, named2))
    const z = t < z1 + 0.6 ? mix(1, 2.2, easeInOut((t - z1) / 0.6)) : t < z2 ? 2.2 : mix(2.2, 3.6, easeInOut(Math.min(1, (t - z2) / 0.45)))
    return { z, x: k.x, y: walker.y - walker.h * 0.45 }
  }
  // where she is on screen at time t (the window's picture, through the zoom)
  const onHer = (t) => {
    const p = zf()
    const k = walkerAt(walker, t)
    const r = { x: p.x.t, y: p.y.t + BAR_H, w: p.w.t, h: p.h.t - BAR_H }
    const z = zoomAt(t) || { z: 1, x: 0.5, y: 0.5 }
    const half = 0.5 / z.z
    const cx = clamp(z.x, half, 1 - half)
    const cy = clamp(z.y, half, 1 - half)
    const sx = 0.5 + (k.x - cx) * z.z
    const sy = 0.5 + (k.y - walker.h * 0.55 - cy) * z.z
    return [r.x + r.w * clamp(sx, 0.05, 0.95), r.y + r.h * clamp(sy, 0.05, 0.95)]
  }
  at(her + 0.25, () => w.cursorTo(...onHer(her + 0.8), her + 0.8))
  at(her + 0.25, () => (w.clock = "Thu Dec 4  2:14 AM"))
  at(her, () => feedsAt("12-04-2025", hms(2, 14, 6)))
  span(z1, named2 + 0.05, (u, t) => w.set("door", { data: { zoom: zoomAt(t) } }))
  // the cursor stays on her as the view follows
  at(z1 + 0.1, () => w.cursorTo(...onHer(z1 + 0.6), z1 + 0.6))
  at(z1 + 0.7, () => w.cursorTo(...onHer(z2 + 0.4), z2 + 0.4))
  key(save, ["⌘", "S"], () => {
    w.set("door", { data: { snap: { name: "Front Door 2025-12-04 02.14.11", sel: 1, edit: 1 } } })
  })
  type(save + 0.4, nm, (s) => w.set("door", { data: { snap: { name: s, edit: 1 } } }), 5)
  key(named2, ["↩"], () => w.set("door", { data: { snap: { name: nm, edit: 0 } } }))

  // ── "So I bought one or two more" ──
  const bought = word("So I bought")
  const orderAt = () => mid(760, 470)
  const orderT = Math.max(bought + 0.35, named2 + 0.25)
  float(orderT, {
    id: "order", app: "browsy", kind: "browser", title: "Order confirmed", at: orderAt,
    data: { tabs: 3, tabNames: ["Pinecrest HOA Bylaws", "Night vision range test"], url: "vigil.example/orders/80412", tab: "Order confirmed", heading: "Thank you. Your order is confirmed.", lines: ["Order 80412 · placed Dec 5, 9:02 AM", { gap: 1 }, { t: "Outdoor camera × 14", b: 1, size: 19 }, "Night vision · 2K · weatherproof", "Mounting kit × 14", { gap: 1 }, "Arriving Thursday, Dec 11"] },
  })
  at(orderT - 0.02, () => w.set("door", { data: { snap: null } }))
  at(orderT, () => (w.clock = "Fri Dec 5  9:02 AM"))
  push(orderT + 0.05, orderT + 0.85, () => frameOf("order", 1.12), "order")
  const gave = word("gave each")
  // he talks on over the naming, so the order lands a beat after "bought"; give it a second on screen
  const backT = Math.max(gave + 1.2, orderT + 1.15)
  push(backT, backT + 1.0, home, null)
  shut(word("Alan is four") - 0.2, "order", 0.45)

  // ── "Alan is four. Juliana is five. Dax is six." ──
  const dossier = (p, who, src, crops, note, week, seq) => [
    CAM(p + "-front", src, who + " — Front", { q: 0.42, seed: 19 + who.length, ir: who === "Alan" ? 1 : 0, seq }),
    CAM(p + "-door", src, who + " — Door", { q: 0.55, seed: 23 + who.length, crop: crops[0], seq }),
    CAM(p + "-up", src, who + " — Upstairs", { q: 0.6, seed: 29 + who.length, crop: crops[1], seq }),
    { id: p + "-note", app: "jot", kind: "jot", title: who, data: { heading: note[0], edited: "Edited 10:52 PM", list: ["Alan", "Juliana", "Dax"], lines: note.slice(1) } },
    { id: p + "-week", app: "daybook", kind: "daybook", title: "Calendar", data: { week: ["Mon 1", "Tue 2", "Wed 3", "Thu 4", "Fri 5", "Sat 6", "Sun 7"], title: who + ", this week", from: 6, to: 24, events: week } },
  ]
  const GREY = "#dcdcd6"
  const DAYS = [0, 1, 2, 3, 4]
  const mark = word("Alan is four")
  sw(mark - 0.05, 4, "Sat Dec 6  10:52 PM")
  openMany(mark + 0.12, 4, dossier("m", "Alan", "st-10", [[0.15, 0.15, 0.42, 0.42], [0.5, 0.14, 0.18, 0.24]], ["Alan (No. 10)", "Out 07:42. Back 18:15.", "Gym Tue/Thu, 19:00–20:30.", "Bins Thursday.", "Lives alone.", { gap: 1 }, { t: "Return Alan's drill", box: 1, drill: 1 }], [
    ...DAYS.map((d) => ({ day: d, at: 7.7, len: 10.5, t: "Out", color: GREY })),
    { day: 1, at: 19, len: 1.5, t: "Gym", color: "#d6dff0" },
    { day: 3, at: 19, len: 1.5, t: "Gym", color: "#d6dff0" },
  ]))
  const joan = word("Juliana is five")
  sw(joan - 0.05, 5)
  openMany(joan + 0.12, 5, dossier("j", "Juliana", "st-16", [[0.33, 0.42, 0.36, 0.38], [0.38, 0.06, 0.3, 0.26]], ["Juliana (No. 16)", "Juliana: lights off 22:31.", "Daughter, Sundays, 2 h 10.", "Cat out 06:50, in 21:05.", "Bins Wednesday."], [
    { day: 6, at: 14, len: 2.17, t: "Daughter", color: "#e9c9c4" },
    { day: 2, at: 20, len: 0.8, t: "Bins", color: "#d9e3cf" },
    ...[0, 1, 2, 3, 4, 5, 6].map((d) => ({ day: d, at: 22.5, len: 0.9, t: "Off", color: GREY })),
  ]))
  const alan = word("Dax is six")
  sw(alan - 0.05, 6)
  openMany(alan + 0.12, 6, dossier("a", "Dax", "st-14", [[0.43, 0.44, 0.26, 0.3], [0.43, 0.22, 0.16, 0.22]], ["Dax (No. 14)", "Up 06:10. Curtains 06:12.", "Hedge 6′ 2″. Limit 6′.", "At his window most nights.", "Looks across."], [
    ...[0, 1, 2, 3, 4, 5, 6].map((d) => ({ day: d, at: 21.8, len: 2.1, t: "Window", color: "#e9c9c4" })),
    { day: 5, at: 9, len: 2, t: "Hedge", color: "#d9e3cf" },
  ], { name: "alan", n: 20, fps: 4, t0: alan + 0.12, loop: "pingpong", from: 6 }))
  at(mark + 0.2, () => feedsAt("12-06-2025", hms(22, 52, 30)))
  at(joan + 0.2, () => feedsAt("12-06-2025", hms(22, 52, 31)))
  at(alan + 0.2, () => feedsAt("12-06-2025", hms(22, 52, 33)))
  // the notes and the weeks sit in the same place on each: the camera stays on them as he flicks
  push(joan + 0.15, joan + 0.85, () => frameBoth("j-note", "j-week", 1.06), "j-note")
  const like = word("I like to know")
  push(like + 0.3, like + 1.2, home, null)
  // then four, five, six, faster each time, the days going by in the menu bar
  let tq = wordEnd("my neighbours") + 0.1
  const DATES = ["Sun Dec 7", "Mon Dec 8", "Wed Dec 10", "Fri Dec 12", "Mon Dec 15", "Thu Dec 18", "Mon Dec 22", "Fri Jan 2", "Mon Jan 12"]
  ;[0.5, 0.42, 0.36, 0.3, 0.26, 0.22, 0.19, 0.17, 0.15].forEach((dt, i) => {
    sw(tq, [4, 5, 6][i % 3], DATES[i] + "  11:" + String(10 + i * 4) + " PM")
    const [, mo, dd] = DATES[i].split(" ")
    const date = (mo === "Dec" ? "12-" : "01-") + dd.padStart(2, "0") + (mo === "Dec" ? "-2025" : "-2026")
    at(tq + 0.005, () => feedsAt(date, hms(23, 10 + i * 4, 20)))
    tq += dt
  })

  // ── "Dax's hedge was a little over the limit, so I took a few photographs, for the record." ──
  const hedge = word("Dax's hedge")
  const albumAt = () => mid(1040, 760, 0)
  float(hedge + 0.1, { id: "album", app: "viewer", kind: "album", title: "Photos", at: albumAt, data: { scroll: 0, bar: 0 } })
  at(hedge + 0.1, () => (w.clock = "Mon Feb 16  11:58 PM"))
  push(hedge + 0.2, hedge + 0.9, () => frameOf("album", 1.04), "album")
  const ag = () => albumGrid(R("album").w.t, R("album").h.t)
  at(hedge + 0.4, () => {
    const p = R("album")
    w.cursorTo(p.x.t + p.w.t * 0.6, p.y.t + p.h.t * 0.62, hedge + 1.0)
  })
  // his scroll: three unhurried throws down the first day, each photo there to be seen, then ⌘↓
  // to the newest; the rows load as they come into view
  const f0 = hedge + 0.9
  const jumpT = f0 + 4.75
  const path = (g) => [
    { t0: f0, t1: f0 + 1.35, tau: 0.5, to: g.scrollTo(26, 0.2) },
    { t0: f0 + 1.5, t1: f0 + 3.05, tau: 0.45, to: g.scrollTo(62, 0.2) },
    { t0: f0 + 3.2, t1: f0 + 4.6, tau: 0.45, to: g.scrollTo(104, 0.2) },
    { t0: jumpT, t1: jumpT, to: g.maxScroll },
  ]
  span(hedge + 0.1, word("And Juliana") - 0.2, (u, t) => {
    if (!R("album")) return
    const p = path(ag())
    const bar = Math.max(0, Math.min(1, t < f0 ? 0 : 1 - (t - (jumpT + 0.7)) / 0.4))
    w.set("album", { data: { scroll: scrollOn(p, t, S.reduced), path: S.reduced ? null : p, now: t, open0: hedge + 0.1, bar } })
  })
  key(jumpT, ["⌘", "↓"], () => {})
  // the last one: him, at his window, looking back
  const lastAt = () => {
    const p = R("album")
    const g = ag()
    const r = g.tile(ALBUM - 1, p.data.scroll ?? g.maxScroll)
    return [p.x.t + r.x + r.w * 0.5, p.y.t + r.y + r.h * 0.55]
  }
  const look = f0 + 6.75
  clickAt(look, "album", lastAt, () => w.set("album", { data: { sel: ALBUM - 1 } }), 0.5)
  at(look + 0.1, () => w.set("album", { data: { open: ALBUM - 1, openT: 0 } }))
  span(look + 0.1, look + 0.5, (u) => w.set("album", { data: { openT: S.reduced ? (u < 0.5 ? 0 : 1) : 1 - (1 - u) ** 3 } }))
  push(look + 0.3, word("And Juliana") - 0.35, () => {
    const p = R("album")
    return fit(p.x.t + p.w.t * 0.18, p.y.t + p.h.t * 0.16, p.w.t * 0.64, p.h.t * 0.64, 1.02)
  })

  // ── "And Juliana and I disagreed about which day the bins go out." ──
  const bins = word("And Juliana")
  shut(bins - 0.25, "album", 0.4)
  push(bins - 0.25, bins - 0.02, home, null)
  sw(bins - 0.1, 5, "Wed Feb 18  9:12 PM")
  at(bins - 0.08, () => {
    w.set("j-front", { data: { src: "st-16-bins" } })
    w.set("j-door", { data: { src: "st-16-bins" } })
    feedsAt("02-18-2026", hms(21, 12, 40), "WED")
  })
  push(bins + 0.2, bins + 0.95, () => frameOf("j-front", 1.12), "j-front")
  const lidAt = () => {
    const p = R("j-front")
    const ww = Math.min(300, p.w.t * 0.5)
    return [p.x.t + p.w.t - ww - 18, p.y.t + p.h.t - ww * 0.62 - 40, ww, ww * 0.62]
  }
  float(bins + 1.25, { id: "lid", app: "jot", kind: "sticky", title: "", at: lidAt, data: { text: "Thursday. It says so on the lid." } })
  // what he sent her, one after another; she never answered
  const SENT = ["Hi Juliana — collection is Thursday. It says so on the lid.", "Just a quick reminder, it's Wednesday today.", "I rolled them back behind your gate for you.", "Juliana."]
  const dr = bins + 2.75
  shut(dr - 0.2, "lid", 0.35)
  push(dr - 0.2, dr, home, null)
  sw(dr, 2, "Mon Feb 23  1:40 AM")
  at(dr + 0.02, () => w.set("joan", { data: { when: "Mon 1:40 AM", day: "Wed, Nov 26, 2025", msgs: [{ t: "Just putting mine out Wednesdays now, hope that's ok!" }], sent: SENT, sentAt: "Wed, Feb 18 at 9:14 PM", shownSent: 0, compose: "", caret: 1, unread: { "Pinecrest Neighbours · 9": 3 } } }))
  push(dr + 0.1, dr + 0.75, () => frameOf("joan", 1.06), "joan")
  SENT.forEach((_, i) => {
    const s = dr + 0.3 + i * 0.42
    at(s, () => w.set("joan", { data: { shownSent: i + 1, pop: 0 } }))
    span(s, s + 0.22, (u) => w.set("joan", { data: { pop: u } }))
    // (a seek replays cues, not spans: land the pop)
    at(s + 0.23, () => w.set("joan", { data: { pop: 1 } }))
  })
  // the letter
  const letter = dr + 2.35
  push(letter - 0.15, letter + 0.6, () => frameOf("postbox", 1.08), "postbox")
  at(letter - 0.2, () => {
    w.clock = "Thu Feb 26  7:05 PM"
    w.setFocus("postbox")
    const d = R("postbox").data
    w.set("postbox", { data: { list: [{ from: "Alan, Juliana and Dax", subject: "About the cameras", when: "7:02 PM", unread: 1 }, ...d.list], sel: -1, slide: 0 } })
  })
  span(letter - 0.2, letter + 0.15, (u) => w.set("postbox", { data: { slide: smooth(u) } }))
  clickAt(letter + 0.45, "postbox", (id) => [R(id).x.t + 120, R(id).y.t + BAR_H + 34], () =>
    w.set("postbox", { data: { sel: 0, open: { subject: "About the cameras", from: "Alan, Juliana and Dax", to: "No. 12", cc: "Pinecrest HOA Committee", when: "Feb 26, 7:02 PM", body: ["We'd like the cameras on No. 12", "that face our houses taken down.", "", "Alan (No. 10)", "Juliana (No. 16)", "Dax (No. 14)"] } } }),
  0.5)

  // ── "I'd kept seven empty, because it's useful to have somewhere to put things." ──
  const seven = word("I'd kept seven")
  push(seven - 0.3, seven + 0.05, home, null)
  sw(seven + 0.08, 7, "Thu Feb 26  11:40 PM")
  const things = word("somewhere to put")
  // a glimpse of his rules: Messages on two, the camera app floating, and a new one by window title
  float(seven + 0.3, { id: "rules7", app: "spaceman", kind: "settings", title: "Spaceman Settings", at: settingsAt, data: { rules: [{ app: "Messages", icon: "chatter", ws: 2 }, ...[["Front Door", 3], ["Alan", 4], ["Juliana", 5], ["Dax", 6]].map(([title, ws]) => ({ app: "Vigil", icon: "vigil", ws, title }))], floats: [{ app: "Vigil", icon: "vigil" }], active: [1, 2, 3, 4, 5, 6] } })
  push(seven + 0.35, seven + 0.9, settingsFrame, "rules7")
  at(seven + 0.95, () => {
    w.set("rules7", { data: { hl: 7, rules: [...R("rules7").data.rules, { app: "Vigil", icon: "vigil", ws: 7, title: "Cellar", in: 0 }] } })
    S.sfx("key", 0.8)
  })
  span(seven + 0.95, seven + 1.2, (u) => {
    const d = R("rules7").data
    w.set("rules7", { data: { rules: d.rules.map((r) => (r.title === "Cellar" ? { ...r, in: smooth(u) } : r)) } })
  })
  shut(things - 0.45, "rules7")
  float(things - 0.3, {
    id: "plastic", app: "browsy", kind: "browser", title: "Order placed", at: () => mid(720, 420),
    data: { tabs: 4, tabNames: ["Pinecrest HOA Bylaws", "Cat feeding guide", "6 mil poly sheeting"], url: "yardsupply.example/orders/YS-5531-0226", tab: "Order placed", heading: "Order placed. Thank you.", lines: ["Order YS-5531-0226 · placed Feb 26, 11:41 PM", { gap: 1 }, { t: "Heavy-duty plastic sheeting, 10 × 100 ft", b: 1, size: 17 }, "Clear · 6 mil", { gap: 1 }, "Arriving Saturday, Feb 28, by 8 PM"] },
  })
  push(things - 0.3, things + 0.35, () => frameOf("plastic", 1.12), "plastic")
  at(things - 0.3, () => (w.clock = "Thu Feb 26  11:41 PM"))
  // ── "I moved Juliana there in March," ──
  const movedJ = word("I moved Juliana")
  shut(movedJ - 0.15, "plastic", 0.4)
  const calAt = () => mid(760, 520)
  float(movedJ + 0.0, { id: "march", app: "daybook", kind: "daybook", title: "Calendar", at: calAt, data: { month: "March 2026", first: 0, days: 31, today: 2, events: [] } })
  // in on the first two weeks, so the entry reads
  push(movedJ, movedJ + 0.6, () => {
    const p = R("march")
    return fit(p.x.t, p.y.t, p.w.t * 0.62, p.h.t * 0.5, 1.04)
  }, "march")
  at(movedJ + 0.5, () => {
    // two days after the plastic came
    w.clock = "Mon Mar 2  2:20 AM"
    w.set("march", { data: { events: [{ day: 2, t: "Juliana → 7", color: "#e9c9c4" }] } })
    S.sfx("key", 0.95)
  })
  // "...and I've looked after her cat ever since." Her house, dark; the post; the cat at the door.
  const cat = word("and I've looked")
  shut(cat - 0.2, "march", 0.4)
  push(cat - 0.2, cat + 0.02, home, null)
  sw(cat, 5, "Sat Mar 7  9:30 PM")
  at(cat + 0.01, () => {
    for (const id of ["j-front", "j-door", "j-up"]) w.set(id, { data: { src: "st-16-cat", seq: { name: "cat", n: 20, fps: 4, t0: cat, loop: "pingpong", from: 6 } } })
    feedsAt("03-07-2026", hms(21, 30, 2), "SAT")
  })
  // a notification from the street's group, over her dark house: it sits, and goes, unclicked
  const NOTE = { app: "chatter", title: "Pinecrest Neighbours", body: "Alan (10): Has anyone seen Juliana? Her cat's been outside since Tuesday.", hold: 2.2 }
  at(cat + 0.2, () => w.notify(NOTE))
  // the street's group, eleven unread now; he leaves it that way
  at(cat + 0.31, () => w.set("group", { data: { unread: { "Pinecrest Neighbours · 9": 11 } } }))
  // ── "I moved Alan in June." ──
  const movedM = word("I moved Alan")
  // June 11 in the calendar; two nights on, on Alan's workspace, the drill quietly ticked off his list
  sw(movedM - 0.15, 4, "Thu Jun 11  11:52 PM")
  at(movedM - 0.14, () => feedsAt("06-11-2026", hms(23, 52, 14), "THU"))
  float(movedM - 0.05, { id: "june", app: "daybook", kind: "daybook", title: "Calendar", at: calAt, data: { month: "June 2026", first: 1, days: 30, today: 11, events: [] } })
  push(movedM - 0.05, movedM + 0.45, () => {
    const p = R("june")
    return fit(p.x.t + p.w.t * 0.38, p.y.t, p.w.t * 0.62, p.h.t * 0.55, 1.04)
  }, "june")
  at(movedM + 0.4, () => {
    w.clock = "Thu Jun 11  11:58 PM"
    w.set("june", { data: { events: [{ day: 11, t: "Alan → 7", color: "#e9c9c4" }] } })
    S.sfx("key", 0.95)
  })
  shut(movedM + 0.85, "june")
  at(movedM + 0.86, () => {
    w.clock = "Sat Jun 13  11:06 PM"
    feedsAt("06-13-2026", hms(23, 6, 3), "SAT")
  })
  push(movedM + 0.88, movedM + 1.35, () => frameOf("m-note", 1.12), "m-note")
  at(movedM + 1.42, () => {
    const d = R("m-note").data
    w.set("m-note", { data: { edited: "Edited June 13, 11:06 PM", lines: d.lines.map((l) => (l.drill ? { ...l, done: 1 } : l)) } })
    S.sfx("tap", 0.8)
  })
  // six, tonight: his light on, nobody in
  const still = wordEnd("in June") + 0.95
  shut(still - 0.2, "june", 0.4)
  push(still - 0.2, still, home, null)
  // from here the switches make no sound at all
  sw(still, 6, "Fri Nov 20  11:58 PM", true)
  at(still + 0.01, () => {
    // his lamp on, and nobody at the window now
    for (const id of ["a-front", "a-door", "a-up"]) w.set(id, { data: { seq: null } })
    feedsAt("11-20-2026", hms(23, 58, 4), "FRI")
  })
  push(still + 0.15, still + 1.0, () => frameOf("a-up", 1.15), "a-up")
  // "Where is Dax now?" "He's on seven, with the other two." Nothing moves.
  at(still + 0.3, () => w.cursorTo(S.SW * 0.9, S.SH * 0.86, still + 0.9))

  // ── "And what is on seven?" "Only the one window." ──
  const one = word("Only the one")
  at(one - 0.5, () => {
    w.veiled.delete(7)
    w.add({ ws: 7, id: "cellar", app: "vigil", kind: "feed", title: "Cellar", dark: 1, data: { src: "cam-cellar", name: "CELLAR", us: 1, ir: 1, q: 0.3, seed: 37, date: "11-20-2026", clock: hms(23, 58, 9) - w.time, wd: "FRI" } })
  })
  push(one - 0.12, one - 0.06, home, null)
  sw(one - 0.1, 7, null, true)
  at(one - 0.08, () => w.showBar())
  at(END, () => S.end())

  LOOKS = looks({ intro, tiled, late, house: mark, hedge, bins, still })

  cues.sort((a, b) => a.t - b.t)
  return { cues, spans }
}
