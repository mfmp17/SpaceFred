// The man's Mac: workspaces, tiled windows, the bar, the cursor and the menu bar, in logical screen
// units. draw() appends rounded-rect primitives to a display list that both renderers consume.
import { APPS, FONT, iconCanvas } from "./apps.js"
import { paintWindow, BAR_H } from "./content.js"
import { plates, blurOf, coverUV } from "./plates.js"
import { sp, to, jump, step, clamp } from "./springs.js"
import { feedClock, feedTick } from "./feeds.js"

export const MENU_H = 26
const GAP = 8
const EDGE_PAD = 10
// The HUD's Icons style (agent/src/hud/icons.rs): each workspace as its number and its apps' icons,
// the one you're on bright and the rest at 0.4, a marker 30 tall reaching 6 past its group
const BAR = { edge: 14, icon: 16, iconGap: 3, numGap: 5, groupGap: 14, pad: 6, markerH: 30, num: 9, dim: 0.4, inner: 42, maxApps: 12 }
const groupW = (apps) => (apps ? BAR.num + BAR.numGap + apps * BAR.icon + (apps - 1) * BAR.iconGap : BAR.num)
// A tiler's moves are short: windows land in about a third of a second, a beat apart.
// a tile's frame settles in about 0.4 s, critically damped (no overshoot on size: text would shimmer)
const TILE_D = 0.38
// a window arriving: from 94% with the smallest settle, faded in a little ahead of its size
const OPEN_D = 0.34
const OPEN_BOUNCE = 0.3
const STAGGER = 0.024

export function tile(n, a) {
  const out = []
  if (!n) return out
  const portrait = a.h > a.w * 1.05
  const T = (x, y, w, h) => (portrait ? { x: a.x + y * a.w, y: a.y + x * a.h, w: h * a.w, h: w * a.h } : { x: a.x + x * a.w, y: a.y + y * a.h, w: w * a.w, h: h * a.h })
  // fractions in a "landscape" frame, transposed for portrait areas
  const fr = []
  if (n === 1) fr.push([0, 0, 1, 1])
  else if (n === 2) fr.push([0, 0, 0.5, 1], [0.5, 0, 0.5, 1])
  else if (n === 3) fr.push([0, 0, 0.5, 1], [0.5, 0, 0.5, 0.5], [0.5, 0.5, 0.5, 0.5])
  else {
    const aspect = portrait ? a.h / a.w : a.w / a.h
    // rows as even as they can be, the fuller rows last: 7 is 3 over 4, never 3, 3 and a lone 1
    const rows = n === 4 ? 2 : Math.max(1, Math.round(Math.sqrt(n / (aspect * 0.8))))
    let k = 0
    for (let r = 0; r < rows; r++) {
      const inRow = Math.floor(n / rows) + (r >= rows - (n % rows) ? 1 : 0)
      for (let c = 0; c < inRow; c++, k++) fr.push([c / inRow, r / rows, 1 / inRow, 1 / rows])
    }
  }
  for (const [x, y, w, h] of fr) {
    const r = T(x, y, w, h)
    out.push({ x: r.x + GAP / 2, y: r.y + GAP / 2, w: r.w - GAP, h: r.h - GAP })
  }
  return out
}

// The pin on a floating window's bar: a capsule with a pushpin and "Floating", drawn once at 2x.
let pinC = null
function pinCanvas() {
  if (pinC || typeof document === "undefined") return pinC
  const k = 2
  const label = "Floating"
  const c = document.createElement("canvas")
  const ctx = c.getContext("2d")
  ctx.font = `600 ${10.5 * k}px ${FONT}`
  const tw = ctx.measureText(label).width
  c.width = Math.ceil(tw + 34 * k)
  c.height = 18 * k
  ctx.font = `600 ${10.5 * k}px ${FONT}`
  ctx.fillStyle = "rgba(255,255,255,.14)"
  ctx.beginPath()
  ctx.roundRect(0, 0, c.width, c.height, c.height / 2)
  ctx.fill()
  ctx.strokeStyle = "rgba(255,255,255,.22)"
  ctx.lineWidth = k
  ctx.beginPath()
  ctx.roundRect(k / 2, k / 2, c.width - k, c.height - k, c.height / 2)
  ctx.stroke()
  // a pushpin, head up, tilted
  ctx.save()
  ctx.translate(12 * k, 9 * k)
  ctx.rotate(0.6)
  ctx.fillStyle = "#ffd36a"
  ctx.beginPath()
  ctx.roundRect(-3 * k, -6 * k, 6 * k, 5 * k, 1.5 * k)
  ctx.fill()
  ctx.fillRect(-4.2 * k, -1.6 * k, 8.4 * k, 1.6 * k)
  ctx.strokeStyle = "rgba(255,255,255,.85)"
  ctx.lineWidth = 1.2 * k
  ctx.beginPath()
  ctx.moveTo(0, 0)
  ctx.lineTo(0, 5.5 * k)
  ctx.stroke()
  ctx.restore()
  ctx.fillStyle = "rgba(255,255,255,.9)"
  ctx.textBaseline = "middle"
  ctx.fillText(label, 22 * k, c.height / 2 + 0.5 * k)
  return (pinC = c)
}

export class World {
  constructor(SW, SH, opts = {}) {
    this.SW = SW
    this.SH = SH
    this.area = { x: EDGE_PAD - GAP / 2, y: MENU_H + EDGE_PAD - GAP / 2, w: SW - 2 * EDGE_PAD + GAP, h: SH - MENU_H - 2 * EDGE_PAD - 80 + GAP }
    this.wins = new Map()
    this.order = new Map() // ws -> [ids] in tile order
    this.active = 1
    this.focus = null
    this.tiling = false
    this.res = 1
    this.z = 0
    this.clock = "Tue 11:48 PM"
    this.status = false
    this.frontApp = "Jot"
    this.menuCanvas = null
    this.menuKey = ""
    this.bar = { shown: sp(0, 0.42), scale: sp(1, 0.4), cells: [], left: sp(0, 0.3), right: sp(0, 0.3), tw: sp(0, 0.35) }
    // workspaces whose group in the bar shows none of its apps until you go there
    this.veiled = new Set()
    this.cursor = { x: SW * 0.62, y: SH * 0.55, vx: 0, vy: 0, path: null, a: sp(1, 0.3), press: sp(0, 0.09) }
    this.keys = { label: null, a: sp(0, 0.2), until: 0 }
    this.keysAbove = false
    this.time = 0
    this.lastFocus = {}
    this.reduced = !!opts.reduced
    this.repaints = 0
  }

  get wall() {
    return plates.wall || plates["wall-s"] || fallbackWall()
  }

