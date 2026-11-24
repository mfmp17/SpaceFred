// The show engine. The page's first image is a still of the study (a Cycles render). start() runs
// inside the click: it brings up WebGPU (or Canvas 2D), fetches the tape, the effects and the opening
// plate, and plays the film. In WebGPU the opening is the plate (the room rendered in Cycles, as video)
// with the live desktop set on the monitor's panel; tight on the screen the desktop is drawn flat,
// framed exactly as the panel was, so the two cut together. Canvas 2D and reduced motion push into
// the still instead. The picture is cut to the tape's word times
// (tape.js), so it runs the same with or without sound. ?capture exposes window.__show for
// deterministic frame stepping and a motion trace.
import { World } from "./world.js"
import { build, pile, HOLD, END } from "./director.js"
import { createCanvas2D } from "./canvas2d.js"
import { createGPU, POST_DEFAULT } from "./gpu.js"
import { Audio } from "./audio.js"
import { LINES, TAPE } from "./tape.js"
import { plates, loadPlate, coverUV } from "./plates.js"
import { PHOTOS } from "./library.js"
import { FEEDS } from "./feeds.js"
import { loadIcons } from "./apps.js"
import { clamp, mix, smooth } from "./springs.js"

// the black between the last cut and the end card, in seconds (Show.tsx's CARD_MS)
export const CARD_AT = 0.75

// pictures inside his apps (renders of our own scenes), and the still Canvas 2D pushes into
const PLATES = ["wall", "wall-s", "across"]
const STILLS = { CamChair: "intro" }

