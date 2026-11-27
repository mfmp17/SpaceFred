import { motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { Download, downloadNote, HeaderDownload } from "./Download";
import signature from "./footer-signature.svg";
import { usePlatform } from "./platform";
import { CHECKSUM_URL, INSTALL, RELEASE_VERSION } from "./release";
import { Show } from "./Show";

export function App() {
  return (
    <main className="bg-[#0a0b0d] text-[#f3f4f6] antialiased">
      <Show header={<Header />} hero={(start) => <Hero start={start} />} />
      <Product />
      <Footer />
    </main>
  );
}

function Header() {
  return (
    <header className="relative isolate flex h-16 items-center justify-between px-5 sm:h-[72px] sm:px-7">
      {/* keeps the links legible over the lit window in the still */}
      <div aria-hidden="true" className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-[170%] bg-[linear-gradient(to_bottom,rgb(8_8_10/0.72),rgb(8_8_10/0.4)_45%,rgb(8_8_10/0.12)_75%,transparent)]" />
      <a href="./" aria-label="Spaceman" className="flex items-center gap-3 rounded-md focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white/60">
        <img src="icon-192.png" alt="" width={28} height={28} className="size-7 rounded-[23%] shadow-[0_2px_8px_rgb(0_0_0/0.45)] ring-1 ring-white/15" />
        <span className="text-[15px] font-semibold tracking-[-0.015em] text-[#f5f5f7] [text-shadow:0_1px_8px_rgb(0_0_0/0.5)]">Spaceman</span>
      </a>
      <nav aria-label="Site" className="flex items-center gap-6 text-[13px] text-[#dfe1e5] [text-shadow:0_1px_8px_rgb(0_0_0/0.6)]">
        <a href="#features" className="hidden transition-colors hover:text-white sm:inline">Features</a>
        <a href="#shortcuts" className="hidden transition-colors hover:text-white sm:inline">Shortcuts</a>
        <a
          href="https://github.com/anomalyco/spaceman"
          aria-label="Spaceman on GitHub"
          className="inline-flex items-center gap-1.5 transition-colors hover:text-white"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M12 .5a12 12 0 0 0-3.79 23.39c.6.11.82-.26.82-.58v-2.03c-3.34.73-4.04-1.61-4.04-1.61a3.18 3.18 0 0 0-1.33-1.76c-1.09-.74.08-.73.08-.73a2.52 2.52 0 0 1 1.84 1.24 2.56 2.56 0 0 0 3.5 1 2.56 2.56 0 0 1 .76-1.6c-2.67-.3-5.47-1.33-5.47-5.93a4.64 4.64 0 0 1 1.24-3.22 4.3 4.3 0 0 1 .12-3.18s1.01-.32 3.3 1.23a11.38 11.38 0 0 1 6 0c2.29-1.55 3.3-1.23 3.3-1.23a4.3 4.3 0 0 1 .12 3.18 4.64 4.64 0 0 1 1.24 3.22c0 4.61-2.81 5.63-5.48 5.92a2.86 2.86 0 0 1 .82 2.22v3.29c0 .32.21.7.82.58A12 12 0 0 0 12 .5Z" />
          </svg>
          <span className="hidden sm:inline">GitHub</span>
        </a>
        <HeaderDownload />
      </nav>
    </header>
  );
}

// over the bottom of the still: the line, then the download and the film (the still itself plays too)
function Hero({ start }: { start: () => void }) {
  return (
    <div className="flex flex-col gap-7 px-5 pb-9 sm:px-7 sm:pb-12">
      <div className="flex max-w-[36rem] flex-col gap-3">
        <h1 className="text-[clamp(44px,6.4vw,76px)] leading-[0.96] font-semibold tracking-[-0.04em] text-balance">For your windows.</h1>
        <p className="max-w-[30rem] text-[17px] leading-[1.5] text-pretty text-[#c4c6cc]">
          Spaceman is a tiling window manager for macOS. Every window side by side, nothing on top of anything else.
        </p>
      </div>
      <div className="flex flex-col gap-3">
        <div className="pointer-events-auto flex flex-wrap items-center gap-3">
          <Download note={false} />
          <button
            type="button"
            onClick={start}
            className="inline-flex h-11 items-center gap-2.5 rounded-full bg-white/[0.1] pr-5 pl-4 text-[15px] font-medium text-[#f3f4f6] shadow-[inset_0_0_0_1px_rgba(255,255,255,.12)] backdrop-blur-md backdrop-saturate-150 transition-[background-color,transform] duration-150 hover:bg-white/[0.16] active:scale-[0.98] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-white/70"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true"><path d="M2.5 1.4v9.2c0 .5.5.8.9.5l7.2-4.6a.6.6 0 0 0 0-1L3.4.9c-.4-.3-.9 0-.9.5Z" /></svg>
            Watch the demo
            <span className="text-[#9a9ca3] tabular-nums">2 min</span>
          </button>
        </div>
        <HeroNote />
      </div>
    </div>
  );
}

function HeroNote() {
  const p = usePlatform();
  return <p className="h-5 text-[13px] leading-5 text-[#9a9ca3] tabular-nums">{downloadNote(p)}</p>;
}

function Product() {
  return (
    <div className="mx-auto flex max-w-[1080px] flex-col gap-28 px-5 py-24 sm:px-10 sm:py-32">
      <section id="features" aria-labelledby="features-title" className="flex scroll-mt-8 flex-col gap-16">
        <div className="flex max-w-[40rem] flex-col gap-4">
          <h2 id="features-title" className="text-[clamp(28px,3.6vw,40px)] leading-[1.1] font-semibold tracking-[-0.03em] text-balance">Every window has a place.</h2>
          <p className="text-[17px] leading-[1.55] text-pretty text-[#9a9ca3]">
            Windows open side by side and make room for each other. Ten numbered workspaces sit under your fingers, and rules send each app where it belongs.
          </p>
        </div>
        <div className="grid gap-x-10 gap-y-16 md:grid-cols-2">
          <Feature title="Windows tile as they open" demo={<TilingDemo />}>
            Each new window takes a share of the screen and the others make room. Drag a window onto another to swap them, or drag an edge to resize the split.
          </Feature>
          <Feature title="Ten workspaces on the keyboard" demo={<WorkspacesDemo />}>
            <Kbd>⌥1</Kbd> to <Kbd>⌥0</Kbd> switch workspaces. Add <Kbd>⇧</Kbd> to send the focused window to one and stay where you are; if that workspace is already showing on another display, the window lands there and focus follows it. A small bar shows each workspace's number and apps as you go.
          </Feature>
          <Feature title="Rules for where apps go" demo={<RulesStill />}>
            Send an app to a workspace when it opens, or leave a window alone by its title. Rules are checked in order and the first match wins.
          </Feature>
          <Feature title="Tabs keep their tile" demo={<TabsDemo />}>
            Switch native tabs, in Ghostty for one, and the window keeps its tile, its size and its place in the layout.
          </Feature>
          <Feature title="Your windows come back" demo={<RestoreDemo />}>
            Pausing, quitting or updating Spaceman brings every hidden window back where it was. It remembers where each one goes before it moves it.
          </Feature>
          <Feature title="Accessibility, and nothing else" demo={<AccessStill />}>
            System Integrity Protection stays on. Window titles stay in memory: Spaceman never saves, logs or sends them, and nothing reports home.
          </Feature>
        </div>
      </section>

      <section id="shortcuts" aria-labelledby="shortcuts-title" className="flex scroll-mt-8 flex-col gap-10">
        <div className="flex max-w-[40rem] flex-col gap-4">
          <h2 id="shortcuts-title" className="text-[clamp(28px,3.6vw,40px)] leading-[1.1] font-semibold tracking-[-0.03em]">On the keyboard.</h2>
          <p className="text-[17px] leading-[1.55] text-pretty text-[#9a9ca3]">Everything starts with Option. Shortcuts follow your keyboard layout; the numbers are always the top row.</p>
        </div>
        <dl className="grid gap-x-10 md:grid-cols-2">
          {SHORTCUTS.map(([action, keys]) => (
            <div key={action} className="flex min-h-12 items-center justify-between gap-6 border-b border-white/[0.07] py-2.5">
              <dt className="text-[15px] text-[#c4c6cc]">{action}</dt>
              <dd className="flex shrink-0 items-center gap-1.5">{keys}</dd>
            </div>
          ))}
        </dl>
      </section>

      <section id="download" aria-labelledby="download-title" className="flex scroll-mt-8 flex-col gap-8 rounded-[28px] bg-white/[0.03] p-6 shadow-[inset_0_0_0_1px_rgba(255,255,255,.06)] sm:p-10">
        <div className="flex flex-col gap-8 md:flex-row md:items-end md:justify-between">
          <div className="flex max-w-[30rem] flex-col gap-4">
            <img src="icon-192.png" alt="" width={64} height={64} className="size-16" />
            <h2 id="download-title" className="text-[clamp(28px,3.6vw,40px)] leading-[1.1] font-semibold tracking-[-0.03em]">Get Spaceman.</h2>
            <p className="text-[15px] leading-[1.6] text-pretty text-[#9a9ca3]">
              Free, signed and notarized. It's a preview, so turn off any other window manager first; the setup tour asks for Accessibility access.
            </p>
          </div>
          <Download />
        </div>
        <div className="flex flex-col gap-3">
          <Install />
          <p className="text-[13px] leading-5 text-[#6b6e76]">
            Later updates arrive inside the app. Check the download against its{" "}
            <a href={CHECKSUM_URL} className="text-[#9a9ca3] underline decoration-white/20 underline-offset-4 transition-colors hover:text-[#f3f4f6]">SHA-256</a>.
          </p>
        </div>
      </section>
    </div>
  );
}

function Footer() {
  return (
    <footer className="mx-auto flex max-w-[1080px] flex-col gap-10 border-t border-white/[0.07] px-5 pt-8 pb-16 text-[13px] text-[#6b6e76] sm:px-10">
      <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
        <p className="flex items-center gap-2.5">
          <img src="icon-192.png" alt="" width={20} height={20} className="size-5 rounded-[23%] ring-1 ring-white/10" />
          <span className="text-[#c4c6cc]">Spaceman {RELEASE_VERSION}</span>
          <span aria-hidden="true">·</span>
          <span>
            Built with{" "}
            <a href="https://opencode.ai" className="text-[#9a9ca3] underline decoration-white/20 underline-offset-2 transition-colors hover:text-[#f3f4f6]">
              OpenCode
            </a>
          </span>
        </p>
        <nav aria-label="Documentation" className="flex flex-wrap gap-x-6 gap-y-2">
          <a href="https://github.com/anomalyco/spaceman" className="transition-colors hover:text-[#f3f4f6]">GitHub</a>
          <a href="llms-full.txt" className="transition-colors hover:text-[#f3f4f6]">Documentation</a>
          <a href="skill.md" className="transition-colors hover:text-[#f3f4f6]">Setup skill for agents</a>
          <a href="llms.txt" className="transition-colors hover:text-[#f3f4f6]">llms.txt</a>
        </nav>
      </div>
      <div className="flex justify-center py-6">
        <div className="relative text-[13px] leading-6 text-[#909090]">
          <img
            src={signature}
            alt=""
            aria-hidden="true"
            width={163}
            height={64}
            className="pointer-events-none absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2"
          />
          <p className="relative m-0">
            ©2026{" "}
            <a href="https://anoma.ly" className="text-inherit underline decoration-white/20 underline-offset-2 transition-colors hover:text-[#f2f2ef]">
              Anomaly
            </a>
            . All rights reserved.
          </p>
        </div>
      </div>
    </footer>
  );
}

function Install() {
  const [copied, setCopied] = useState(false);
  return (
    <div className="overflow-hidden rounded-2xl bg-black/40 shadow-[inset_0_0_0_1px_rgba(255,255,255,.08)]">
      <div className="flex h-11 items-center justify-between border-b border-white/[0.06] pr-2 pl-5">
        <span className="text-[13px] font-medium text-[#c4c6cc]">Or install it from Terminal</span>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard?.writeText(INSTALL.replace(/\\\n\s*/g, "")).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            });
          }}
          className="inline-flex h-7 w-[72px] items-center justify-center rounded-full bg-white/[0.08] text-[12px] font-medium text-[#c4c6cc] transition-colors hover:bg-white/[0.14] hover:text-white focus-visible:outline-2 focus-visible:outline-white/60"
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="overflow-x-auto px-5 py-4 font-mono text-[13px] leading-[1.7] text-[#d4d6db]">
        <code>{INSTALL}</code>
      </pre>
    </div>
  );
}