  add(def) {
    const w = {
      ws: 1,
      float: false,
      ...def,
      x: sp(def.at?.[0] ?? 100, TILE_D),
      y: sp(def.at?.[1] ?? 100, TILE_D),
      w: sp(def.at?.[2] ?? 480, TILE_D),
      h: sp(def.at?.[3] ?? 340, TILE_D),
      a: sp(def.hidden ? 0 : 1, 0.2),
      s: sp(1, 0.26),
      off: sp(0, 0.2),
      fo: sp(0, 0.24),
      // lift: 1 for a float, which rises over the tiles (a few points up, a softer, wider shadow)
      lf: sp(def.float ? 1 : 0, 0.42),
      z: ++this.z,
      canvas: null,
      dirty: true,
      cw: 0,
      ch: 0,
      leaving: false,
    }
    this.wins.set(w.id, w)
    if (!w.float) this.listOf(w.ws).push(w.id)
    return w
  }

  listOf(ws) {
    let l = this.order.get(ws)
    if (!l) this.order.set(ws, (l = []))
    return l
  }

  rectsOf(ws) {
    const ids = this.listOf(ws).filter((id) => !this.wins.get(id).leaving)
    const rs = tile(ids.length, this.area)
    return new Map(ids.map((id, i) => [id, rs[i]]))
  }

  // Retarget every tiled window on the active workspace to its tile, a beat apart in tile order.
  reflow({ snap = false, delay = 0 } = {}) {
    if (!this.tiling) return
    let i = 0
    for (const [id, r] of this.rectsOf(this.active)) {
      const w = this.wins.get(id)
      if (snap) for (const k of ["x", "y", "w", "h"]) jump(w[k], r[k])
      else if (Math.abs(w.x.t - r.x) + Math.abs(w.y.t - r.y) + Math.abs(w.w.t - r.w) + Math.abs(w.h.t - r.h) > 0.5) {
        w.target = r
        w.delay = delay + i++ * STAGGER
      }
    }
  }

  tileNow() {
    this.tiling = true
    this.reflow()
  }

  setFocus(id) {
    const prev = this.wins.get(this.focus)
    if (prev && prev.id !== id) {
      prev.dirty = true
      to(prev.fo, 0)
    }
    this.focus = id
    const w = this.wins.get(id)
    if (w) {
      w.z = ++this.z
      w.dirty = true
      to(w.fo, 1)
      this.frontApp = APPS[w.app]?.name || "Jot"
      this.lastFocus[w.ws] = id
    }
  }

  // Change a window's title or contents; it repaints on the next frame.
  set(id, patch) {
    const w = this.wins.get(id)
    if (!w) return
    if (patch.data) w.data = { ...w.data, ...patch.data }
    if (patch.title != null) w.title = patch.title
    w.dirty = true
  }

  open(def, { focus = true } = {}) {
    const w = this.add({ ...def, hidden: true })
    const r = (!w.float && this.tiling && this.rectsOf(w.ws).get(w.id)) || { x: def.at[0], y: def.at[1], w: def.at[2], h: def.at[3] }
    for (const k of ["x", "y", "w", "h"]) jump(w[k], r[k])
    jump(w.s, 0.94)
    to(w.s, 1, OPEN_D, this.reduced ? 0 : OPEN_BOUNCE)
    to(w.a, 1, 0.16)
    // a float rises off the tiles as it arrives, swelling a hair and settling
    if (w.float && !this.reduced) {
      jump(w.lf, 0)
      to(w.lf, 1, 0.42, 0.25)
    }
    if (w.ws === this.active) this.reflow()
    if (focus) this.setFocus(w.id)
    return w
  }

  // Several windows at once (a workspace being filled): each goes straight to its final tile and they
  // appear a beat apart, so nothing already on screen is moved twice.
  openMany(defs, { gap = 0.16 } = {}) {
    const ws = defs[0].ws
    const made = defs.map((d) => this.add({ ...d, hidden: true }))
    const rs = this.rectsOf(ws)
    made.forEach((m, i) => {
      const r = (this.tiling && rs.get(m.id)) || { x: m.at?.[0] ?? 100, y: m.at?.[1] ?? 100, w: m.at?.[2] ?? 480, h: m.at?.[3] ?? 340 }
      for (const k of ["x", "y", "w", "h"]) jump(m[k], r[k])
      jump(m.s, 0.94)
      m.revealAt = this.time + i * gap
    })
    if (ws === this.active) this.reflow()
    return made
  }

  // A workspace's group in the bar swells for a moment: something just arrived there.
  pulse(n) {
    const c = this.bar.cells.find((c) => c.n === n)
    if (!c) return
    to(c.s, 1.18, 0.12)
    to(c.o, 1, 0.12)
    c.pulseEnd = this.time + 0.32
  }

  // The window fades and shrinks a little; its neighbours wait a beat, then close the gap.
  close(id) {
    const w = this.wins.get(id)
    if (!w) return
    w.leaving = true
    // a closed window is gone almost at once, as on a Mac; the others don't grow over its ghost
    to(w.s, 0.95, 0.16)
    to(w.a, 0, 0.11)
    w.removeAt = this.time + 0.22
    if (this.focus === id) this.focus = null
    this.reflow({ delay: 0.06 })
  }

  // Option+Shift+n: the window leaves for workspace n, shrinking into its app's icon in the bar.
  move(id, ws) {
    const w = this.wins.get(id)
    const from = this.listOf(w.ws)
    if (from.includes(id)) from.splice(from.indexOf(id), 1)
    w.float = false
    this.ensureCell(ws)
    w.ws = ws
    this.listOf(ws).push(id)
    w.flying = this.time + 0.42
    const c = this.iconRect(ws, w.app)
    w.target = null
    if (this.reduced) {
      to(w.a, 0, 0.24)
    } else {
      // one duration for every edge, so no edge doubles back on its way into the bar
      for (const k of ["x", "y", "w", "h"]) to(w[k], c[k], 0.34)
      to(w.a, 0, 0.3)
    }
    if (this.focus === id) {
      const next = [...this.rectsOf(this.active).keys()].pop()
      if (next) this.setFocus(next)
    }
    this.reflow({ delay: 0.05 })
  }

