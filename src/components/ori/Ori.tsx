import { memo, useId } from "react";
import "./ori.css";

/**
 * Ori, the Elchi mascot (from Orbit): a soft eight-point star with a ring around it and two
 * floating mitten hands. In Alloy the ring is steel and its moon is gold. Drawn in SVG on a 400 unit square; every
 * pose moves with CSS and holds still under reduced motion.
 */
export type OriPose =
  | "hi"
  | "thinking"
  | "working"
  | "reading"
  | "searching"
  | "writing"
  | "planning"
  | "drawing"
  | "camera"
  | "needs"
  | "done"
  | "sleeping"
  | "oops"
  | "helpers"
  | "idle";

export const ORI_POSES: OriPose[] = ["idle", "hi", "thinking", "working", "reading", "searching", "writing", "planning", "drawing", "camera", "needs", "done", "sleeping", "oops", "helpers"];

const LABEL: Record<OriPose, string> = {
  hi: "Ori waving hi",
  thinking: "Ori thinking",
  working: "Ori typing on a laptop",
  reading: "Ori reading through a magnifying glass",
  searching: "Ori looking through a telescope",
  writing: "Ori writing",
  planning: "Ori checking a plan",
  drawing: "Ori holding up a note",
  camera: "Ori taking a picture",
  needs: "Ori waving a flag for your attention",
  done: "Ori celebrating",
  sleeping: "Ori asleep",
  oops: "Ori, something went wrong",
  helpers: "Ori with helper stars",
  idle: "Ori",
};

export const BODY =
  "M209.0 99.1 Q213.9 105.7 218.7 112.4 Q227.0 123.6 240.5 119.7 Q249.6 117.1 258.7 114.4 Q283.6 107.1 279.0 132.7 Q277.3 142.0 275.6 151.4 Q273.1 165.1 285.1 172.4 Q291.4 176.3 297.7 180.1 Q319.8 193.7 299.2 209.5 Q293.3 214.0 287.5 218.5 Q276.4 227.0 280.2 240.5 Q282.6 249.2 285.1 258.0 Q292.1 283.0 266.5 278.6 Q257.6 277.0 248.7 275.5 Q234.9 273.1 227.4 285.0 Q223.7 290.9 220.0 296.8 Q206.2 318.8 190.2 298.4 Q185.9 292.9 181.6 287.4 Q173.0 276.4 159.5 280.0 Q151.1 282.3 142.8 284.6 Q117.7 291.4 121.9 265.7 Q123.3 257.2 124.6 248.7 Q126.9 234.9 114.9 227.7 Q108.2 223.7 101.5 219.7 Q79.2 206.3 100.0 190.7 Q106.2 186.0 112.4 181.4 Q123.6 173.0 119.8 159.5 Q117.4 150.8 114.9 142.0 Q107.9 117.0 133.5 121.4 Q142.4 123.0 151.3 124.5 Q165.1 126.9 172.2 114.8 Q176.3 107.7 180.5 100.6 Q193.6 78.2 209.0 99.1Z";

// the ring: a tilted ellipse around the star, its back half drawn behind it
const RING_BACK = "M44.7 273.1 A170 36 -24 0 1 355.3 134.9";
const RING_FRONT = "M355.3 134.9 A170 36 -24 0 1 44.7 273.1";
const MITTEN = "M-4 -20 C7 -22 16 -12 16 0 C16 12 9 21 -1 21 C-11 21 -16 12 -15 1 C-14 -10 -11 -18 -4 -20 Z";

const C = {
  body: "#FFE6AE",
  shade: "#F5C98C",
  ink: "#1A1430",
  ring: "#A9B2BC",
  moon: "#F8C95A",
  sky: "#0A0A16",
  metal: "#8D86B8",
  dark: "#2B2846",
  paper: "#F4F1FF",
  blush: "#FFB199",
};