export function createShow({ still, canvas, base, capture = false, reduced = false, on = {} }) {
  const emit = (k, v) => on[k]?.(v)
  let cssW = 0
  let cssH = 0
  let SW = 1440
  let SH = 900
  let world = null
  let renderer = null
  let phase = "still"
  let t = 0
  let cues = []
  let spans = []
  let ci = 0
  let raf = 0
  let last = 0
  let visible = true
  let dprScale = 1
  let caption = null
  let manifest = null
  let tapeAt = null // { ctx, t } when the tape started: the film follows the tape's clock
  let paused = false
  const cam = { D: 1, cx: 0, cy: 0 }
  const grade = [0, 0, 0]
  const fx = { ...POST_DEFAULT }
  const scene = {}
  const look = { focus: null, dof: 0.6 }
  const audio = new Audio(base + "audio/", { log: capture })
  const stats = { intervals: [], cpu: [], frames: 0 }
  const trace = []

  function measure() {
    const r = (canvas || still).getBoundingClientRect()
    cssW = Math.max(1, r.width)
    cssH = Math.max(1, r.height)
  }

  function logical() {
    measure()
    SW = clamp(Math.round(cssW * 1.25), 600, 1440)
    SH = Math.round(clamp((SW * cssH) / cssW, 0.5 * SW, 2.2 * SW))
    cam.cx = SW / 2
    cam.cy = SH / 2
  }

  function makeWorld() {
    const w = new World(SW, SH, { reduced })
    w.clock = "Tue Jun 2  11:48 PM"
    w.frontApp = "Jot"
    for (const def of pile(SW, SH)) w.add(def)
    w.setFocus("notes")
    w.cursor.x = SW * 0.7
    w.cursor.y = SH * 0.62
    return w
  }

  const dpr = () => Math.min(window.devicePixelRatio || 1, 2) * dprScale
  function fit() {
    const k = Math.min(cssW / SW, cssH / SH)
    return { k, ox: (cssW - SW * k) / 2, oy: (cssH - SH * k) / 2 }
  }

  function sizeCanvas(c) {
    let d = dpr()
    const px = cssW * cssH * d * d
    if (px > 3.6e6) d *= Math.sqrt(3.6e6 / px)
    const w = Math.round(cssW * d)
    const h = Math.round(cssH * d)
    if (c.width !== w || c.height !== h) {
      c.width = w
      c.height = h
    }
    return d
  }

  // Window canvases are painted at the device pixels they cover, so text stays sharp in a push.
  function setRes(w, k, d) {
    const r = Math.round(clamp(k * d, 0.5, 5) * 4) / 4
    if (w.res !== r) {
      w.res = r
      w.menuCanvas = null
      w.keys.canvas = null
    }
  }

  // ── Projection ─────────────────────────────────────────────────────────
  // The camera looks at part of his screen: D is the fraction of the screen in view.
  function camT(F) {
    const s = 1 / cam.D
    return { k: s * F.k, ox: (SW / 2 - cam.cx * s) * F.k + F.ox, oy: (SH / 2 - cam.cy * s) * F.k + F.oy }
  }

  function focusRect(T) {
    const id = look.focus
    const w = id && world.wins.get(id)
    if (!w) return null
    return [w.x.x * T.k + T.ox, w.y.x * T.k + T.oy, (w.x.x + w.w.x) * T.k + T.ox, (w.y.x + w.h.x) * T.k + T.oy]
  }

  // Reduced motion: held framings with cuts between them instead of continuous moves.
  const ease = (u) => {
    u = clamp(u)
    return u < 0.5 ? 4 * u * u * u : 1 - (-2 * u + 2) ** 3 / 2
  }

  // ── The room ───────────────────────────────────────────────────────────
  // three.a → three.b at three.u: the opening's shot, from the chair's camera onto "screen" (the panel
  // seen exactly as the flat desktop shows it). WebGPU plays it as the Cycles plate; Canvas 2D and
  // reduced motion push into the still.
  const three = { on: false, a: "CamChair", b: "CamChair", u: 0, glow: 1, drift: 0.1 }

  // the panel's texture: the desktop fitted into it, the wallpaper past its edges
  function panel() {
    const th = Math.round(clamp(canvas.height, 720, 1600))
    const tw = Math.round(th * 1.6)
    const g = Math.min(tw / SW, th / SH)
    return { tw, th, G: { k: g, ox: (tw - SW * g) / 2, oy: (th - SH * g) / 2 } }
  }

  // ── The opening plate ──────────────────────────────────────────────────
  // In WebGPU the opening (the asleep room, the screen waking, the move in) is a Cycles render played
  // as video, fetched on the click; the live desktop sits on the panel's quad in each frame, so the
  // hand-off to the flat desktop is exact. Until the video has a frame, the page's still stands in for
  // it (it is frame 0). If the video fails, the film pushes into the still instead.
  const plate = { src: null, meta: null, video: null, frames: null, still: null, frame: -1, ready: false, failed: false, loading: null, done: false }
  // capture with ?plateFrames: the plate's frames as images (capture.ts serves them), because a seek in
  // the video can land a frame or two off at the end of a group of pictures
  const plateFrames = capture && typeof location !== "undefined" && new URLSearchParams(location.search).has("plateFrames")
  function loadOpening() {
    if (plate.loading) return plate.loading
    plate.loading = (async () => {
      try {
        await renderer.preparePlate()
        if (!(await loadOpeningMeta())) throw new Error("no plate")
        if (plateFrames) {
          const c = document.createElement("canvas")
          c.width = plate.meta.size[0]
          c.height = plate.meta.size[1]
          c.version = 0
          plate.src = "frames"
          plate.frames = c
          plate.ready = true
          return true
        }
        const v = document.createElement("video")
        v.muted = true
        v.playsInline = true
        v.preload = "auto"
        // 10-bit where the browser decodes it: the room is dark gradients, and 8 bits band there
        const can = (t) => v.canPlayType(t) !== ""
        plate.src = (can('video/mp4; codecs="hvc1.2.4.L150.90"') ? "opening-hevc.mp4" : can('video/webm; codecs="vp09.02.50.10"') ? "opening.webm" : "opening.mp4")
        v.src = base + "plate/" + plate.src
        await new Promise((res, rej) => {
          v.addEventListener("loadeddata", res, { once: true })
          v.addEventListener("error", rej, { once: true })
        })
        v.width = v.videoWidth
        v.height = v.videoHeight
        v.version = 0
        if (v.requestVideoFrameCallback && !capture) {
          const onFrame = (_, md) => {
            plate.frame = Math.round(md.mediaTime * plate.meta.fps)
            v.version++
            if (!plate.done) v.requestVideoFrameCallback(onFrame)
          }
          v.requestVideoFrameCallback(onFrame)
        }
        plate.video = v
        plate.ready = true
      } catch {
        plate.failed = true
        plate.video = null
        void stillsIn()
      }
      return plate.ready
    })()
    return plate.loading
  }
  // a replay or a seek back into the opening brings the video back
  function resetPlate() {
    if (!plate.done) return
    Object.assign(plate, { done: false, ready: false, video: null, frames: null, loading: null, frame: -1, failed: false })
    if (renderer?.renderPlate && !reduced) void loadOpening()
  }
  function loadOpeningStill() {
    if (plate.still || !renderer?.renderPlate) return
    const img = new Image()
    img.decoding = "async"
    img.src = base + "renders/intro.jpg"
    img.decode().then(() => {
      img.version = 1
      plate.still = img
    }, () => {})
  }
  const plateOn = () => three.on && !reduced && !plate.failed && !plate.done && renderer?.renderPlate && plate.meta && (plate.ready || plate.still) && t < plate.meta.end
  // after the hand-off the video is let go
  function plateEnd() {
    if (plate.done || !plate.meta || t < plate.meta.end) return
    plate.done = true
    plate.frames = null
    if (plate.video) {
      plate.video.pause()
      plate.video.removeAttribute("src")
      plate.video.load()
      plate.video = null
    }
  }
  // the video follows the film's clock; a frame callback says which frame is up
  function syncPlate() {
    const v = plate.video
    if (!v || capture) return
    if (!plateOn() || phase !== "live" || !visible || paused) {
      if (!v.paused) v.pause()
      // paused on the opening (or a seek into it while paused): show the frame for t
      if (paused && plateOn() && Math.abs(v.currentTime - t) > 0.05 && !v.seeking) {
        v.addEventListener("seeked", () => (plate.frame = Math.round(v.currentTime * plate.meta.fps), v.version++, render()), { once: true })
        v.currentTime = Math.min(t, v.duration || t)
      }
      return
    }
    if (Math.abs(v.currentTime - t) > 0.1) v.currentTime = Math.min(t, v.duration || t)
    if (v.paused) v.play().catch(() => {})
    if (!v.requestVideoFrameCallback) {
      const f = Math.round(v.currentTime * plate.meta.fps)
      if (f !== plate.frame) {
        plate.frame = f
        v.version++
      }
    }
  }
  // capture: the frame for t, decoded, before the picture is drawn
  async function seekPlate() {
    const v = plate.video
    const c = plate.frames
    if ((!v && !c) || !plateOn()) return
    const f = Math.min(plate.meta.frames - 1, Math.round(t * plate.meta.fps))
    if (f === plate.frame) return
    if (c) {
      const r = await fetch("/__plate_frames/" + String(f).padStart(4, "0") + ".png")
      if (!r.ok) throw new Error("plate frame " + f + ": " + r.status)
      const bm = await createImageBitmap(await r.blob())
      c.getContext("2d").drawImage(bm, 0, 0, c.width, c.height)
      bm.close()
      plate.frame = f
      c.version++
      return
    }
    await new Promise((res) => {
      v.addEventListener("seeked", res, { once: true })
      v.currentTime = (f + 0.5) / plate.meta.fps
    })
    plate.frame = f
    v.version++
  }
  function renderPlate() {
    const m = plate.meta
    const live = plate.ready && plate.frame >= 0
    const src = live ? plate.frames || plate.video : plate.still
    if (!src) return false
    const f = live ? clamp(plate.frame, 0, m.frames - 1) : 0
    const c = m.corners[f] // [bl, br, tr, tl], 0..1 from the frame's top-left
    const glow = m.glow[f]
    const iw = m.size[0]
    const ih = m.size[1]
    // covering the screen, centred on the panel as far as the frame allows (as the page's still is)
    const ka = Math.max(cssW / iw, cssH / ih)
    const pcx = ((c[0][0] + c[1][0] + c[2][0] + c[3][0]) / 4) * iw
    const pcy = ((c[0][1] + c[1][1] + c[2][1] + c[3][1]) / 4) * ih
    const ax = clamp(cssW / 2 - pcx * ka, cssW - iw * ka, 0)
    const ay = clamp(cssH / 2 - pcy * ka, cssH - ih * ka, 0)
    // landing: the panel exactly where the flat desktop's panel lies at the hand-off
    const { tw, th, G } = panel()
    const T = camT(fit())
    const P = [T.ox - (G.ox / G.k) * T.k, T.oy - (G.oy / G.k) * T.k, (tw / G.k) * T.k, (th / G.k) * T.k]
    const e = m.corners[m.frames - 1]
    const ex0 = (e[0][0] + e[3][0]) / 2
    const ex1 = (e[1][0] + e[2][0]) / 2
    const ey0 = (e[2][1] + e[3][1]) / 2
    const ey1 = (e[0][1] + e[1][1]) / 2
    const bw = P[2] / (ex1 - ex0)
    const bh = P[3] / (ey1 - ey0)
    const w = smooth(clamp((t - m.land) / (m.end - m.land)))
    const rw = iw * ka * Math.pow(bw / (iw * ka), w)
    const rh = ih * ka * Math.pow(bh / (ih * ka), w)
    const rx = mix(ax, P[0] - ex0 * bw, w)
    const ry = mix(ay, P[1] - ey0 * bh, w)
    const quad = [c[3], c[2], c[1], c[0]].map(([u, v]) => [rx + u * rw, ry + v * rh])
    const list = []
    const wall = world.wall
    list.push({ x: 0, y: 0, w: tw, h: th, tex: wall, uv: coverUV(wall, tw, th), g: 1 })
    list.push({ x: 0, y: 0, w: tw, h: th, col: [0, 0, 0, 0.4], g: 0 })
    setRes(world, G.k, 1)
    world.draw(list, G, { g: 1 })
    // a panel in a room reads a little dimmer than the desktop seen flat, and the plate's own panel (its
    // glass, the room's sheen on it) shows through a little; landing, both go
    const gain = glow * (0.9 + 0.1 * w)
    const alpha = clamp(glow * 2.5) * (0.92 + 0.08 * w)
    // the room's own image keeps its shadows: a lighter vignette and no extra contrast while it's on
    // screen, reaching the flat desktop's grade as the panel lands
    const pfx = { ...fx, vignette: (fx.vignette ?? 0) * mix(0.55, 1, w), contrast: mix(1, fx.contrast ?? 1, w), saturation: mix(1, fx.saturation ?? 1, w) }
    return renderer.renderPlate(list, tw, th, cssW, cssH, grade, pfx, { video: src, rect: [rx, ry, rw, rh], quad, alpha, gain, screen: 1 - smooth(clamp((w - 0.5) / 0.5)) })
  }

  // ── Getting ready ──────────────────────────────────────────────────────
  let rendererIn = null
  let platesIn = null
  let prepping = null
  const stillsIn = () => Promise.all([...Object.values(STILLS).map((p) => loadPlate(base, p)), loadOpeningMeta()])
  function loadOpeningMeta() {
    plate.metaIn ||= fetch(base + "plate/opening.json")
      .then((r) => r.json())
      .then((m) => (plate.meta = m))
      .catch(() => null)
    return plate.metaIn
  }
  function prepare() {
    if (prepping) return prepping
    const wantGPU = !new URLSearchParams(location.search).has("canvas2d")
    rendererIn = (wantGPU ? createGPU(canvas).catch(() => null) : Promise.resolve(null)).then((g) => (renderer = g || createCanvas2D(canvas)))
    const manifestIn = fetch(base + "audio/manifest.json")
      .then((x) => x.json())
      .then((m) => (manifest = m))
      .catch(() => null)
    platesIn = Promise.all([...PLATES.map((p) => loadPlate(base, p)), ...PHOTOS.map((p) => loadPlate(base, p, "photos/")), ...FEEDS.map((p) => loadPlate(base, p, "feeds/"))])
    prepping = Promise.all([
      manifestIn.then((m) => m && audio.load(m)),
      loadPlate(base, "wall"),
      loadIcons(base).then(() => {
        if (world) for (const w of world.wins.values()) w.dirty = true
      }),
      // WebGPU fetches only the plate's corners and the still now, and the video on the click; Canvas 2D gets the still
      rendererIn.then((r) => (r.renderPlate ? loadOpeningMeta().then(loadOpeningStill) : stillsIn())),
    ]).then(() => true)
    return prepping
  }

  // ── Canvas 2D, reduced motion, or no plate: the still ──
  // A move onto the screen pushes into the still's monitor; a move off it cuts to the still and
  // drifts in; a move between two cameras cuts to the second.
  function drawStillShot(list) {
    const toScreen = three.b === "screen"
    const name = toScreen ? three.a : three.b
    const img = plates[STILLS[name]]
    if (!img) {
      list.push({ x: 0, y: 0, w: cssW, h: cssH, col: [0.02, 0.02, 0.025, 1], g: 0 })
      return
    }
    const cover = Math.max(cssW / img.width, cssH / img.height)
    let k = cover
    let cx = img.width / 2
    let cy = img.height / 2
    // the panel in the still: the plate's frame 0 is the still
    const corners = plate.meta?.corners[0].map(([u, v]) => [u * img.width, v * img.height])
    const near = toScreen ? corners : null
    if (near && near.every(Boolean)) {
      // push in until the monitor fills the frame
      const pts = near
      const x0 = Math.min(...pts.map((p) => p[0]))
      const x1 = Math.max(...pts.map((p) => p[0]))
      const y0 = Math.min(...pts.map((p) => p[1]))
      const y1 = Math.max(...pts.map((p) => p[1]))
      const k1 = Math.min(cssW / (x1 - x0), cssH / (y1 - y0))
      const e = three.u
      k = cover * Math.exp(Math.log(k1 / cover) * e)
      cx = mix(cx, (x0 + x1) / 2, e)
      cy = mix(cy, (y0 + y1) / 2, e)
    } else k = cover * (1 + 0.05 * three.u)
    list.push({ x: cssW / 2 - cx * k, y: cssH / 2 - cy * k, w: img.width * k, h: img.height * k, tex: img, g: 0 })
    // the stills were rendered with a blank panel: the desktop goes on it, as it is now
    const pts = corners
    if (pts && pts.every((p) => p && isFinite(p[0]) && isFinite(p[1]))) {
      const X = pts.map((p) => cssW / 2 + (p[0] - cx) * k)
      const Y = pts.map((p) => cssH / 2 + (p[1] - cy) * k)
      const clip = [Math.min(...X), Math.min(...Y), Math.max(...X), Math.max(...Y)]
      const bw = clip[2] - clip[0]
      const bh = clip[3] - clip[1]
      if (bw > 2 && bw < cssW * 6) {
        // a panel in a room is a little dimmer than the desktop seen flat; arriving on it, that goes, so the cut to flat is seamless
        const panelDim = toScreen ? 1 - smooth(clamp((three.u - 0.55) / 0.45)) : 1
        const kk = Math.max(bw / SW, bh / SH)
        setRes(world, kk, dpr())
        world.draw(list, { k: kk, ox: (clip[0] + clip[2]) / 2 - (SW * kk) / 2, oy: (clip[1] + clip[3]) / 2 - (SH * kk) / 2 }, { clip, g: 1 })
        list.push({ x: clip[0], y: clip[1], w: bw, h: bh, col: [0, 0, 0, 0.12 * panelDim + 0.88 * (1 - clamp(three.glow))], g: 0, clip })
      }
    }
  }

  function buildList() {
    const list = []
    const F = fit()
    fx.focus = null
    if (three.on) {
      drawStillShot(list)
      return list
    }
    const T = camT(F)
    setRes(world, T.k, dpr())
    world.draw(list, T, { g: 1 })
    fx.focus = focusRect(T)
    if (F.ox > 0.5 || F.oy > 0.5) letterbox(list, F)
    return list
  }

  function postFx() {
    // depth of field grows with the push, so a full-screen view is sharp everywhere
    const depth = three.on ? 0.4 : clamp((1 - cam.D) / 0.45)
    return { ...fx, dof: fx.focus ? look.dof * depth : 0 }
  }

  function letterbox(list, F) {
    const col = [0.03, 0.03, 0.035, 1]
    if (F.ox > 0.5) {
      list.push({ x: 0, y: 0, w: F.ox, h: cssH, col })
      list.push({ x: cssW - F.ox, y: 0, w: F.ox, h: cssH, col })
    }
    if (F.oy > 0.5) {
      list.push({ x: 0, y: 0, w: cssW, h: F.oy, col })
      list.push({ x: 0, y: cssH - F.oy, w: cssW, h: F.oy, col })
    }
  }

  // ── Clock ──────────────────────────────────────────────────────────────
  const S = {
    get world() {
      return world
    },
    get SW() {
      return SW
    },
    get SH() {
      return SH
    },
    get t() {
      return t
    },
    reduced,
    cam,
    grade,
    fx,
    scene,
    look,
    three,
    // a move between two cameras of the room, at u (already eased)
    shot(a, b, u) {
      three.on = true
      three.a = a
      three.b = b
      three.u = u
    },
    flat() {
      three.on = false
    },
    sfx(name, gain = 1) {
      audio.sfx(name, { t, gain })
    },
    bed(name, v, tc) {
      audio.bed("sfx-" + name + ".mp3", v, t, tc)
    },
    hint(v) {
      emit("hint", v)
    },
    end() {
      phase = "ended"
      setCaption(null)
      // a hard cut: picture and sound stop on the same frame, nothing runs on under the card
      audio.stopAll()
      tapeAt = null
      // then 0.75 s of black and dead silence, and the card arrives with a children's "Yaaay!" (kind
      // "card": the export mixes it after the cut that silences everything else)
      const yay = audio.manifest?.sfx?.yay
      if (yay) audio.play(yay, { t: t + CARD_AT, delay: CARD_AT, kind: "card" })
      emit("phase", "ended")
    },
  }

  // ── The tape ───────────────────────────────────────────────────────────
  function startTape() {
    const tape = audio.tape
    if (!tape || phase !== "live" || t >= tape.dur - 0.05) return
    const h = audio.play(tape.file, { t, offset: t, kind: "voice" })
    tapeAt = h ? { ctx: audio.ctx.currentTime, t } : null
    startScore()
  }
  // The strings: one file from score.at in show time. Before then it waits on the context's clock, so
  // a pause or a hidden tab holds it in step with the tape; it never runs past the last cut.
  function startScore() {
    const sc = audio.manifest?.score
    if (!sc || t >= sc.end - 0.05 || (audio.on && !audio.has(sc.file))) return
    for (const h of audio.live) if (h.file === sc.file) return
    const wait = sc.at - t
    audio.play(sc.file, { t: Math.max(t, sc.at), offset: Math.max(0, -wait), delay: Math.max(0, wait), kind: "score" })
  }
  audio.onReady = (file) => {
    if (file === audio.tape?.file && phase === "live" && !tapeAt) {
      startTape()
      audio.restoreBeds(t)
    } else if (file === audio.manifest?.score?.file && phase === "live" && tapeAt) startScore()
  }

  // one sentence at a time, held a moment after it ends unless the next one starts
  // (the aligner runs a line's end through a pause after it; the pause is where it really ends)
  const lineEnd = LINES.map(([, a, b]) => Math.min(b, ...TAPE.marks.gaps.filter((g) => g[0] > a && g[0] < b).map((g) => g[0])))
  let li = 0
  function captions() {
    while (li < LINES.length && t > lineEnd[li] + 0.6) li++
    const L = LINES[li]
    const on = L && t >= L[1] - 0.05 && (li + 1 >= LINES.length || t < LINES[li + 1][1] - 0.05)
    const high = !!(on && S.capHigh && L[1] >= S.capHigh[0] && L[1] <= S.capHigh[1])
    setCaption(on ? L[0] : null, high)
  }
  function setCaption(text, high = false) {
    if (text === caption) return
    caption = text
    emit("caption", text ? { text, high } : null)
  }

  function setup() {
    world = makeWorld()
    t = 0
    ci = 0
    li = 0
    grade.fill(0)
    Object.assign(fx, POST_DEFAULT)
    Object.assign(three, { on: false, a: "CamChair", b: "CamChair", u: 0, glow: 1, drift: 0.1 })
    look.focus = null
    cam.D = 1
    cam.cx = SW / 2
    cam.cy = SH / 2
    trace.length = 0
    const b = build(S)
    cues = b.cues
    spans = b.spans
  }

  function advance(dt) {
    if (phase !== "live") return
    const t1 = t + dt
    // substeps keep springs and cues in lockstep at any frame rate
    const n = Math.max(1, Math.ceil(dt / (1 / 60)))
    const h = dt / n
    for (let i = 0; i < n; i++) {
      t += h
      while (ci < cues.length && cues[ci].t <= t) cues[ci++].fn()
      for (const s of spans) if (t >= s.t0 && t - h < s.t1) s.fn(clamp((t - s.t0) / (s.t1 - s.t0)), t)
      world.update(h)
      if (phase !== "live") break
    }
    t = Math.max(t, Math.min(t1, END))
    if (phase === "live") captions()
  }

  function render() {
    if (!renderer) return
    const c0 = performance.now()
    const d = sizeCanvas(canvas)
    syncPlate()
    if (phase === "ended" || !plateOn() || !renderPlate()) renderer.render(phase === "ended" ? [] : buildList(), cssW, cssH, d, grade, postFx())
    renderer.sweep()
    stats.cpu.push(performance.now() - c0)
    if (stats.cpu.length > 600) stats.cpu.shift()
  }

  // While the tape plays, its clock leads: a long frame never lets the picture fall behind it.
  function tapeTime() {
    if (!tapeAt || !audio.on) return null
    return tapeAt.t + (audio.ctx.currentTime - tapeAt.ctx)
  }

  function frame(now) {
    raf = 0
    let dt = last ? Math.min(0.05, (now - last) / 1000) : 1 / 60
    if (last) {
      stats.intervals.push(now - last)
      if (stats.intervals.length > 600) stats.intervals.shift()
    }
    last = now
    const tt = tapeTime()
    if (tt != null) dt = clamp(tt - t, 0, 0.12)
    advance(dt)
    plateEnd()
    render()
    adapt()
    schedule()
  }

  // If frames run long for a while, drop the pixel ratio a step.
  let slow = 0
  function adapt() {
    const n = stats.intervals.length
    if (n < 2) return
    const iv = stats.intervals[n - 1]
    slow = iv > 24 ? slow + 1 : Math.max(0, slow - 1)
    if (slow > 40 && dprScale > 0.55) {
      dprScale -= 0.15
      slow = 0
    }
  }

  function schedule() {
    if (raf || capture || !visible || phase !== "live" || paused) return
    raf = requestAnimationFrame(frame)
  }

  function setVisible(v) {
    if (v === visible) return
    visible = v
    audio.setHidden(!v)
    if (!v && plate.video && !plate.video.paused) plate.video.pause()
    last = 0
    if (v) schedule()
    else if (raf) {
      cancelAnimationFrame(raf)
      raf = 0
    }
  }
  const onVis = () => setVisible(!document.hidden && inView)
  let inView = true
  document.addEventListener("visibilitychange", onVis)
  const io = new IntersectionObserver((es) => {
    inView = es[0].isIntersecting
    onVis()
  })
  io.observe(canvas)
  const ro = new ResizeObserver(() => {
    measure()
    if (phase === "still") drawStill()
    else if (phase !== "live") render()
  })
  ro.observe(canvas)

  // ── The still ──────────────────────────────────────────────────────────
  let stillR = null
  function drawStill() {
    if (!still || phase !== "still") return
    measure()
    stillR ||= createCanvas2D(still)
    const d = sizeCanvas(still)
    setRes(world, fit().k, d)
    const list = []
    world.draw(list, fit(), { g: 1 })
    letterbox(list, fit())
    stillR.render(list, cssW, cssH, d, [0, 0, 0])
  }

  logical()
  setup()
  // Everything but the sound's start comes down once the page is idle, so a click starts the film at
  // once: the renderer, the room (or Canvas 2D's stills), the tape and the effects. Save-Data waits
  // for the click.
  if (!capture && !navigator.connection?.saveData) setTimeout(() => void prepare(), 1200)

  const api = {
    get phase() {
      return phase
    },
    get renderer() {
      return renderer?.kind ?? null
    },
    get time() {
      return t
    },
    prepare,
    async start({ context = null, muted = false } = {}) {
      if (phase !== "still" && phase !== "ended") return
      if (phase === "ended") {
        audio.stopAll()
        setup()
        resetPlate()
      }
      paused = false
      audio.setPaused(false)
      phase = "loading"
      emit("phase", "loading")
      audio.attach(context, muted)
      // usually everything is already here (prepare() ran while the page was idle); if not, the still
      // stays up a moment, then the film starts with whatever has arrived and the rest joins in
      const ready = prepare()
      await rendererIn
      await Promise.race([ready, new Promise((res) => setTimeout(res, capture ? 60000 : 1200))])
      const opening = renderer.renderPlate && !reduced ? loadOpening() : null
      if (!opening) void stillsIn()
      if (capture) await Promise.all([ready, platesIn, opening])
      for (const w of world.wins.values()) w.dirty = true
      // the cues at 0 (the room's first shot) before the first frame, so it isn't the flat desktop
      while (ci < cues.length && cues[ci].t <= 0) cues[ci++].fn()
      if (capture) await seekPlate()
      phase = "live"
      emit("phase", "live")
      last = 0
      startTape()
      render()
      schedule()
    },
    setMuted(muted, context) {
      audio.setMuted(muted, context)
      if (!muted && phase === "live") {
        audio.stopAll()
        tapeAt = null
        startTape()
        audio.restoreBeds(t)
      }
    },
    skip() {
      if (phase === "live" || phase === "loading") S.end()
    },
    get duration() {
      return END
    },
    get paused() {
      return paused
    },
    setPaused(p) {
      if (phase !== "live" || p === paused) return
      paused = p
      audio.setPaused(p)
      // a seek while paused left the tape stopped: it joins again here
      if (!p && !tapeAt) {
        startTape()
        audio.restoreBeds(t)
      }
      if (p && plate.video && !plate.video.paused) plate.video.pause()
      last = 0
      emit("paused", p)
      schedule()
    },
    // The world is deterministic in show time: a seek back rebuilds it and replays the cues up to the
    // new time without sound or pictures, then the tape and the beds join at that point.
    seek(to) {
      if (!world || phase === "still" || phase === "loading") return
      to = clamp(to, 0, END - 0.1)
      audio.stopAll()
      tapeAt = null
      if (phase === "ended") {
        phase = "live"
        emit("phase", "live")
      }
      if (to < t) {
        setup()
        setCaption(null)
        if (plate.meta && to < plate.meta.end) resetPlate()
      }
      audio.quiet = true
      while (t < to - 1e-6 && phase === "live") advance(Math.min(0.25, to - t))
      while (ci < cues.length && cues[ci].t <= t) cues[ci++].fn()
      audio.quiet = false
      for (const w of world.wins.values()) w.dirty = true
      plateEnd()
      last = 0
      if (!paused) {
        startTape()
        audio.restoreBeds(t)
      }
      render()
      schedule()
    },
    // Control+n during the visitor's turn.
    key(n) {
      if (phase !== "live" || t < HOLD[0] || t > HOLD[1] - 0.5 || n < 1 || n > 3) return false
      world.switchTo(n)
      S.sfx("key", 0.9)
      return true
    },
    // A tap on the bar during the visitor's turn: which workspace was it?
    tap(x, y) {
      if (phase !== "live" || !world) return false
      const T = camT(fit())
      const lx = (x - T.ox) / T.k
      const ly = (y - T.oy) / T.k
      const g = world.barGeom()
      if (ly < g.y - 20 || ly > g.y + g.h + 20) return false
      for (let n = 1; n <= world.bar.cells.length; n++) {
        const r = world.cellRect(n)
        if (lx >= r.x - 6 && lx <= r.x + r.w + 6) return api.key(n)
      }
      return false
    },
    destroy() {
      document.removeEventListener("visibilitychange", onVis)
      io.disconnect()
      ro.disconnect()
      if (raf) cancelAnimationFrame(raf)
      audio.close()
      renderer?.destroy()
    },
  }

  function sample() {
    return { t: +t.toFixed(4), cue: ci, cam: [+cam.cx.toFixed(2), +cam.cy.toFixed(2), +cam.D.toFixed(5)], three: three.on ? [three.a, three.b, +three.u.toFixed(4)] : null, active: world.active, ...world.sample() }
  }

  if (capture) {
    window.__show = {
      api,
      // Advance the clock by n steps of dt and render once; every step lands in the trace.
      async step(dt = 1 / 30, n = 1) {
        for (let i = 0; i < n; i++) {
          advance(dt)
          trace.push(sample())
        }
        plateEnd()
        await seekPlate()
        render()
        await renderer?.done()
      },
      // Jump forward without rendering (cues and springs still run).
      async seek(to) {
        while (t < to - 1e-6 && phase === "live") advance(Math.min(1 / 30, to - t))
        plateEnd()
        await seekPlate()
        render()
        await renderer?.done()
      },
      // Real-time playback for perf: run rAF for ms and report frame stats.
      perf(ms) {
        return new Promise((res) => {
          stats.intervals.length = 0
          stats.cpu.length = 0
          last = 0
          const t0 = performance.now()
          const loop = (now) => {
            const dt = last ? Math.min(0.05, (now - last) / 1000) : 1 / 60
            if (last) stats.intervals.push(now - last)
            last = now
            advance(dt)
            render()
            if (now - t0 < ms && phase === "live") requestAnimationFrame(loop)
            else res(summary())
          }
          requestAnimationFrame(loop)
        })
      },
      state: () => ({ t: +t.toFixed(3), phase, active: world.active, D: +cam.D.toFixed(3), three: { ...three }, renderer: renderer?.kind, dprScale, plates: Object.keys(plates), plate: plate.failed ? "failed" : plate.src ?? null }),
      trace: (from = 0) => trace.slice(from),
      audioLog: () => audio.log,
      stats: summary,
      tape: TAPE,
    }
  }

  function summary() {
    const q = (arr, p) => {
      if (!arr.length) return 0
      const s = [...arr].sort((a, b) => a - b)
      return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(2)
    }
    return { n: stats.intervals.length, interval: { p50: q(stats.intervals, 0.5), p95: q(stats.intervals, 0.95), p99: q(stats.intervals, 0.99), max: q(stats.intervals, 1) }, cpu: { p50: q(stats.cpu, 0.5), p95: q(stats.cpu, 0.95), max: q(stats.cpu, 1) }, over20: stats.intervals.filter((x) => x > 20).length, dprScale }
  }

  void mix
  return api
}