  switchTo(ws, { quiet } = {}) {
    if (ws === this.active) return
    const dir = this.reduced ? 0 : Math.sign(ws - this.active)
    this.ensureCell(ws)
    // (a pinned float, one a float rule keeps above the tiles, belongs to its workspace and leaves with
    // it; the film's other floats are overlays and stay up)
    for (const w of this.wins.values()) {
      if ((w.float && !w.pin) || w.leaving) continue
      if (w.ws === this.active) {
        // the old workspace is gone before the new one has arrived, so the two never read as a dissolve
        to(w.off, -dir * 32 || -0.001, 0.12)
        to(w.a, 0, 0.055)
      }
    }
    const prev = this.active
    this.active = ws
    for (const [id, r] of this.rectsOf(ws)) {
      const w = this.wins.get(id)
      w.target = null
      for (const k of ["x", "y", "w", "h"]) jump(w[k], r[k])
      jump(w.off, dir * 32)
      to(w.off, 0, 0.22)
      jump(w.a, 0)
      to(w.a, 1, 0.15)
      jump(w.s, 1)
      if (Math.abs(w.cw - r.w) > 0.5 || Math.abs(w.ch - r.h) > 0.5) w.dirty = true
    }
    for (const w of this.wins.values()) {
      if (!w.pin || w.leaving || w.ws !== ws) continue
      jump(w.off, dir * 32)
      to(w.off, 0, 0.22)
      jump(w.a, 0)
      to(w.a, 1, 0.15)
    }
    const ids = [...this.rectsOf(ws).keys()]
    const keep = this.lastFocus?.[ws]
    if (ids.length) this.setFocus(ids.includes(keep) ? keep : ids[ids.length - 1])
    this.barGo(prev, ws)
    if (!quiet) this.keyCast(["⌃", String(ws % 10)])
  }

  // A macOS notification banner: in from the right edge, it sits, and goes back the way it came.
  notify(n) {
    const x = sp(1.06, 0.45)
    to(x, 0, 0.45, 0.12)
    this.note = { ...n, x, out: this.time + 0.45 + (n.hold ?? 2.2) }
  }

  keyCast(label, hold = 1.0) {
    const same = this.keys.label && this.keys.a.t > 0 && this.keys.label.length === label.length
    this.keys.label = label
    this.keys.until = this.time + hold
    to(this.keys.a, 1)
    this.keys.canvas = null
    if (!same) this.keys.pop = this.time
  }

  // ─── Bar ────────────────────────────────────────────────────────────────
  ensureCell(ws) {
    while (this.bar.cells.length < ws) {
      const n = this.bar.cells.length + 1
      // a workspace joins the row in place, fading and scaling up from 0.9
      this.bar.cells.push({ n, s: sp(0.9, 0.3), o: sp(0, 0.3), x: sp(0, 0.35), w: sp(BAR.num, 0.35), icons: new Map(), fresh: true })
    }
    this.layoutBar()
  }

  // Each app once, in tile order, then the floating ones; nothing of a veiled workspace you're not on.
  appsOf(n) {
    if (this.veiled.has(n) && n !== this.active) return []
    const out = []
    const add = (w) => {
      if (!w || w.leaving || w.flying || w.revealAt !== undefined || (n !== this.active && !w.seen)) return
      if (!out.includes(w.app) && out.length < BAR.maxApps) out.push(w.app)
    }
    for (const id of this.rectsOf(n).keys()) add(this.wins.get(id))
    for (const w of this.wins.values()) if (w.float && w.ws === n && w.a.t > 0) add(w)
    return out
  }

  // Where every group, icon and the marker are headed; the springs carry them there.
  layoutBar() {
    const b = this.bar
    let x = BAR.edge
    for (const c of b.cells) {
      const apps = this.appsOf(c.n)
      const gw = groupW(apps.length)
      if (c.fresh) {
        jump(c.x, x)
        jump(c.w, gw)
        c.fresh = false
      } else {
        to(c.x, x)
        to(c.w, gw)
      }
      if (!c.pulseEnd) {
        to(c.s, 1, 0.3)
        to(c.o, c.n === this.active ? 1 : BAR.dim, 0.3)
      }
      apps.forEach((app, i) => {
        const ix = BAR.num + BAR.numGap + i * (BAR.icon + BAR.iconGap)
        let ic = c.icons.get(app)
        if (!ic) c.icons.set(app, (ic = { x: sp(ix, 0.35), a: sp(0, 0.2), s: sp(0.6, 0.3) }))
        to(ic.x, ix)
        to(ic.a, 1, 0.2)
        to(ic.s, 1, 0.3)
      })
      for (const [app, ic] of c.icons) {
        if (apps.includes(app)) continue
        to(ic.a, 0, 0.15)
        to(ic.s, 0.6, 0.2)
        if (ic.a.x < 0.01) c.icons.delete(app)
      }
      if (c.n === this.active) {
        to(b.left, x - BAR.pad)
        to(b.right, x + gw + BAR.pad)
      }
      x += gw + BAR.groupGap
    }
    to(b.tw, b.cells.length ? x - BAR.groupGap + BAR.edge : 2 * BAR.edge)
  }

  // The marker: its leading edge 0.26 s, its trailing edge 0.42 s; the groups brighten and dim in 0.3 s.
  barGo(from, ws) {
    const dir = Math.sign(ws - from)
    const b = this.bar
    to(b.left, b.left.t, dir > 0 ? 0.42 : dir < 0 ? 0.26 : 0.3)
    to(b.right, b.right.t, dir > 0 ? 0.26 : dir < 0 ? 0.42 : 0.3)
    this.layoutBar()
  }

  showBar() {
    this.ensureCell(Math.max(1, this.active))
    const b = this.bar
    // it pops in: up from below, a little small, landing with a slight overshoot
    if (b.shown.t < 1 && !this.reduced) {
      const s = b.scale.t
      b.scale.x = s * 0.84
      to(b.scale, s, 0.42, 0.28)
    }
    const hidden = b.shown.x < 0.01
    to(b.shown, 1, 0.34)
    this.layoutBar()
    // shown from nothing, everything lands as drawn
    if (hidden) {
      jump(b.tw, b.tw.t)
      for (const c of b.cells) {
        jump(c.x, c.x.t)
        jump(c.w, c.w.t)
        for (const ic of c.icons.values()) for (const k of ["x", "a", "s"]) jump(ic[k], ic[k].t)
      }
    }
    jump(b.left, b.left.t)
    jump(b.right, b.right.t)
  }

  barGeom() {
    const sc = this.bar.scale.x
    const h = (BAR.inner + 2) * sc
    const w = (this.bar.tw.x + 2) * sc
    const x = this.SW / 2 - w / 2
    const y = this.SH - 12 - h + (1 - this.bar.shown.x) * 70
    return { x, y, w, h, sc }
  }

  // A workspace's group (its number and icons) on screen, for taps.
  cellRect(ws) {
    const g = this.barGeom()
    const c = this.bar.cells[ws - 1]
    const y = g.y + (1 + (BAR.inner - BAR.icon) / 2) * g.sc
    if (!c) return { x: g.x + g.w - 30 * g.sc, y, w: BAR.icon * g.sc, h: BAR.icon * g.sc }
    return { x: g.x + (1 + c.x.x) * g.sc, y, w: c.w.x * g.sc, h: BAR.icon * g.sc }
  }

