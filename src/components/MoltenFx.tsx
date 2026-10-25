// The Play button's forge moment: molten metal (WebGL) on the button face and sparks
// (2D canvas) around it. Mounted only while the button is hot, so it costs nothing otherwise.

import { useEffect, useRef, useState } from "react";

const VERT = `attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}`;

// Domain-warped noise mapped to a molten palette: crust -> red -> orange -> gold -> white-hot.
const FRAG = `precision mediump float;
uniform vec2 r;uniform float t;uniform float heat;
float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float n(vec2 p){vec2 i=floor(p),f=fract(p);vec2 u=f*f*(3.-2.*f);
 return mix(mix(h(i),h(i+vec2(1,0)),u.x),mix(h(i+vec2(0,1)),h(i+vec2(1,1)),u.x),u.y);}
float fbm(vec2 p){float v=0.,a=.5;for(int i=0;i<5;i++){v+=a*n(p);p=p*2.03+vec2(1.7,9.2);a*=.5;}return v;}
void main(){
 vec2 uv=gl_FragCoord.xy/r;
 vec2 p=uv*vec2(r.x/r.y,1.)*2.2;
 vec2 q=vec2(fbm(p+vec2(0.,-t*.45)),fbm(p+vec2(5.2,1.3)+vec2(t*.2,-t*.3)));
 float f=fbm(p+2.2*q+vec2(t*.15,-t*.5));
 vec3 crust=vec3(.22,.04,.02),red=vec3(.85,.16,.02),orange=vec3(1.,.48,.06),gold=vec3(1.,.78,.3),white=vec3(1.,.96,.82);
 vec3 c=mix(crust,red,smoothstep(.18,.42,f));
 c=mix(c,orange,smoothstep(.38,.58,f));
 c=mix(c,gold,smoothstep(.54,.72,f));
 c=mix(c,white,smoothstep(.7,.9,f)*heat);
 c+=vec3(1.,.45,.08)*pow(1.-uv.y,2.5)*.35*heat;
 gl_FragColor=vec4(c,1.);
}`;

export function MoltenFace({ heat }: { heat: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const heatRef = useRef(heat);
  heatRef.current = heat;
  const [noGl, setNoGl] = useState(false);
  const opacity = Math.min(1, Math.max(0, (heat - 0.35) / 0.4));

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const gl = canvas.getContext("webgl", { premultipliedAlpha: false, antialias: false });
    if (!gl) {
      setNoGl(true);
      return;
    }
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      setNoGl(true);
      return;
    }
    gl.useProgram(prog);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, "p");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const uR = gl.getUniformLocation(prog, "r");
    const uT = gl.getUniformLocation(prog, "t");
    const uH = gl.getUniformLocation(prog, "heat");

    let raf = 0;
    let checked = false;
    const start = performance.now();
    const frame = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      const w = Math.round(canvas.clientWidth * dpr);
      const h = Math.round(canvas.clientHeight * dpr);
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        gl.viewport(0, 0, w, h);
      }
      gl.uniform2f(uR, w, h);
      gl.uniform1f(uT, (performance.now() - start) / 1000);
      gl.uniform1f(uH, heatRef.current);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      if (!checked) {
        // Some drivers "succeed" and draw nothing; read one pixel back to be sure.
        checked = true;
        const px = new Uint8Array(4);
        gl.readPixels(Math.floor(w / 2), Math.floor(h / 2), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
        if (px[3] === 0 || px[0] + px[1] + px[2] === 0) {
          setNoGl(true);
          return;
        }
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    };
  }, []);

  // Without WebGL (some VMs, old drivers) the same look comes from layered moving gradients.
  if (noGl) return <span className="molten-css pointer-events-none absolute inset-0 rounded-[inherit]" style={{ opacity }} aria-hidden />;
  return (
    <canvas
      ref={ref}
      className="pointer-events-none absolute inset-0 h-full w-full rounded-[inherit]"
      style={{ opacity, transition: "opacity 120ms" }}
      aria-hidden
    />
  );
}

interface Spark {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  max: number;
  size: number;
}

/** Sparks spit off the button's edges while it's hot; `burst` (a counter) fires a shower. */
export function Sparks({ heat, burst }: { heat: number; burst: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const heatRef = useRef(heat);
  heatRef.current = heat;
  const sparks = useRef<Spark[]>([]);
  const lastBurst = useRef(burst);

  useEffect(() => {
    const canvas = ref.current!;
    const ctx = canvas.getContext("2d")!;
    const pad = 60;
    let raf = 0;
    let last = performance.now();

    const spawn = (n: number, power: number) => {
      const w = canvas.clientWidth - pad * 2;
      const h = canvas.clientHeight - pad * 2;
      for (let i = 0; i < n; i++) {
        // from a random point on the button's top or side edges
        const edge = Math.random();
        const x = edge < 0.7 ? pad + Math.random() * w : Math.random() < 0.5 ? pad : pad + w;
        const y = edge < 0.7 ? pad + Math.random() * 4 : pad + Math.random() * h;
        const angle = -Math.PI / 2 + (Math.random() - 0.5) * 2.2;
        const speed = (60 + Math.random() * 160) * power;
        sparks.current.push({
          x,
          y,
          vx: Math.cos(angle) * speed,
          vy: Math.sin(angle) * speed,
          life: 0,
          max: 0.35 + Math.random() * 0.55,
          size: 0.8 + Math.random() * 1.6,
        });
      }
    };

    const frame = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const w = canvas.clientWidth * dpr;
      const h = canvas.clientHeight * dpr;
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      if (lastBurst.current !== burstRef.current) {
        lastBurst.current = burstRef.current;
        spawn(70, 1.8);
      }
      if (heatRef.current > 0.6 && Math.random() < heatRef.current * 0.55) spawn(1, 1);

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, canvas.clientWidth, canvas.clientHeight);
      ctx.globalCompositeOperation = "lighter";
      sparks.current = sparks.current.filter((s) => (s.life += dt) < s.max);
      for (const s of sparks.current) {
        s.vy += 380 * dt; // gravity
        s.x += s.vx * dt;
        s.y += s.vy * dt;
        const k = 1 - s.life / s.max;
        ctx.strokeStyle = `rgba(255,${Math.round(160 + 80 * k)},${Math.round(60 * k)},${k})`;
        ctx.lineWidth = s.size;
        ctx.beginPath();
        ctx.moveTo(s.x, s.y);
        ctx.lineTo(s.x - s.vx * 0.025, s.y - s.vy * 0.025);
        ctx.stroke();
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  const burstRef = useRef(burst);
  burstRef.current = burst;

  return <canvas ref={ref} className="pointer-events-none absolute -inset-[60px] z-20 h-[calc(100%+120px)] w-[calc(100%+120px)]" aria-hidden />;
}
