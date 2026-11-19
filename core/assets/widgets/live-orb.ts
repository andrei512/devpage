// The live session's orb, drawn by a fragment shader: a faint sphere with plasma inside that does not fill it. The
// plasma is purple while the session is on and listening. A voice makes it turbulent, as fast and as torn as the voice
// is loud: red for the user's, blue for the AI's, and nothing else differs between the two. When the session is off the
// sphere is empty and grey. A ring turns around it while work is running.

/** What the orb shows right now. */
export interface Shown {
  /** The session is on. */
  on: boolean;
  /** The user's voice and the AI's, 0 quiet to 1 loud. */
  me: number;
  ai: number;
  /** The session is starting. */
  starting: boolean;
  /** Tasks are running. */
  working: boolean;
}

const vertex = `attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }`;

const fragment = `
precision highp float;
uniform vec2 uRes;
uniform float uTime;   // seconds, already scaled by the state's speed
uniform float uOn;     // 0 off .. 1 on
uniform float uMe;     // the user's voice, 0 quiet .. 1 loud
uniform float uAi;     // the AI's voice, 0 quiet .. 1 loud
uniform float uThink;  // 0 .. 1: the session is starting
uniform float uWork;   // 0 .. 1: the agent is working on tasks
uniform float uLight;  // 1 on a light page

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
  vec2 p = (gl_FragCoord.xy - 0.5 * uRes) / (0.5 * uRes.y);
  float me = uMe * uOn;
  float ai = uAi * uOn;
  float loud = max(me, ai);
  float R = 0.33 + 0.055 * loud;               // it swells with each word
  vec2 u = p / R;
  float r = length(u);
  float t = uTime;

  // The colours: purple at rest, toward red with the user's voice, toward blue with the AI's.
  // The two voices behave the same way. Only the colour says whose it is, and it is full already at a quiet word.
  float mine = clamp(me * 3.0, 0.0, 1.0);
  float its = clamp(ai * 3.0, 0.0, 1.0) * (1.0 - mine);
  float voiced = max(mine, its);
  vec3 a = mix(mix(vec3(0.53, 0.30, 0.92), vec3(0.06, 0.20, 0.78), its), vec3(0.76, 0.12, 0.10), mine);   // a deep blue for the AI, a deep red for the user
  vec3 b = mix(mix(vec3(0.80, 0.52, 1.00), vec3(0.14, 0.42, 0.95), its), vec3(0.90, 0.30, 0.24), mine);   // #e64c3c, the brand's red, where it is densest
  vec3 white = vec3(1.0, 0.96, 0.92);

  // Inside the sphere: plasma that does not fill it.
  float z = sqrt(max(1.0 - r * r, 0.0));
  vec2 s = u / (1.0 + z);                        // bulge: the middle is closer
  float ang = t * (0.25 + 1.4 * uThink);
  vec2 q = mat2(cos(ang), -sin(ang), sin(ang), cos(ang)) * s * 1.7;
  vec2 warp = vec2(fbm(q + vec2(0.0, t * 0.35)), fbm(q + vec2(5.2, -t * 0.3)));
  // A voice tears the plasma: the louder, the finer and the more twisted.
  float f = fbm(q * (1.4 + 0.9 * loud) + (2.2 + 3.0 * loud) * warp + vec2(0.0, t * 0.5));

  // A cloud with a ragged edge, which grows with a voice, and threads through it.
  float fill = 0.58 + 0.30 * loud + 0.10 * uThink;
  float cloud = smoothstep(fill + 0.12, fill - 0.32, r + (f - 0.5) * 1.1) * (0.45 + 1.3 * smoothstep(0.28, 0.66, f));
  float lines = pow(1.0 - abs(sin(f * 9.0 + t * 1.6)), 3.0) * smoothstep(0.92, 0.3, r);
  float d = cloud + lines * (0.20 + 0.5 * uThink + 1.3 * loud);
  d *= uOn * (0.55 + 0.45 * z);                  // fades toward the edge; none when the session is off
  d = clamp(d, 0.0, 1.6);
  vec3 plasma = mix(a, b, clamp(d * 0.9, 0.0, 1.0));
  // The hottest part goes only a little toward white: on a light page, white would read as a hole.
  plasma = mix(plasma, white, pow(clamp(d - 0.6, 0.0, 1.0), 1.5) * 0.3 * (1.0 - voiced));
  // A voice's plasma is denser than the purple, so that the page behind does not wash it out.
  float body = clamp(d * (1.0 + 0.5 * voiced), 0.0, 1.0);

  // The sphere itself: a faint tint inside, and a contour that grows toward the edge. Grey when the session is off.
  vec3 grey = mix(vec3(0.62), vec3(0.50), uLight);
  vec3 skin = mix(grey, mix(a, b, 0.25), uOn);
  float edge = pow(1.0 - z, 2.2) * 0.7;
  float haze = 0.07;
  float alphaIn = clamp(body + haze + edge, 0.0, 1.0);
  vec3 colIn = (plasma * body + mix(vec3(0.45), skin, 0.5) * haze + skin * edge) / max(body + haze + edge, 0.0001);
  float inside = smoothstep(1.0, 0.95, r);

  // Outside: a soft glow in the voice's colour, and the ring that turns while the agent works. The ring is grey when
  // the session is off and purple when it is on: never a voice's colour.
  float out_ = max(r - 1.0, 0.0);
  float glow = exp(-5.0 * out_) * 0.40 * loud;
  float around = atan(p.y, p.x);
  float ring = smoothstep(0.07, 0.0, abs(r - 1.26)) * smoothstep(0.2, 0.75, sin(around * 3.0 - t * 2.4)) * uWork;
  float dots = smoothstep(0.04, 0.0, abs(r - 1.26)) * 0.18 * uWork;
  float ringA = clamp(ring + dots, 0.0, 1.0);
  vec3 ringCol = mix(grey, vec3(0.53, 0.30, 0.92), uOn);
  float alphaOut = clamp(glow + ringA, 0.0, 1.0);
  vec3 colOut = (a * glow + ringCol * ringA) / max(glow + ringA, 0.0001);

  float alpha = mix(alphaOut, alphaIn, inside);
  vec3 color = mix(colOut, colIn, inside);
  gl_FragColor = vec4(color * alpha, alpha);
}
`;