  // Where an app's icon will sit in workspace ws's group once it lands, for a window flying there.
  iconRect(ws, app) {
    const b = this.bar
    const c = b.cells[ws - 1]
    const apps = this.appsOf(ws)
    let i = apps.indexOf(app)
    let grow = 0
    if (i < 0) {
      i = apps.length
      grow = apps.length ? BAR.icon + BAR.iconGap : BAR.numGap + BAR.icon
    }
    const sc = b.scale.t
    const x0 = this.SW / 2 - ((b.tw.t + grow + 2) * sc) / 2
    const lx = (c ? c.x.t : 0) + BAR.num + BAR.numGap + i * (BAR.icon + BAR.iconGap)
    const g = this.barGeom()
    return { x: x0 + (1 + lx) * sc, y: g.y + (1 + (BAR.inner - BAR.icon) / 2) * sc, w: BAR.icon * sc, h: BAR.icon * sc }
  }

  // ─── Simulation ─────────────────────────────────────────────────────────
  update(dt) {
    this.time += dt
    feedClock(this.time)
    for (const w of [...this.wins.values()]) {
      // a camera window repaints at its camera's frame rate while it can be seen
      if (w.kind === "feed" && (w.ws === this.active || w.flying) && w.a.x > 0.01) {
        const k = feedTick(w.data || {})
        if (k !== w.tick) {
          w.tick = k
          w.dirty = true
        }
      }
      if (w.target) {
        w.delay -= dt
        if (w.delay <= 0) {
          for (const k of ["x", "y", "w", "h"]) to(w[k], w.target[k], TILE_D)
          w.dirty = true
          w.target = null
        }
      }
      for (const k of ["x", "y", "w", "h", "a", "s", "off", "fo", "lf"]) step(w[k], dt)
      if (w.a.x > 0.5) w.seen = true
      if (w.removeAt && this.time >= w.removeAt) {
        this.wins.delete(w.id)
        const l = this.listOf(w.ws)
        if (l.includes(w.id)) l.splice(l.indexOf(w.id), 1)
      }
      if (w.flying && this.time >= w.flying) w.flying = 0
      if (w.revealAt !== undefined && this.time >= w.revealAt) {
        w.revealAt = undefined
        to(w.s, 1, OPEN_D, this.reduced ? 0 : OPEN_BOUNCE)
        to(w.a, 1, 0.16)
        if (w.ws === this.active) this.setFocus(w.id)
      }
      // once a workspace's windows have faded out, they rest where they are
      if ((!w.float || w.pin) && w.ws !== this.active && w.a.t === 0 && w.a.x < 0.002 && w.off.x !== 0) jump(w.off, 0)
    }
    const b = this.bar
    for (const c of b.cells) if (c.pulseEnd && this.time >= c.pulseEnd) c.pulseEnd = 0
    if (b.cells.length) this.layoutBar()
    for (const k of ["shown", "scale", "left", "right", "tw"]) step(b[k], dt)
    for (const c of b.cells) {
      for (const k of ["s", "o", "x", "w"]) step(c[k], dt)
      for (const ic of c.icons.values()) {
        step(ic.x, dt)
        step(ic.a, dt)
        step(ic.s, dt)
      }
    }
    const nt = this.note
    if (nt) {
      if (this.time >= nt.out && nt.x.t === 0) to(nt.x, 1.06, 0.4)
      step(nt.x, dt)
      if (nt.x.t > 1 && nt.x.x > 1.05) this.note = null
    }
    this.stepCursor(dt)
    if (this.keys.label && this.time > this.keys.until) to(this.keys.a, 0)
    step(this.keys.a, dt)
  }

  // The cursor travels on a slight arc, speeding up and settling (a Hermite curve that starts with
  // whatever velocity it had), and takes longer for longer trips, the way a hand does.
  moveCursor(x, y, d) {
    const c = this.cursor
    const dx = x - c.x
    const dy = y - c.y
    const dist = Math.hypot(dx, dy)
    if (dist < 0.5) return
    const dur = d ?? cursorTime(dist)
    let nx = -dy / dist
    let ny = dx / dist
    if (ny > 0 || (Math.abs(ny) < 0.2 && nx * dx < 0)) {
      nx = -nx
      ny = -ny
    }
    c.path = { x0: c.x, y0: c.y, x1: x, y1: y, mx: c.vx * dur, my: c.vy * dur, t0: this.time, dur, bx: nx * dist * 0.085, by: ny * dist * 0.085 }
  }

  // Set off in time to arrive at arriveAt (show time), or now if the trip needs longer.
  cursorTo(x, y, arriveAt) {
    const c = this.cursor
    const dist = Math.hypot(x - c.x, y - c.y)
    if (dist < 0.5) return
    // a trip that would land late is made quicker, so the click never lands on a moving cursor
    const dur = Math.max(0.22, Math.min(cursorTime(dist), arriveAt - this.time))
    // a hand already moving doesn't stop to wait: it carries on to the new point
    const t0 = c.path ? this.time : Math.max(this.time, arriveAt - dur)
    this.moveCursor(x, y, dur)
    const p = this.cursor.path
    if (t0 > this.time + 1e-4) {
      // waiting to move: no carried velocity
      p.t0 = t0
      p.mx = p.my = 0
    }
  }

  stepCursor(dt) {
    const c = this.cursor
    step(c.a, dt)
    if (c.release && this.time >= c.release) {
      c.release = 0
      to(c.press, 0, 0.16)
    }
    step(c.press, dt)
    const p = c.path
    if (!p) {
      c.vx = c.vy = 0
      return
    }
    // quintic Hermite: carried velocity at the start, and both speed and acceleration fall to zero
    // at the end, so the hand eases into its spot instead of stopping on it
    const s = clamp((this.time - p.t0) / p.dur)
    const s3 = s * s * s
    const h01 = s3 * (10 - 15 * s + 6 * s * s)
    const h00 = 1 - h01
    const h10 = s - 6 * s3 + 8 * s3 * s - 3 * s3 * s * s
    const bulge = Math.sin(Math.PI * s) ** 2
    const x = h00 * p.x0 + h10 * p.mx + h01 * p.x1 + p.bx * bulge
    const y = h00 * p.y0 + h10 * p.my + h01 * p.y1 + p.by * bulge
    c.vx = (x - c.x) / dt
    c.vy = (y - c.y) / dt
    c.x = x
    c.y = y
    if (s >= 1) {
      c.path = null
      c.vx = c.vy = 0
    }
  }

  // arrow, hand (over a link) or I-beam (over text); hidden while he types, back when he moves
  cursorKind(kind) {
    this.cursor.kind = kind
  }
  hideCursor(on) {
    to(this.cursor.a, on ? 0 : 1, 0.08)
  }

  // pressed for a moment, then let go
  click() {
    to(this.cursor.press, 1, 0.06)
    this.cursor.release = this.time + 0.085
  }