function Feature({ title, demo, children }: { title: string; demo: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-5">
      <div aria-hidden="true" inert className="aspect-[16/10] overflow-hidden rounded-2xl bg-[#141518] shadow-[inset_0_0_0_1px_rgba(255,255,255,.06),0_20px_40px_-24px_rgba(0,0,0,.8)]">
        {demo}
      </div>
      <div className="flex flex-col gap-2 pr-4">
        <h3 className="text-[17px] font-semibold tracking-[-0.01em]">{title}</h3>
        <p className="text-[15px] leading-[1.6] text-pretty text-[#9a9ca3]">{children}</p>
      </div>
    </div>
  );
}

function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-6 min-w-6 items-center justify-center rounded-[6px] bg-white/[0.07] px-1.5 font-sans text-[13px] leading-none text-[#e4e6ea] shadow-[inset_0_0_0_1px_rgba(255,255,255,.1),0_1px_0_rgba(0,0,0,.4)]">
      {children}
    </kbd>
  );
}

const SHORTCUTS: [string, React.ReactNode][] = [
  ["Switch workspace", <><Kbd>⌥1</Kbd><span className="text-[#6b6e76]">…</span><Kbd>⌥0</Kbd></>],
  ["Send the window to a workspace", <><Kbd>⌥⇧1</Kbd><span className="text-[#6b6e76]">…</span><Kbd>⌥⇧0</Kbd></>],
  ["Back to the last workspace", <Kbd>⌥Tab</Kbd>],
  ["Next or previous window", <><Kbd>⌥`</Kbd><Kbd>⌥⇧`</Kbd></>],
  ["Focus left, down, up, right", <><Kbd>⌥H</Kbd><Kbd>J</Kbd><Kbd>K</Kbd><Kbd>L</Kbd></>],
  ["Move the tile", <><Kbd>⌥⇧H</Kbd><Kbd>J</Kbd><Kbd>K</Kbd><Kbd>L</Kbd></>],
  ["Flip the split", <Kbd>⌥/</Kbd>],
  ["Shrink or grow the tile", <><Kbd>⌥-</Kbd><Kbd>⌥=</Kbd></>],
  ["Fullscreen Layout", <Kbd>⌥⇧F</Kbd>],
  ["Next or previous workspace", <span className="text-[13px] text-[#9a9ca3]">Three-finger swipe</span>],
];

