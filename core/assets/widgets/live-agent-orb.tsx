// The icon of a live session agent: a faint sphere with plasma inside that does not fill it. Everything about an orb
// comes from a seed, and the seed from the agent's number.
//
// All the orbs are drawn by one hidden WebGL canvas and copied to plain
// canvases, because a browser allows only a few WebGL canvases a page.
import { useEffect, useRef } from "react";

/** The kinds of plasma the shader draws, in its order. */
const kinds = ["cloud", "filaments", "ribbons", "swirl", "wisps", "ring"];
/** The kinds an agent's orb is one of. */
const mix = ["cloud", "filaments", "ribbons", "swirl", "ring"];

function hash(text: string) {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h >>> 0;
}

/** A repeatable random number source: the same seed gives the same numbers. */
function random(seed: number) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A colour of full strength from a hue, as red, green and blue from 0 to 1. */
function rgb(hue: number, light: number): [number, number, number] {
  const f = (k: number) => {
    const at = (k + hue / 30) % 12;
    return light - Math.min(light, 1 - light) * Math.max(-1, Math.min(at - 3, 9 - at, 1));
  };
  return [f(0), f(8), f(4)];
}

/** What a seed decides about one orb. */
function orb(seed: number) {
  const r = random(seed);
  const hue = r() * 360;
  const second = (hue + (r() < 0.5 ? 1 : -1) * (25 + r() * 70) + 360) % 360;
  return {
    hue,
    second,
    a: rgb(hue, 0.5),
    b: rgb(second, 0.62),
    at: [r() * 40, r() * 40] as [number, number],
    fill: 0.45 + r() * 0.4,
    speed: 0.6 + r() * 0.7,
    arms: 1 + Math.floor(r() * 2),
  };
}

const vertex = `attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }`;

const fragment = `
precision highp float;
uniform vec2 uRes;
uniform float uTime;
uniform vec2 uSeed;    // where in the noise this orb lives
uniform vec3 uA;       // the plasma's colour
uniform vec3 uB;       // its second colour, where it is denser
uniform float uKind;   // 0 cloud, 1 filaments, 2 ribbons, 3 swirl, 4 wisps, 5 ring
uniform float uFill;   // how much of the sphere the plasma takes, 0 .. 1
uniform float uArms;

float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  mat2 m = mat2(0.8, -0.6, 0.6, 0.8);
  for (int i = 0; i < 4; i++) { v += a * noise(p); p = m * p * 2.03; a *= 0.5; }
  return v;
}

void main() {
  vec2 u = (gl_FragCoord.xy - 0.5 * uRes) / (0.5 * uRes.y) / 0.94;
  float r = length(u);
  float t = uTime;
  float z = sqrt(max(1.0 - r * r, 0.0));
  vec2 s = u / (1.0 + z);                        // bulge: the middle is closer
  vec2 q = s * 1.9 + uSeed;
  vec2 warp = vec2(fbm(q + vec2(0.0, t * 0.35)), fbm(q + vec2(5.2, -t * 0.3)));
  float f = fbm(q * 1.4 + 2.2 * warp + vec2(0.0, t * 0.5));
  float d = 0.0;

  if (uKind < 0.5) {
    // A cloud with a ragged edge, somewhere inside the glass.
    float edge = r + (f - 0.5) * 1.1;
    d = smoothstep(uFill + 0.12, uFill - 0.32, edge) * (0.45 + 1.3 * smoothstep(0.28, 0.66, f));
  } else if (uKind < 1.5) {
    // Threads all through the glass, brighter in some places. No core. They are few and thick enough to survive
    // being scaled down to an icon.
    float lines = pow(1.0 - abs(sin(f * 9.0 + t * 1.6)), 3.0);
    float where = smoothstep(0.25, 0.7, fbm(q * 0.9 + vec2(t * 0.2, 3.1)));
    d = lines * (0.85 + 1.3 * where) * smoothstep(0.8 + 0.2 * uFill, 0.3, r) * 1.5;
  } else if (uKind < 2.5) {
    // Ribbons that flow across the glass, like an aurora.
    float turn = uSeed.x;
    vec2 v = mat2(cos(turn), -sin(turn), sin(turn), cos(turn)) * s;
    float band = pow(1.0 - abs(sin(v.y * (2.0 + 2.0 * uFill) + (f - 0.5) * 4.5 + t * 0.4)), 3.0);
    d = band * (0.55 + 1.3 * smoothstep(0.3, 0.7, f)) * smoothstep(0.98, 0.4, r) * 1.3;
  } else if (uKind < 3.5) {
    // A vortex off the middle: arms that turn, torn by the noise, with nothing at their centre.
    vec2 off = u - 0.22 * vec2(sin(uSeed.x), cos(uSeed.y));
    float far = length(off);
    float arm = sin(atan(off.y, off.x) * uArms + far * 6.0 - t * 1.3 + (f - 0.5) * 7.0);
    d = pow(0.5 + 0.5 * arm, 2.5) * smoothstep(0.05, 0.3, far) * smoothstep(uFill + 0.3, 0.2, r) * (0.35 + 1.4 * f);
  } else if (uKind < 4.5) {
    // A few drops that drift, meet and part.
    float m = 0.0;
    for (int i = 0; i < 4; i++) {
      float k = float(i) * 1.7 + uSeed.x;
      vec2 c = (0.25 + 0.3 * uFill) * vec2(sin(t * (0.5 + 0.13 * float(i)) + k), cos(t * (0.4 + 0.11 * float(i)) + k * 1.3));
      vec2 e = u - c;
      m += 0.034 / (dot(e, e) + 0.014);
    }
    d = smoothstep(0.85, 2.0, m + (f - 0.5) * 1.6) * (0.7 + 0.6 * f);
  } else {
    // A ring of plasma that leaves the middle empty.
    float off = (r - 0.52 - 0.22 * (f - 0.5)) / (0.07 + 0.12 * uFill);
    d = exp(-off * off) * (0.35 + 1.3 * smoothstep(0.3, 0.75, f));
  }
  d *= 0.55 + 0.45 * z;                           // fades toward the glass
  d = clamp(d, 0.0, 1.6);

  vec3 white = vec3(1.0, 0.96, 0.92);
  vec3 plasma = mix(uA, uB, clamp(d * 0.9, 0.0, 1.0));
  // The hottest part goes only a little toward white: on a light page, white would read as a hole.
  plasma = mix(plasma, white, pow(clamp(d - 0.6, 0.0, 1.0), 1.5) * 0.3);
  float body = clamp(d, 0.0, 1.0);

  // The sphere itself: a faint tint of the agent's colour inside, and a contour of that colour that grows toward the
  // edge, strong enough to show when the orb is scaled down to an icon.
  float rim = pow(1.0 - z, 2.2);
  float edge = rim * 0.7;
  float haze = 0.07;
  float inside = smoothstep(1.0, 0.95, r);
  float alpha = clamp(body + haze + edge, 0.0, 1.0) * inside;
  vec3 color = (plasma * body + mix(vec3(0.45), uA, 0.5) * haze + mix(uA, uB, 0.25) * edge) / max(body + haze + edge, 0.0001);
  gl_FragColor = vec4(color * alpha, alpha);
}
`;