  // ─── Drawing ────────────────────────────────────────────────────────────
  // Window canvases follow the camera's zoom (device pixels x zoom), so text stays sharp in a
  // push. A canvas is only re-rasterised when its scale is 20% off, at most two per frame for
  // zoom alone, the focused window first.
  ensureCanvas(w) {
    const tw = Math.max(40, w.w.t)
    const th = Math.max(40, w.h.t)
    const cap = w.kind === "feed" ? 1.4e6 : w.id === this.focus || w.float ? 7e6 : 2.6e6
    const want = Math.max(0.5, Math.min(this.res, Math.sqrt(cap / (tw * th))))
    const sizeChanged = Math.abs(w.cw - tw) > 0.5 || Math.abs(w.ch - th) > 0.5
    const resOff = !w.res || want > w.res * 1.2 || want < w.res / 1.6
    if (!w.dirty && !sizeChanged && w.canvas && !resOff) return
    if (w.flying && w.canvas) return
    if (!w.dirty && !sizeChanged && w.canvas && resOff) {
      if (this.repaints >= 2) return
      this.repaints++
    }
    const res = Math.round(want * 8) / 8
    w.cw = tw
    w.ch = th
    w.res = res
    if (!w.canvas) w.canvas = document.createElement("canvas")
    const pw = Math.ceil(tw * res)
    const ph = Math.ceil(th * res)
    if (w.canvas.width !== pw || w.canvas.height !== ph) {
      w.canvas.width = pw
      w.canvas.height = ph
    }
    const ctx = w.canvas.getContext("2d")
    ctx.setTransform(res, 0, 0, res, 0, 0)
    paintWindow(ctx, tw, th, w, w.id === this.focus)
    w.canvas.version = (w.canvas.version || 0) + 1
    w.dirty = false
  }

  menu() {
    const res = Math.min(4, this.res)
    // the Spaceman item fades and grows in over a quarter second when it first appears
    const sIn = this.status ? Math.min(1, Math.max(0, (this.time - (this.statusIn ?? -9)) / 0.28)) : 0
    const key = this.clock + this.frontApp + sIn.toFixed(2) + this.active + res
    if (this.menuCanvas && key === this.menuKey) return this.menuCanvas
    this.menuKey = key
    const c = this.menuCanvas || (this.menuCanvas = document.createElement("canvas"))
    c.width = Math.ceil(this.SW * res)
    c.height = Math.ceil(MENU_H * res)
    const ctx = c.getContext("2d")
    ctx.setTransform(res, 0, 0, res, 0, 0)
    ctx.clearRect(0, 0, this.SW, MENU_H)
    // a light frosted strip: the blurred desktop shows through underneath
    ctx.fillStyle = "rgba(246,243,238,.58)"
    ctx.fillRect(0, 0, this.SW, MENU_H)
    ctx.fillStyle = "rgba(0,0,0,.12)"
    ctx.fillRect(0, MENU_H - 1 / res, this.SW, 1 / res)
    const ink = "#1f1e1c"
    ctx.fillStyle = ink
    ctx.textBaseline = "middle"
    const mid = MENU_H / 2 + 0.5
    ctx.font = `700 13px ${FONT}`
    ctx.textAlign = "left"
    ctx.fillText(this.frontApp, 16, mid)
    let x = 30 + ctx.measureText(this.frontApp).width
    ctx.font = `400 13px ${FONT}`
    if (this.SW > 760)
      for (const m of ["File", "Edit", "View", "Window", "Help"]) {
        ctx.fillText(m, x, mid)
        x += ctx.measureText(m).width + 19
      }
    ctx.textAlign = "right"
    ctx.font = `500 13px ${FONT}`
    let r = this.SW - 14
    ctx.fillText(this.clock, r, mid)
    r -= ctx.measureText(this.clock).width + 16
    // status items, right to left: battery, wifi, then Spaceman's workspace number
    battery(ctx, r - 24, mid, ink)
    r -= 40
    wifi(ctx, r - 9, mid + 5, ink)
    r -= 30
    if (sIn > 0) {
      const e = 1 - (1 - sIn) ** 3
      const sx = r - 22
      ctx.save()
      ctx.globalAlpha = e
      ctx.translate(sx + 11, mid)
      ctx.scale(0.7 + 0.3 * e, 0.7 + 0.3 * e)
      ctx.translate(-(sx + 11), -mid)
      ctx.fillStyle = ink
      ctx.beginPath()
      ctx.roundRect(sx, 5.5, 22, 15, 4)
      ctx.fill()
      ctx.fillStyle = "#f4f1ec"
      ctx.font = `700 11px ${FONT}`
      ctx.textAlign = "center"
      ctx.fillText(String(this.active % 10), sx + 11, mid)
      ctx.restore()
    }
    c.version = (c.version || 0) + 1
    return c
  }

  keyCanvas() {
    if (this.keys.canvas) return this.keys.canvas
    const c = (this.keys.canvas = document.createElement("canvas"))
    const res = Math.min(4, this.res)
    const label = this.keys.label || []
    const kw = 44
    const W = label.length * (kw + 8) + 16
    c.width = Math.ceil(W * res)
    c.height = Math.ceil(60 * res)
    c.lw = W
    c.lh = 60
    const ctx = c.getContext("2d")
    ctx.setTransform(res, 0, 0, res, 0, 0)
    ctx.fillStyle = "rgba(22,22,25,.62)"
    ctx.beginPath()
    ctx.roundRect(0, 0, W, 60, 15)
    ctx.fill()
    label.forEach((k, i) => {
      const x = 8 + i * (kw + 8)
      const g = ctx.createLinearGradient(0, 8, 0, 52)
      g.addColorStop(0, "rgba(255,255,255,.17)")
      g.addColorStop(1, "rgba(255,255,255,.09)")
      ctx.fillStyle = g
      ctx.beginPath()
      ctx.roundRect(x, 8, kw, 44, 9)
      ctx.fill()
      ctx.fillStyle = "rgba(255,255,255,.22)"
      ctx.fillRect(x + 6, 8, kw - 12, 1 / res)
      ctx.fillStyle = "#f5f3ee"
      ctx.font = `500 ${k.length > 1 ? 15 : 21}px ${FONT}`
      ctx.textAlign = "center"
      ctx.textBaseline = "middle"
      ctx.fillText(k, x + kw / 2, 31)
    })
    c.version = 1
    return c
  }