// ── Demos ───────────────────────────────────────────────────────────────

type R = { x: number; y: number; w: number; h: number };
const APP_COLORS = ["#e9b93f", "#4f8ad6", "#4fb3a7", "#8a74d1", "#d9803a", "#5cb86e"];

function tiles(n: number): R[] {
  if (n === 1) return [{ x: 0, y: 0, w: 1, h: 1 }];
  if (n === 2) return [{ x: 0, y: 0, w: 0.5, h: 1 }, { x: 0.5, y: 0, w: 0.5, h: 1 }];
  if (n === 3) return [{ x: 0, y: 0, w: 0.5, h: 1 }, { x: 0.5, y: 0, w: 0.5, h: 0.5 }, { x: 0.5, y: 0.5, w: 0.5, h: 0.5 }];
  return [{ x: 0, y: 0, w: 0.5, h: 0.5 }, { x: 0.5, y: 0, w: 0.5, h: 0.5 }, { x: 0, y: 0.5, w: 0.5, h: 0.5 }, { x: 0.5, y: 0.5, w: 0.5, h: 0.5 }];
}

// Steps through a cycle while the demo is on screen.
function useCycle(length: number, ms: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [i, setI] = useState(0);
  useEffect(() => {
    let timer = 0;
    const io = new IntersectionObserver(([e]) => {
      window.clearInterval(timer);
      if (e.isIntersecting) timer = window.setInterval(() => setI((x) => (x + 1) % length), ms);
    });
    if (ref.current) io.observe(ref.current);
    return () => {
      io.disconnect();
      window.clearInterval(timer);
    };
  }, [length, ms]);
  return [ref, i] as const;
}