/** A mitten hand. Fingers point up in its own frame; `left` mirrors it. */
function Hand({ x, y, r = 0, left, cls, edge }: { x: number; y: number; r?: number; left?: boolean; cls?: string; edge?: boolean }) {
  return (
    <g transform={`translate(${x} ${y}) rotate(${r})${left ? " scale(-1 1)" : ""}`}>
      <g className={cls} fill={C.body} stroke={edge ? "#E7B06C" : "none"} strokeWidth={edge ? 3.5 : 0} paintOrder="stroke">
        <path d={MITTEN} />
        <ellipse cx="-17" cy="-10" rx="5.5" ry="9.5" transform="rotate(-28 -17 -10)" />
      </g>
    </g>
  );
}

function MiniStar({ x, y, s = 0.15, cls }: { x: number; y: number; s?: number; cls?: string }) {
  return (
    <g transform={`translate(${x} ${y})`}>
      <g className={cls}>
        <g transform={`scale(${s}) translate(-200 -200)`}>
          <path d={BODY} fill={C.body} />
          <circle cx="170" cy="193" r="13" fill={C.ink} />
          <circle cx="229" cy="187" r="13" fill={C.ink} />
        </g>
      </g>
    </g>
  );
}

type Eyes = "open" | "up" | "down" | "happy" | "closed" | "wide" | "lens" | "wink" | "scope";
type Mouth = "smile" | "open" | "flat" | "o" | "wavy" | "tongue" | "sleep";

const FACE: Record<OriPose, { eyes: Eyes; mouth: Mouth; blush?: boolean; anim: string }> = {
  hi: { eyes: "open", mouth: "open", blush: true, anim: "bob" },
  thinking: { eyes: "up", mouth: "flat", anim: "bob" },
  working: { eyes: "down", mouth: "smile", anim: "bob" },
  reading: { eyes: "lens", mouth: "o", anim: "bob" },
  searching: { eyes: "scope", mouth: "smile", anim: "bob" },
  writing: { eyes: "down", mouth: "tongue", anim: "bob" },
  planning: { eyes: "down", mouth: "smile", anim: "bob" },
  drawing: { eyes: "open", mouth: "open", blush: true, anim: "bob" },
  camera: { eyes: "wink", mouth: "open", anim: "bob" },
  needs: { eyes: "wide", mouth: "o", anim: "hop" },
  done: { eyes: "happy", mouth: "open", blush: true, anim: "hop" },
  sleeping: { eyes: "closed", mouth: "sleep", anim: "breathe" },
  oops: { eyes: "wide", mouth: "wavy", anim: "shake" },
  helpers: { eyes: "open", mouth: "smile", blush: true, anim: "bob" },
  idle: { eyes: "open", mouth: "smile", anim: "bob" },
};