  // T maps logical units to CSS px: px = x * T.k + T.ox. clip is in CSS px; g grades; am scales
  // alpha. part is "base" (wallpaper and windows), "top" (menu bar, bar, keys, cursor) or "all".
  draw(list, T, { clip = null, g = 1, am = 1, part = "all" } = {}) {
    this.repaints = 0
    const P = (x, y, w, h, o) => list.push({ g, ...o, x: x * T.k + T.ox, y: y * T.k + T.oy, w: w * T.k, h: h * T.k, clip, a: (o.a ?? 1) * am, r: (o.r ?? 0) * T.k, ring: o.ring ? o.ring * T.k : 0, shadow: o.shadow ? o.shadow * T.k : 0 })
    this.hair = 1 / Math.max(1, this.res)
    this.wallUV = coverUV(this.wall, this.SW, this.SH, 0.5, 0.42)
    if (part !== "top") this.drawBase(P)
    if (part !== "base") this.drawTop(P)
  }

  // the blurred desktop under a rect, for frosted materials
  frost(P, x, y, w, h, o) {
    const [u0, v0, u1, v1] = this.wallUV
    const U = (px) => u0 + (px / this.SW) * (u1 - u0)
    const V = (py) => v0 + (py / this.SH) * (v1 - v0)
    P(x, y, w, h, { ...o, tex: blurOf(this.wall), uv: [U(x), V(y), U(x + w), V(y + h)] })
  }

  drawBase(P) {
    P(0, 0, this.SW, this.SH, { tex: this.wall, uv: this.wallUV, r: 0 })
    const wins = [...this.wins.values()].sort((a, b) => (a.float ? 1e6 : 0) + a.z - ((b.float ? 1e6 : 0) + b.z))
    const hair = this.hair
    for (const w of wins) {
      if (w.a.x < 0.003) continue
      if ((!w.float || w.pin) && w.ws !== this.active && !w.flying && w.off.x === 0) continue
      this.ensureCanvas(w)
      // a float's lift: up a few points, with a 1.5% swell at the middle of the rise
      const lift = w.float ? clamp(w.lf.x, 0, 1.2) : 0
      const s = w.s.x * (1 + 0.015 * Math.sin(Math.PI * clamp(w.lf.x)) * (w.float ? 1 : 0))
      const ww = w.w.x * s
      const hh = w.h.x * s
      const x = w.x.x + (w.w.x - ww) / 2 + w.off.x
      const y = w.y.x + (w.h.x - hh) / 2 - 3 * lift
      const r = Math.min(11, ww * 0.1, hh * 0.1)
      const a = w.a.x
      const f = w.fo.x
      // once tiled, the window you're in lifts and the rest step back: an accent edge and a soft glow
      // on it, the others dimmed a sixth
      const tiled = this.tiling && !w.float
      const hi = tiled ? f : 0
      // two shadows: a wide soft one that grows with focus and height, and a tight contact one
      P(x, y, ww, hh, { shadow: 22 + 16 * f + 26 * lift, col: [0, 0, 0, 0.16 + 0.12 * f + 0.12 * lift], r, a })
      P(x, y, ww, hh, { shadow: 4 + 2 * f, col: [0, 0, 0, 0.26 + 0.06 * f], r, a })
      if (hi > 0.01) P(x, y, ww, hh, { shadow: 16, col: [0.16, 0.5, 1, 0.22 * hi], r, a })
      this.frost(P, x, y, ww, hh, { r, a })
      // the content is laid out at the size the window is going to, and a resize reveals or crops it
      // from the top left the way an app re-lays itself, instead of stretching the old pixels
      const cwS = (w.cw || ww) * s
      const chS = (w.ch || hh) * s
      const dw = Math.min(ww, cwS)
      const dh = Math.min(hh, chS)
      if (dw < ww - 0.5 || dh < hh - 0.5) P(x, y, ww, hh, { tex: w.canvas, uv: [0.96, 0.96, 0.965, 0.965], r, a })
      P(x, y, dw, dh, { tex: w.canvas, uv: [0, 0, dw / cwS, dh / chS], r, a })
      if (tiled && f < 0.99) P(x, y, ww, hh, { col: [0.02, 0.02, 0.03, 0.17 * (1 - f)], r, a })
      // device-pixel hairlines: a dark edge outside, a faint light edge inside
      P(x - hair, y - hair, ww + 2 * hair, hh + 2 * hair, { ring: hair, col: [0, 0, 0, 0.3 + 0.08 * f], r: r + hair, a })
      P(x, y, ww, hh, { ring: hair, col: [1, 1, 1, w.dark ? 0.1 : 0.42], r, a })
      if (hi > 0.01) P(x - 0.5, y - 0.5, ww + 1, hh + 1, { ring: 1.5, col: [0.2, 0.55, 1, 0.7 * hi], r: r + 0.5, a })
      // a window a float rule keeps above the tiles: held a little higher, and a small pin on its bar
      if (w.pin) {
        const b = pinCanvas()
        if (b) {
          const bw = (b.width / 2) * s
          const bh = (b.height / 2) * s
          // right of the traffic lights, however narrow the window
          const bx = Math.max(x + 64 * s, x + ww - bw - 8 * s)
          const by = y + (BAR_H * s - bh) / 2
          P(bx, by, bw, bh, { tex: b, r: bh / 2, a })
        }
      }
    }
  }

  drawTop(P) {
    this.frost(P, 0, 0, this.SW, MENU_H, { r: 0 })
    P(0, 0, this.SW, MENU_H, { tex: this.menu(), r: 0 })
    this.drawBar(P)
    this.drawNote(P)
    // key cast, beside the bar, so the keys and what they did are one glance
    if (this.keys.a.x > 0.01 && this.keys.label) {
      const c = this.keyCanvas()
      const bg = this.barGeom()
      const k = this.keys.a.x
      // in the close-up on the bar the keys would be cropped off its side, so they sit on top of it,
      // smaller, so the bar stays the subject
      const up = this.keysAbove
      const s = (0.94 + 0.06 * k) * (up ? 0.7 : 1)
      const kx = up ? bg.x + bg.w / 2 - c.lw / 2 : Math.max(EDGE_PAD, bg.x - c.lw - 14)
      const ky = up ? bg.y - c.lh - 12 : bg.y + bg.h / 2 - c.lh / 2
      const kw = c.lw * s
      const kh = c.lh * s
      const ox = kx + (c.lw - kw) / 2
      const oy = up ? bg.y - kh - 10 : ky + (c.lh - kh) / 2
      P(ox, oy, kw, kh, { shadow: 18, col: [0, 0, 0, 0.3], r: 15 * s, a: k, g: 0 })
      this.frost(P, ox, oy, kw, kh, { r: 15 * s, a: k, g: 0 })
      P(ox, oy, kw, kh, { tex: c, r: 15 * s, a: k, g: 0 })
      P(ox, oy, kw, kh, { ring: this.hair, col: [1, 1, 1, 0.16], r: 15 * s, a: k, g: 0 })
    }
    const cu = this.cursor
    if (cu.a.x > 0.01) {
      const s = 1 - cu.press.x * 0.1
      const k = cu.kind || "arrow"
      const [hx, hy] = HOTSPOT[k]
      P(cu.x - hx * s, cu.y - hy * s, 26 * s, 34 * s, { tex: cursorCanvas(k), r: 0, a: cu.a.x, cut: 1 })
    }
  }