function MiniWindow({ r, color, pad = 0.012 }: { r: R; color: string; pad?: number }) {
  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.94 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.94 }}
      transition={{ type: "spring", duration: 0.42, bounce: 0 }}
      className="absolute overflow-hidden rounded-[6px] bg-[#e9ebef] shadow-[0_6px_18px_rgba(0,0,0,.35)]"
      style={{ left: `${(r.x + pad) * 100}%`, top: `${(r.y + pad * 1.6) * 100}%`, width: `${(r.w - pad * 2) * 100}%`, height: `${(r.h - pad * 3.2) * 100}%` }}
    >
      <div className="flex h-[14%] max-h-4 min-h-2.5 items-center gap-[3px] bg-[#dcdfe4] px-1.5">
        {[0, 1, 2].map((k) => <span key={k} className="size-[5px] rounded-full bg-[#c2c6cd]" />)}
      </div>
      <div className="flex flex-col gap-[6%] p-[6%]">
        <span className="h-1.5 w-1/2 rounded-full" style={{ background: color }} />
        <span className="h-1 w-4/5 rounded-full bg-[#d0d3d9]" />
        <span className="h-1 w-3/5 rounded-full bg-[#d0d3d9]" />
      </div>
    </motion.div>
  );
}

function TilingDemo() {
  const [ref, i] = useCycle(5, 1300);
  const n = i === 0 ? 1 : i;
  const rs = tiles(Math.min(n, 4));
  return (
    <div ref={ref} className="relative size-full bg-[linear-gradient(135deg,#465363,#262c35)] p-[3%]">
      <div className="relative size-full">
        {rs.map((r, k) => <MiniWindow key={k} r={r} color={APP_COLORS[k]} />)}
      </div>
    </div>
  );
}