interface Drawn {
  canvases: HTMLCanvasElement[];
  kind: number;
  made: ReturnType<typeof orb>;
  /** The plasma rests: it is drawn at one moment, and does not move. */
  still?: boolean;
}

/** The one WebGL canvas every orb on the page is drawn with. */
let shared: { add: (drawn: Drawn) => () => void } | null = null;

function renderer() {
  if (shared) return shared;
  const element = document.createElement("canvas");
  const gl = element.getContext("webgl", { premultipliedAlpha: true, alpha: true, antialias: false, preserveDrawingBuffer: true });
  const all = new Set<Drawn>();
  if (!gl) return (shared = { add: () => () => {} });
  const compile = (type: number, source: string) => {
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) console.error(gl.getShaderInfoLog(shader));
    return shader;
  };
  const program = gl.createProgram()!;
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vertex));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragment));
  gl.linkProgram(program);
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const at = gl.getAttribLocation(program, "p");
  gl.enableVertexAttribArray(at);
  gl.vertexAttribPointer(at, 2, gl.FLOAT, false, 0, 0);
  const u = Object.fromEntries(["uRes", "uTime", "uSeed", "uA", "uB", "uKind", "uFill", "uArms"].map((name) => [name, gl.getUniformLocation(program, name)]));
  const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  let frame = 0;

  // An orb is drawn once, at the size of its large canvas, and copied to both: the small icon is the same picture,
  // scaled down.
  const draw = (time: number) => {
    frame = all.size > 0 ? requestAnimationFrame(draw) : 0;
    const scale = Math.min(window.devicePixelRatio || 1, 2);
    for (const drawn of all) {
      const box = drawn.canvases[0].getBoundingClientRect();
      if (box.bottom < -50 || box.top > window.innerHeight + 50 || box.width === 0) continue;
      const size = Math.round(box.width * scale);
      if (element.width !== size) element.width = element.height = size;
      gl.viewport(0, 0, size, size);
      gl.uniform2f(u.uRes, size, size);
      gl.uniform1f(u.uTime, (calm || drawn.still ? 3 : time / 1000) * drawn.made.speed + drawn.made.at[0]);
      gl.uniform2f(u.uSeed, ...drawn.made.at);
      gl.uniform3f(u.uA, ...drawn.made.a);
      gl.uniform3f(u.uB, ...drawn.made.b);
      gl.uniform1f(u.uKind, drawn.kind);
      gl.uniform1f(u.uFill, drawn.made.fill);
      gl.uniform1f(u.uArms, drawn.made.arms);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      for (const canvas of drawn.canvases) {
        const side = Math.round(canvas.clientWidth * scale);
        if (side === 0) continue;
        if (canvas.width !== side) canvas.width = canvas.height = side;
        const context = canvas.getContext("2d")!;
        context.imageSmoothingQuality = "high";
        context.clearRect(0, 0, side, side);
        context.drawImage(element, 0, 0, side, side);
      }
    }
  };
  return (shared = {
    add: (drawn) => {
      all.add(drawn);
      if (!frame) frame = requestAnimationFrame(draw);
      return () => all.delete(drawn);
    },
  });
}

/** One agent's orb, as an icon: its colours and its plasma come from the agent's number, so they never change. It
    takes the size its class gives it. `still` rests the plasma. */
export function AgentOrb({ id, still = false, className }: { id: number; still?: boolean; className?: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    if (!canvas.current) return;
    const seed = hash(`agent ${id}`);
    return renderer().add({ canvases: [canvas.current], kind: kinds.indexOf(mix[seed % mix.length]), made: orb(seed), still });
  }, [id, still]);
  return <canvas ref={canvas} className={className} aria-hidden="true" />;
}