  drawNote(P) {
    const n = this.note
    if (!n) return
    const c = noteCanvas(n, this.res)
    const W = NOTE.w
    const H = NOTE.h
    const x = this.SW - W - 12 + n.x.x * (W + 24)
    const y = MENU_H + 8
    const a = clamp(1.6 - n.x.x * 1.4)
    P(x, y, W, H, { shadow: 22, col: [0, 0, 0, 0.22], r: 16, a, g: 0 })
    this.frost(P, x, y, W, H, { r: 16, a, g: 0 })
    P(x, y, W, H, { col: [0.965, 0.965, 0.972, 0.72], r: 16, a, g: 0 })
    if (c) P(x, y, W, H, { tex: c, r: 16, a, g: 0 })
    P(x, y, W, H, { ring: this.hair, col: [0, 0, 0, 0.12], r: 16, a, g: 0 })
  }

  drawBar(P) {
    const b = this.bar
    if (b.shown.x < 0.01 || !b.cells.length) return
    const g = this.barGeom()
    const sc = g.sc
    const a = clamp(b.shown.x * 1.4)
    const R = g.h / 2
    // the HUD's capsule (agent/src/hud/pill.rs): #161618, a hairline #303036 edge, overlay shadows
    P(g.x, g.y, g.w, g.h, { shadow: 30, col: [0, 0, 0, 0.32], r: R, a })
    P(g.x, g.y, g.w, g.h, { shadow: 6, col: [0, 0, 0, 0.3], r: R, a })
    P(g.x, g.y, g.w, g.h, { col: [0.086, 0.086, 0.094, 0.97], r: R, a })
    P(g.x, g.y, g.w, g.h, { ring: this.hair * 1.2, col: [0.188, 0.188, 0.212, 1], r: R, a })
    // the marker: 30 tall, radius 9, white at 0.1 with a 0.2 hairline
    const ox = g.x + sc
    const mh = BAR.markerH * sc
    const my = g.y + sc + ((BAR.inner - BAR.markerH) / 2) * sc
    const mw = Math.max(0, b.right.x - b.left.x) * sc
    P(ox + b.left.x * sc, my, mw, mh, { col: [1, 1, 1, 0.1], r: 9 * sc, a })
    P(ox + b.left.x * sc, my, mw, mh, { ring: this.hair * 1.2, col: [1, 1, 1, 0.2], r: 9 * sc, a })
    const cy = g.y + sc + (BAR.inner / 2) * sc
    for (const c of b.cells) {
      const ca = a * c.o.x
      if (ca < 0.005) continue
      const s = c.s.x
      const gx = ox + c.x.x * sc
      const mid = gx + (c.w.x * sc) / 2
      const X = (lx) => mid + (gx + lx * sc - mid) * s
      const nc = numCanvas(c.n, this.res)
      if (nc) P(X(BAR.num / 2) - (nc.lw / 2) * sc * s, cy - 8 * sc * s, nc.lw * sc * s, 16 * sc * s, { tex: nc, r: 0, a: ca })
      for (const [app, ic] of c.icons) {
        const z = BAR.icon * sc * s * ic.s.x
        const ix = X(ic.x.x + BAR.icon / 2)
        P(ix - z / 2, cy - z / 2, z, z, { tex: iconCanvas(app, 64), r: 0, a: ca * ic.a.x })
      }
    }
  }

  // Positions of everything that moves, for the motion trace.
  sample() {
    const o = { cursor: [+this.cursor.x.toFixed(2), +this.cursor.y.toFixed(2)], bar: [+this.bar.left.x.toFixed(2), +this.bar.right.x.toFixed(2), +this.bar.scale.x.toFixed(4), +this.bar.shown.x.toFixed(3)], wins: {} }
    for (const w of this.wins.values()) {
      const vis = (w.float && !w.pin) || w.ws === this.active || w.flying || w.off.x !== 0
      if (!vis) continue
      o.wins[w.id] = [+(w.x.x + w.off.x).toFixed(2), +w.y.x.toFixed(2), +(w.w.x * w.s.x).toFixed(2), +(w.h.x * w.s.x).toFixed(2), +w.a.x.toFixed(3)]
    }
    return o
  }
}

// Fitts-like: longer trips take longer, but not proportionally.
const cursorTime = (dist) => clamp(0.26 + 0.1 * Math.log2(1 + dist / 24), 0.3, 0.95)

function wifi(ctx, x, y, ink) {
  ctx.strokeStyle = ink
  ctx.lineWidth = 1.7
  ctx.lineCap = "round"
  for (const r of [4.5, 8.5, 12.5]) {
    ctx.beginPath()
    ctx.arc(x, y, r, Math.PI * 1.25, Math.PI * 1.75)
    ctx.stroke()
  }
  ctx.fillStyle = ink
  ctx.beginPath()
  ctx.arc(x, y, 1.6, 0, Math.PI * 2)
  ctx.fill()
}

function battery(ctx, x, y, ink) {
  ctx.strokeStyle = ink
  ctx.globalAlpha = 0.55
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.roundRect(x + 0.5, y - 5.5, 22, 11, 3)
  ctx.stroke()
  ctx.fillStyle = ink
  ctx.fillRect(x + 23.5, y - 2, 1.6, 4)
  ctx.globalAlpha = 1
  ctx.beginPath()
  ctx.roundRect(x + 2.5, y - 3.5, 15, 7, 1.5)
  ctx.fill()
}

// The banner's face: the app's icon, its title, "now", and the message in two lines at most.
const NOTE = { w: 356, h: 70 }
let noteC = null
function noteCanvas(n, res) {
  if (typeof document === "undefined") return null
  const r = Math.min(4, Math.max(2, Math.ceil(res)))
  if (noteC && noteC.key === n.body + r) return noteC
  const c = document.createElement("canvas")
  c.width = NOTE.w * r
  c.height = NOTE.h * r
  const ctx = c.getContext("2d")
  ctx.setTransform(r, 0, 0, r, 0, 0)
  ctx.drawImage(iconCanvas(n.app, 96), 12, (NOTE.h - 34) / 2, 34, 34)
  ctx.textBaseline = "alphabetic"
  ctx.font = `600 13px ${FONT}`
  ctx.fillStyle = "rgba(0,0,0,.86)"
  ctx.fillText(n.title, 56, 26)
  ctx.font = `400 12px ${FONT}`
  ctx.fillStyle = "rgba(0,0,0,.42)"
  ctx.textAlign = "right"
  ctx.fillText("now", NOTE.w - 14, 26)
  ctx.textAlign = "left"
  ctx.font = `400 13px ${FONT}`
  ctx.fillStyle = "rgba(0,0,0,.8)"
  const max = NOTE.w - 56 - 14
  const out = []
  let line = ""
  for (const word of n.body.split(" ")) {
    const t = line ? line + " " + word : word
    if (ctx.measureText(t).width > max && line) {
      out.push(line)
      line = word
    } else line = t
  }
  out.push(line)
  if (out.length > 2) out.splice(2, out.length, out[1].replace(/\s*\S+$/, "") + "…")
  out.forEach((l, i) => ctx.fillText(l, 56, 44 + i * 16))
  c.lw = NOTE.w
  c.key = n.body + r
  c.version = 1
  return (noteC = c)
}