function Face({ eyes, mouth, blush, tiny }: { eyes: Eyes; mouth: Mouth; blush?: boolean; tiny?: boolean }) {
  const ink = C.ink;
  if (tiny)
    return (
      <g fill={ink}>
        {eyes === "happy" || eyes === "closed" ? (
          <g fill="none" stroke={ink} strokeWidth="13" strokeLinecap="round">
            <path d={eyes === "happy" ? "M152 200 Q168 180 184 200" : "M152 194 Q168 210 184 194"} />
            <path d={eyes === "happy" ? "M214 194 Q230 174 246 194" : "M214 188 Q230 204 246 188"} />
          </g>
        ) : (
          <>
            <circle cx="168" cy="194" r="17" />
            <circle cx="230" cy="188" r="17" />
          </>
        )}
      </g>
    );
  const stroke = { fill: "none", stroke: ink, strokeWidth: 5, strokeLinecap: "round" as const };
  return (
    <g>
      {blush && (
        <g fill={C.blush} opacity=".42">
          <ellipse cx="147" cy="212" rx="12" ry="6.5" />
          <ellipse cx="253" cy="203" rx="12" ry="6.5" />
        </g>
      )}
      <g className="ori-blink">
        {eyes === "open" && (
          <g fill={ink}>
            <circle cx="170" cy="193" r="8.5" />
            <circle cx="229" cy="187" r="8.5" />
          </g>
        )}
        {eyes === "up" && (
          <g fill={ink}>
            <circle cx="174" cy="186" r="8.5" />
            <circle cx="233" cy="180" r="8.5" />
          </g>
        )}
        {eyes === "down" && (
          <g fill={ink}>
            <ellipse cx="168" cy="198" rx="8.5" ry="6.5" />
            <ellipse cx="227" cy="192" rx="8.5" ry="6.5" />
          </g>
        )}
        {eyes === "wide" && (
          <g>
            <circle cx="170" cy="192" r="11" fill={ink} />
            <circle cx="229" cy="186" r="11" fill={ink} />
            <circle cx="173.5" cy="188" r="3.6" fill="#fff" />
            <circle cx="232.5" cy="182" r="3.6" fill="#fff" />
          </g>
        )}
        {eyes === "lens" && (
          <g fill={ink}>
            <circle cx="170" cy="193" r="8.5" />
            <circle cx="229" cy="187" r="15" />
            <circle cx="234" cy="181" r="4.5" fill="#fff" />
          </g>
        )}
      </g>
      {eyes === "happy" && (
        <g {...stroke}>
          <path d="M160 197 Q170 183 180 197" />
          <path d="M219 191 Q229 177 239 191" />
        </g>
      )}
      {eyes === "closed" && (
        <g {...stroke}>
          <path d="M160 193 Q170 202 180 193" />
          <path d="M219 187 Q229 196 239 187" />
        </g>
      )}
      {eyes === "wink" && (
        <g>
          <circle cx="170" cy="193" r="8.5" fill={ink} />
          <path d="M219 189 Q229 181 239 189" {...stroke} />
        </g>
      )}
      {eyes === "scope" && (
        <g>
          <path d="M160 194 Q170 186 180 194" {...stroke} />
        </g>
      )}
      {mouth === "smile" && <path d="M190 202 Q201 213 212 199" {...stroke} strokeWidth={4.4} />}
      {mouth === "open" && (
        <g>
          <path d="M188 199 Q201 222 214 196 Z" fill={ink} stroke={ink} strokeWidth="2" strokeLinejoin="round" />
          <path d="M195 210 Q201 216 207 208 Q201 205 195 210 Z" fill="#FF8C7A" />
        </g>
      )}
      {mouth === "flat" && <path d="M193 205 Q201 206 209 202" {...stroke} strokeWidth={4.4} />}
      {mouth === "o" && <ellipse cx="201" cy="206" rx="6" ry="7.5" fill={ink} />}
      {mouth === "wavy" && <path d="M186 207 q7.5 -7 15 0 q7.5 7 15 0" {...stroke} strokeWidth={4.4} />}
      {mouth === "tongue" && (
        <g>
          <path d="M190 202 Q201 213 212 199" {...stroke} strokeWidth={4.4} />
          <ellipse cx="210" cy="207" rx="4.5" ry="5.5" fill="#FF8C7A" transform="rotate(-20 210 207)" />
        </g>
      )}
      {mouth === "sleep" && <ellipse cx="201" cy="205" rx="4.5" ry="3.5" fill={ink} />}
    </g>
  );
}