// apps by colour, per workspace
const SPACES = [[0, 1, 2], [3, 4], [5]];

// the Icons bar Spaceman shows on a switch: each number with its apps, a marker on the current one
function WorkspacesDemo() {
  const [ref, i] = useCycle(3, 2000);
  return (
    <div ref={ref} className="relative size-full bg-[linear-gradient(135deg,#465363,#262c35)] p-[3%]">
      <div className="relative h-[80%] w-full">
        {tiles(SPACES[i].length).map((r, k) => <MiniWindow key={`${i}-${k}`} r={r} color={APP_COLORS[SPACES[i][k]]} />)}
      </div>
      <div className="absolute inset-x-0 bottom-[5%] flex justify-center">
        <div className="flex items-center gap-1 rounded-[13px] bg-[#1c1d20]/90 p-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,.09),0_10px_28px_rgba(0,0,0,.45)]">
          {SPACES.map((apps, k) => (
            <div key={k} className="relative flex h-7 items-center gap-[5px] px-2.5">
              {k === i && <motion.span layoutId="ws-marker" transition={{ type: "spring", duration: 0.34, bounce: 0 }} className="absolute inset-0 rounded-[9px] bg-white/[0.13] shadow-[inset_0_0_0_1px_rgba(255,255,255,.1)]" />}
              <span className={`relative pr-0.5 text-[12px] font-semibold tabular-nums transition-opacity duration-300 ${k === i ? "text-white" : "text-white/45"}`}>{k + 1}</span>
              {apps.map((c) => (
                <span key={c} className={`relative size-3.5 rounded-[4px] transition-opacity duration-300 ${k === i ? "" : "opacity-45"}`} style={{ background: APP_COLORS[c] }} />
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

// a tabbed window keeps its tile while its tabs change under it
function TabsDemo() {
  const [ref, i] = useCycle(3, 1500);
  const r = { x: 0, y: 0, w: 0.5, h: 1 };
  return (
    <div ref={ref} className="relative size-full bg-[linear-gradient(135deg,#465363,#262c35)] p-[3%]">
      <div className="relative size-full">
        <div className="absolute flex flex-col overflow-hidden rounded-[6px] bg-[#16171a] shadow-[0_6px_18px_rgba(0,0,0,.35)]" style={{ left: "1.2%", top: "1.9%", width: "47.6%", height: "96.2%" }}>
          <div className="flex h-[14%] max-h-6 min-h-4 items-end gap-[2px] bg-[#0f1012] px-1.5">
            {[0, 1, 2].map((k) => (
              <span key={k} className={`relative flex h-[80%] flex-1 items-center gap-1 rounded-t-[4px] px-1.5 transition-colors duration-200 ${k === i ? "bg-[#16171a]" : ""}`}>
                <span className="size-[5px] shrink-0 rounded-full" style={{ background: APP_COLORS[[2, 0, 5][k]], opacity: k === i ? 1 : 0.5 }} />
                <span className={`h-1 flex-1 rounded-full ${k === i ? "bg-white/40" : "bg-white/15"}`} />
              </span>
            ))}
          </div>
          <div className="flex flex-col gap-[7%] p-[7%] font-mono">
            {[0.72, 0.5, 0.86, 0.4, 0.64].map((w, k) => (
              <motion.span key={`${i}-${k}`} initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.18, delay: k * 0.03 }} className="h-1 rounded-full" style={{ width: `${(w * (1 - ((i + k) % 3) * 0.12)) * 100}%`, background: k === 0 ? APP_COLORS[[2, 0, 5][i]] : "rgba(255,255,255,.18)" }} />
            ))}
          </div>
        </div>
        <MiniWindow r={{ ...r, x: 0.5, h: 0.5 }} color={APP_COLORS[1]} />
        <MiniWindow r={{ ...r, x: 0.5, y: 0.5, h: 0.5 }} color={APP_COLORS[3]} />
      </div>
    </div>
  );
}

const RULES = [
  { app: "Chat", color: 1, title: "Standup", action: "Ignore", ws: "" },
  { app: "Chat", color: 1, title: "Any", action: "Route", ws: "2" },
  { app: "Mail", color: 2, title: "Any", action: "Route", ws: "2" },
  { app: "Browser", color: 3, title: "Any", action: "Route", ws: "3" },
];

function RulesStill() {
  return (
    <div className="flex size-full flex-col justify-center gap-1.5 bg-[#eceef1] px-[6%] text-[#1f2126]">
      <div className="grid grid-cols-[1.1fr_1.2fr_1fr_0.8fr] px-2 pb-1 text-[11px] font-semibold text-[#6b6e76]">
        <span>App</span><span>Window title</span><span>Action</span><span>Workspace</span>
      </div>
      {RULES.map((r, k) => (
        <div key={k} className="grid grid-cols-[1.1fr_1.2fr_1fr_0.8fr] items-center rounded-md bg-white px-2 py-1.5 text-[13px] shadow-[0_1px_0_rgba(0,0,0,.05)]">
          <span className="flex items-center gap-1.5"><span className="size-3 rounded-[3px]" style={{ background: APP_COLORS[r.color] }} />{r.app}</span>
          <span className={r.title === "Any" ? "text-[#9a9ca3]" : ""}>{r.title === "Any" ? "Any" : `“${r.title}”`}</span>
          <span>{r.action}</span>
          <span>{r.ws || "—"}</span>
        </div>
      ))}
    </div>
  );
}

// Pause Tiling: the parked windows come back to the frames they had before Spaceman moved them
const PLACED: R[] = [{ x: 0.06, y: 0.18, w: 0.42, h: 0.52 }, { x: 0.36, y: 0.3, w: 0.44, h: 0.5 }, { x: 0.55, y: 0.14, w: 0.38, h: 0.44 }];

function RestoreDemo() {
  const [ref, i] = useCycle(2, 2200);
  const on = i === 0;
  const tiled = tiles(2);
  return (
    <div ref={ref} className="relative size-full bg-[linear-gradient(135deg,#465363,#262c35)]">
      <div className="absolute inset-x-0 top-0 z-10 flex h-[9%] items-center justify-end gap-2 bg-[#eceef1]/85 px-[3%] text-[11px] text-[#24262b]">
        <span className={`rounded-[4px] px-1.5 py-px text-[10px] font-bold transition-colors duration-300 ${on ? "bg-[#24262b] text-[#eceef1]" : "bg-[#24262b]/25 text-[#24262b]"}`}>1</span>
        <span className="w-[84px] text-right">{on ? "Pause Tiling" : "Resume Tiling"}</span>
      </div>
      <div className="absolute inset-x-[3%] top-[12%] bottom-[3%]">
        {PLACED.map((p, k) => {
          const r = on ? tiled[k] : p;
          return k === 2 && on ? null : <MiniWindow key={k} r={r} color={APP_COLORS[k + 2]} pad={on ? 0.012 : 0} />;
        })}
      </div>
    </div>
  );
}

// the one permission Spaceman asks for, as System Settings shows it
function AccessStill() {
  const rows = [
    { name: "Spaceman", on: true, icon: <img src="icon-192.png" alt="" className="size-full" /> },
    { name: "Terminal", on: false, icon: <span className="block size-full rounded-[5px] bg-[#2a2c31]" /> },
    { name: "Script Editor", on: false, icon: <span className="block size-full rounded-[5px] bg-[#c9ccd2]" /> },
  ];
  return (
    <div className="flex size-full flex-col justify-center gap-3 bg-[#eceef1] px-[8%] text-[#1f2126]">
      <div className="flex items-center gap-2 text-[13px] font-semibold">
        <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><rect width="20" height="20" rx="5" fill="#3478f6" /><circle cx="10" cy="5.6" r="1.6" fill="#fff" /><path d="M5.2 8.2 10 9l4.8-.8M10 9v3.4m0 0-2.2 4m2.2-4 2.2 4" stroke="#fff" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" fill="none" /></svg>
        Accessibility
      </div>
      <div className="overflow-hidden rounded-[10px] bg-white shadow-[0_0_0_1px_rgba(0,0,0,.06)]">
        {rows.map((r, k) => (
          <div key={r.name} className={`flex items-center gap-2.5 px-3 py-2 text-[13px] ${k ? "border-t border-black/[0.06]" : ""} ${r.on ? "" : "text-[#1f2126]/45"}`}>
            <span className={`size-5 shrink-0 ${r.on ? "" : "opacity-50"}`}>{r.icon}</span>
            <span className="flex-1">{r.name}</span>
            <span className={`relative h-[18px] w-8 rounded-full ${r.on ? "bg-[#3478f6]" : "bg-black/[0.12]"}`}>
              <span className={`absolute top-[2px] size-[14px] rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,.25)] ${r.on ? "left-[16px]" : "left-[2px]"}`} />
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