// A workspace's number: 13 pt semibold, tabular figures, #f4f4f6, centred in a 16 pt wide canvas.
const numbers = new Map()
function numCanvas(n, res) {
  if (typeof document === "undefined") return null
  const r = Math.min(4, Math.max(1, Math.ceil(res * 1.5)))
  const key = n + "|" + r
  let c = numbers.get(key)
  if (c) return c
  c = document.createElement("canvas")
  const W = 16
  c.width = W * r
  c.height = 16 * r
  const ctx = c.getContext("2d")
  ctx.setTransform(r, 0, 0, r, 0, 0)
  ctx.font = `600 13px ${FONT}`
  ctx.fontVariantNumeric = "tabular-nums"
  ctx.textAlign = "center"
  ctx.textBaseline = "middle"
  ctx.fillStyle = "#f4f4f6"
  ctx.fillText(String(n % 10), W / 2, 8.5)
  c.lw = W
  c.version = 1
  numbers.set(key, c)
  return c
}

// where each cursor's point is, in its 26 × 34 box
const HOTSPOT = { arrow: [3, 2], hand: [9.5, 3.5], ibeam: [13, 17] }
const cursorTex = {}
function cursorCanvas(kind = "arrow") {
  if (cursorTex[kind]) return cursorTex[kind]
  const c = (cursorTex[kind] = document.createElement("canvas"))
  if (kind !== "arrow") return drawCursor(c, kind)
  c.width = 104
  c.height = 136
  const ctx = c.getContext("2d")
  ctx.scale(4, 4)
  ctx.shadowColor = "rgba(0,0,0,.35)"
  ctx.shadowBlur = 2
  ctx.shadowOffsetY = 0.8
  ctx.beginPath()
  ctx.moveTo(4, 3)
  ctx.lineTo(4, 26)
  ctx.lineTo(9.5, 20.5)
  ctx.lineTo(13.5, 30)
  ctx.lineTo(17, 28.5)
  ctx.lineTo(13, 19.5)
  ctx.lineTo(20.5, 19.5)
  ctx.closePath()
  ctx.fillStyle = "#fff"
  ctx.fill()
  ctx.shadowColor = "transparent"
  ctx.beginPath()
  ctx.moveTo(5.6, 6.6)
  ctx.lineTo(5.6, 22.2)
  ctx.lineTo(9.9, 18)
  ctx.lineTo(13.9, 27.6)
  ctx.lineTo(15.1, 27.1)
  ctx.lineTo(11, 18.1)
  ctx.lineTo(17, 18.1)
  ctx.closePath()
  ctx.fillStyle = "#111"
  ctx.fill()
  c.version = 1
  return c
}

function drawCursor(c, kind) {
  c.width = 104
  c.height = 136
  const ctx = c.getContext("2d")
  ctx.scale(4, 4)
  ctx.lineJoin = ctx.lineCap = "round"
  if (kind === "ibeam") {
    // two strokes: a white halo, then the black beam with its curled serifs
    const beam = () => {
      ctx.beginPath()
      ctx.moveTo(10, 8)
      ctx.quadraticCurveTo(13, 8, 13, 10.5)
      ctx.quadraticCurveTo(13, 8, 16, 8)
      ctx.moveTo(13, 10.5)
      ctx.lineTo(13, 23.5)
      ctx.moveTo(10, 26)
      ctx.quadraticCurveTo(13, 26, 13, 23.5)
      ctx.quadraticCurveTo(13, 26, 16, 26)
      ctx.moveTo(11, 17)
      ctx.lineTo(15, 17)
    }
    beam()
    ctx.strokeStyle = "rgba(255,255,255,.95)"
    ctx.lineWidth = 3.2
    ctx.stroke()
    beam()
    ctx.strokeStyle = "#111"
    ctx.lineWidth = 1.3
    ctx.stroke()
  } else {
    // the pointing hand: index finger up, three knuckles, the thumb at the side
    ctx.beginPath()
    ctx.moveTo(8, 5)
    ctx.arc(9.5, 5, 1.5, Math.PI, 0)
    ctx.lineTo(11, 13)
    ctx.arc(12.5, 13.4, 1.5, Math.PI, 0)
    ctx.arc(15.5, 14.2, 1.5, Math.PI, 0)
    ctx.arc(18.5, 15.2, 1.5, Math.PI, 0)
    ctx.lineTo(20, 22)
    ctx.quadraticCurveTo(20, 28, 15, 29)
    ctx.lineTo(11.5, 29)
    ctx.quadraticCurveTo(9, 29, 7.5, 26)
    ctx.lineTo(4, 19.5)
    ctx.quadraticCurveTo(3.4, 17.6, 5, 17.4)
    ctx.quadraticCurveTo(6.4, 17.4, 8, 20)
    ctx.closePath()
    ctx.shadowColor = "rgba(0,0,0,.35)"
    ctx.shadowBlur = 2
    ctx.shadowOffsetY = 0.8
    ctx.fillStyle = "#fff"
    ctx.fill()
    ctx.shadowColor = "transparent"
    ctx.strokeStyle = "#111"
    ctx.lineWidth = 1.1
    ctx.stroke()
    // the creases between the fingers
    ctx.beginPath()
    for (const x of [14, 17]) {
      ctx.moveTo(x, 14.6)
      ctx.lineTo(x, 18)
    }
    ctx.lineWidth = 0.8
    ctx.stroke()
  }
  c.version = 1
  return c
}

let wallTex = null
// Until the photograph arrives: a gradient in its colours (dusk over hills).
function fallbackWall() {
  if (wallTex) return wallTex
  const c = (wallTex = document.createElement("canvas"))
  c.width = 320
  c.height = 180
  const ctx = c.getContext("2d")
  const g = ctx.createLinearGradient(0, 0, 0, 180)
  g.addColorStop(0, "#5b6a80")
  g.addColorStop(0.55, "#8a8393")
  g.addColorStop(0.75, "#4b4d5a")
  g.addColorStop(1, "#23252d")
  ctx.fillStyle = g
  ctx.fillRect(0, 0, 320, 180)
  c.version = 1
  return c
}

export { BAR_H }