/** Draws the orb on `element`, asking `shown` each frame. Returns the function that stops it. */
export function drawOrb(element: HTMLCanvasElement, shown: () => Shown) {
  const gl = element.getContext("webgl", { premultipliedAlpha: true, alpha: true, antialias: false });
  if (!gl) return () => {};
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
  const uniform = (name: string) => gl.getUniformLocation(program, name);
  const [uRes, uTime, uOn, uMe, uAi, uThink, uWork, uLight] = ["uRes", "uTime", "uOn", "uMe", "uAi", "uThink", "uWork", "uLight"].map(uniform);

  const calm = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const now = { on: 0, me: 0, ai: 0, think: 0, work: 0 };
  let clock = 0;
  let last = performance.now();
  let frame = 0;

  const draw = (time: number) => {
    frame = requestAnimationFrame(draw);
    const dt = Math.min(0.05, (time - last) / 1000);
    last = time;
    const { on, me, ai, starting, working } = shown();
    const ease = (from: number, to: number, speed: number) => from + (to - from) * Math.min(1, dt * speed);
    now.on = ease(now.on, on || starting ? 1 : 0, 3);
    // A voice as the orb shows it: lifted, so that ordinary speech already moves it, rising at once and falling fast.
    const lift = (level: number) => Math.pow(Math.max(0, Math.min(1, level)), 0.6);
    now.me = lift(me) > now.me ? lift(me) : ease(now.me, lift(me), 10);
    now.ai = lift(ai) > now.ai ? lift(ai) : ease(now.ai, lift(ai), 10);
    now.think = ease(now.think, starting ? 1 : 0, 3);
    now.work = ease(now.work, working ? 1 : 0, 4);
    // The plasma flows several times faster while someone speaks, as fast as the voice is loud.
    clock += dt * (calm ? 0.25 : 0.55 + 0.9 * now.think + 4.0 * Math.max(now.me, now.ai));

    const scale = Math.min(window.devicePixelRatio || 1, 2);
    const size = Math.round(element.clientWidth * scale);
    if (element.width !== size) element.width = element.height = size;
    gl.viewport(0, 0, size, size);
    const theme = document.documentElement.dataset.theme;
    const dark = theme ? theme === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
    gl.uniform2f(uRes, size, size);
    gl.uniform1f(uTime, clock);
    gl.uniform1f(uOn, now.on);
    gl.uniform1f(uMe, now.me);
    gl.uniform1f(uAi, now.ai);
    gl.uniform1f(uThink, now.think);
    gl.uniform1f(uWork, now.work);
    gl.uniform1f(uLight, dark ? 0 : 1);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  };
  frame = requestAnimationFrame(draw);
  return () => cancelAnimationFrame(frame);
}