function OriImpl({ pose = "hi", size = 160, still, className, title }: { pose?: OriPose; size?: number; still?: boolean; className?: string; title?: string }) {
  const raw = useId();
  const id = `ori${raw.replace(/[^a-zA-Z0-9]/g, "")}`;
  const f = FACE[pose] ?? FACE.hi;
  // small sizes keep the star and the face; the ring and hands would only blur
  const compact = size < 34;
  const label = title ?? LABEL[pose];

  const hands = (() => {
    if (compact) return null;
    switch (pose) {
      case "hi":
        return {
          back: <Hand x={70} y={244} r={-160} left />,
          front: (
            <g transform="translate(348 172) rotate(16)">
              <g className="ori-wave">
                <Hand x={0} y={0} />
              </g>
            </g>
          ),
        };
      case "done":
        return { back: null, front: (<g className="ori-cheer"><Hand x={88} y={112} r={-30} left /><Hand x={312} y={106} r={30} /></g>) };
      case "sleeping":
        return { back: <><Hand x={100} y={270} r={-140} left /><Hand x={300} y={266} r={140} /></>, front: null };
      case "oops":
        return { back: <Hand x={84} y={248} r={-160} left />, front: <Hand x={262} y={96} r={-150} edge /> };
      case "thinking":
        return { back: <Hand x={78} y={246} r={-160} left />, front: <Hand x={236} y={232} r={-30} edge /> };
      case "needs":
        return { back: <Hand x={76} y={244} r={-160} left />, front: null };
      case "helpers":
      case "idle":
        return { back: <><Hand x={70} y={244} r={-160} left /><Hand x={330} y={240} r={160} /></>, front: null };
      default:
        // the poses that hold something bring their own two hands with the object
        return null;
    }
  })();

  // things Ori holds; drawn in front of the ring
  const props = (() => {
    if (compact) return null;
    switch (pose) {
      case "working":
        return (
          <g>
            <rect x="140" y="230" width="122" height="80" rx="9" fill={C.dark} stroke="#3B3662" strokeWidth="2.5" />
            <g transform="rotate(-18 201 270)">
              <ellipse cx="201" cy="270" rx="15" ry="5" fill="none" stroke={C.ring} strokeWidth="3" />
            </g>
            <circle cx="201" cy="270" r="5" fill={C.body} />
            <rect x="124" y="306" width="154" height="11" rx="5.5" fill="#3B3662" />
            <Hand x={138} y={300} r={-62} left cls="ori-tap" />
            <Hand x={264} y={300} r={62} cls="ori-tap ori-tap2" />
          </g>
        );
      case "reading":
        return (
          <g className="ori-peer">
            <circle cx="229" cy="187" r="30" fill="rgba(210,225,255,.16)" stroke={C.metal} strokeWidth="7" />
            <path d="M250 209 L284 247" stroke="#6E6896" strokeWidth="10" strokeLinecap="round" />
            <Hand x={290} y={254} r={-140} />
            <Hand x={72} y={246} r={-160} left />
          </g>
        );
      case "searching":
        return (
          <g className="ori-scan">
            <g transform="translate(240 176) rotate(-30)">
              <rect x="-12" y="-9" width="16" height="18" rx="4" fill="#6E6896" />
              <path d="M2 -12 L52 -15 L52 15 L2 12 Z" fill={C.metal} />
              <path d="M50 -18 L112 -23 L112 23 L50 18 Z" fill={C.ring} />
              <rect x="46" y="-19" width="8" height="38" rx="3" fill="#6E6896" />
              <rect x="108" y="-27" width="14" height="54" rx="5" fill="#ECEBF4" />
              <ellipse cx="122" cy="0" rx="5" ry="22" fill="#A6DEFF" />
              <path d="M70 -14 L96 -16" stroke="#fff" strokeOpacity=".45" strokeWidth="4" strokeLinecap="round" />
            </g>
            <Hand x={300} y={170} r={160} />
            <Hand x={264} y={214} r={-150} edge />
          </g>
        );
      case "writing":
        return (
          <g>
            <g transform="rotate(-8 168 272)">
              <rect x="116" y="236" width="104" height="74" rx="6" fill={C.paper} />
              <path d="M128 256 H204 M128 272 H196 M128 288 H178" stroke="#C9C3E6" strokeWidth="4" strokeLinecap="round" />
            </g>
            <g className="ori-scribble">
              <g transform="translate(196 274) rotate(-40)">
                <rect x="0" y="-7" width="78" height="14" rx="3" fill={C.moon} />
                <rect x="8" y="-7" width="5" height="14" fill="#F29C82" />
                <rect x="70" y="-7" width="14" height="14" rx="3" fill="#ECEBF4" />
                <path d="M0 -7 L-18 0 L0 7 Z" fill="#F7D9B5" />
                <path d="M-11 -3 L-18 0 L-11 3 Z" fill={C.ink} />
              </g>
              <Hand x={244} y={240} r={-140} edge />
            </g>
            <Hand x={102} y={268} r={-110} left />
          </g>
        );
      case "planning":
        return (
          <g>
            <rect x="144" y="226" width="114" height="92" rx="9" fill={C.metal} />
            <rect x="153" y="238" width="96" height="72" rx="5" fill={C.paper} />
            <rect x="185" y="219" width="32" height="13" rx="4" fill="#ECEBF4" />
            <g strokeLinecap="round" strokeLinejoin="round" fill="none">
              <path d="M163 255 l5 5 l9 -10" stroke={C.moon} strokeWidth="4" />
              <path d="M163 275 l5 5 l9 -10" stroke={C.moon} strokeWidth="4" />
              <rect className="ori-pulse" x="162" y="289" width="14" height="13" rx="3" stroke="#B4AEDA" strokeWidth="3.5" />
              <path d="M185 256 H236 M185 276 H228 M185 296 H232" stroke="#C9C3E6" strokeWidth="4" />
            </g>
            <Hand x={144} y={288} r={-75} left />
            <Hand x={258} y={288} r={75} />
          </g>
        );
      case "drawing":
        return (
          <g className="ori-present">
            <g transform="translate(318 128) rotate(9)">
              <rect x="-30" y="-30" width="60" height="60" rx="4" fill="#FFC9B8" />
              <path d="M-18 -12 H16 M-18 2 H10 M-18 16 H4" stroke="#E79A84" strokeWidth="4" strokeLinecap="round" />
            </g>
            <Hand x={328} y={176} r={170} />
            <Hand x={72} y={246} r={-160} left />
          </g>
        );
      case "camera":
        return (
          <g>
            <g className="ori-flash">
              <path d="M330 92 l8 -18 M352 102 l18 -8 M346 84 l14 -14" stroke={C.body} strokeWidth="5" strokeLinecap="round" />
            </g>
            <g transform="rotate(-10 306 138)">
              <rect x="266" y="112" width="82" height="56" rx="11" fill={C.dark} stroke="#3B3662" strokeWidth="2.5" />
              <rect x="280" y="104" width="24" height="12" rx="4" fill={C.dark} />
              <circle cx="310" cy="140" r="17" fill={C.metal} />
              <circle cx="310" cy="140" r="9" fill="#ECEBF4" />
              <circle cx="335" cy="122" r="4" fill={C.moon} />
            </g>
            <Hand x={268} y={176} r={-120} left />
            <Hand x={350} y={170} r={130} />
          </g>
        );
      case "needs":
        return (
          <g>
            <path d="M336 186 L336 64" stroke="#ECEBF4" strokeWidth="5" strokeLinecap="round" />
            <path className="ori-flag" d="M338 64 L392 82 L338 100 Z" fill={C.moon} />
            <Hand x={334} y={180} r={10} />
          </g>
        );
      case "oops":
        return <path d="M136 128 Q148 146 142 156 Q135 163 128 155 Q124 146 136 128 Z" fill="#A6DEFF" />;
      default:
        return null;
    }
  })();

  const extras = (() => {
    if (compact) return null;
    switch (pose) {
      case "thinking":
        return (
          <g fill="#ECEBF4">
            <circle className="ori-dot" cx="286" cy="112" r="6" />
            <circle className="ori-dot ori-d2" cx="306" cy="88" r="8.5" />
            <circle className="ori-dot ori-d3" cx="332" cy="60" r="12" />
          </g>
        );
      case "done":
        return (
          <g>
            {[
              [64, 64, 1.1, C.moon, ""],
              [342, 56, 0.9, "#ECEBF4", "ori-s2"],
              [366, 236, 0.75, C.moon, "ori-s3"],
              [36, 220, 0.75, "#ECEBF4", "ori-s4"],
            ].map(([x, y, s, c, k]) => (
              <g key={`${x}`} transform={`translate(${x} ${y}) scale(${s})`}>
                <path className={`ori-spark ${k}`} d="M0 -16 L4 -4 L16 0 L4 4 L0 16 L-4 4 L-16 0 L-4 -4 Z" fill={c as string} />
              </g>
            ))}
          </g>
        );
      case "sleeping":
        return (
          <g fill="#ECEBF4" fontFamily="Unbounded, Figtree, sans-serif" fontWeight="600">
            <text className="ori-z" x="262" y="122" fontSize="26">z</text>
            <text className="ori-z ori-z2" x="262" y="122" fontSize="34">z</text>
            <text className="ori-z ori-z3" x="262" y="122" fontSize="42">z</text>
          </g>
        );
      case "hi":
      case "drawing":
        return (
          <g fill="#ECEBF4">
            <circle className="ori-twinkle" cx="62" cy="96" r="4" />
            <circle className="ori-twinkle ori-s3" cx="352" cy="300" r="3.5" fill={C.body} />
            <circle className="ori-twinkle ori-s2" cx="96" cy="336" r="3" />
          </g>
        );
      default:
        return null;
    }
  })();

  const helpers = pose === "helpers" && !compact;

  return (
    <svg
      className={`ori ${still ? "ori-still" : ""} ${className ?? ""}`}
      viewBox="0 0 400 400"
      width={size}
      height={size}
      role="img"
      aria-label={label}
    >
      <title>{label}</title>
      <defs>
        <mask id={`${id}-shade`}>
          <rect width="400" height="400" fill="#fff" />
          <path d={BODY} fill="#000" transform="translate(-13 -15)" />
        </mask>
        <radialGradient id={`${id}-glow`}>
          <stop offset="0" stopColor={C.body} stopOpacity=".28" />
          <stop offset="1" stopColor={C.body} stopOpacity="0" />
        </radialGradient>
      </defs>
      <circle className="ori-aura" cx="200" cy="200" r={compact ? 0 : 150} fill={`url(#${id}-glow)`} />
      {extras}
      <g className={`ori-${f.anim}`}>
        {!compact && (
          <>
            <path d={RING_BACK} fill="none" stroke={C.ring} strokeWidth="5.5" strokeLinecap="round" />
            {!helpers && <circle cx="314.6" cy="129.1" r="7" fill={C.body} />}
            {helpers && <MiniStar x={314.6} y={124} cls="ori-mini" />}
          </>
        )}
        {hands?.back}
        <path d={BODY} fill={C.body} />
        <path d={BODY} fill={C.shade} mask={`url(#${id}-shade)`} />
        <Face eyes={compact && (f.eyes === "lens" || f.eyes === "scope") ? "open" : f.eyes} mouth={f.mouth} blush={f.blush && !compact} tiny={compact} />
        {!compact && (
          <>
            <path d={RING_FRONT} fill="none" stroke={C.ring} strokeWidth="5.5" strokeLinecap="round" />
            {helpers ? (
              <>
                <MiniStar x={88.8} y={276} s={0.13} cls="ori-mini ori-m2" />
                <MiniStar x={268.6} y={208} s={0.17} cls="ori-mini ori-m3" />
              </>
            ) : (
              <>
                <circle cx="88.8" cy="278.4" r="7.5" fill={C.ring} />
                <circle cx="268.6" cy="210.3" r="12" fill={C.moon} stroke={C.sky} strokeWidth="4" />
              </>
            )}
          </>
        )}
        {props}
        {hands?.front}
      </g>
    </svg>
  );
}

export const Ori = memo(OriImpl);
