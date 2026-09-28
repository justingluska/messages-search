import { useEffect, useRef } from "react";

/**
 * Ambient home background: calm tropical water seen from above. One WebGL
 * canvas, one full-screen triangle, one procedural fragment shader (no
 * images). Caustic light (two domain-warped layers combined with min(), with
 * slight chromatic dispersion) falls on a sand seabed and is attenuated by
 * depth (Beer-Lambert plus in-scattering), so the turquoise shallows, the teal
 * and the navy depths all come from the same physics.
 *
 * Budget: about 1 M pixels (the water is soft, so the CSS upscale is
 * invisible), one draw call per display frame at the full refresh rate, with
 * all motion time-based and no per-frame allocations. The pointer leaves a
 * dense, short-lived wake (points spawned by distance, interpolated across
 * fast moves) and eases a slight parallax. It stops when the window is hidden
 * or unfocused for more than 10 s (30 fps while unfocused), draws a single still frame (no wake) under
 * prefers-reduced-motion, and releases the GL context on unmount (it only
 * exists on the home screen). Without WebGL it renders nothing.
 */

const VERT = `
attribute vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }
`;

const FRAG = `
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 uRes;      // drawing-buffer size, px
uniform float uTime;    // seconds (wrapped)
uniform float uDark;    // 1 dark, 0 light
uniform vec2 uFocus;    // search pill centre, 0..1 (y up)
uniform float uScale;   // drawing-buffer px per CSS px
uniform vec3 uTrail[32]; // pointer trail points: CSS px (y up), birth time in uTime seconds
uniform float uTrailAlive; // 1 while any trail point is alive, else the loop is skipped
uniform float uRadius;  // trail disturbance radius, CSS px
uniform vec2 uPar;      // eased pointer position minus centre, -0.5..0.5 (parallax)

const float TAU = 6.28318530718;

float hash12(vec2 p) {
  vec3 p3 = fract(vec3(p.xyx) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash12(i), hash12(i + vec2(1.0, 0.0)), u.x),
             mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), u.x), u.y);
}

// Water caustic by iterative domain warping (the idea behind the classic
// tileable water caustic): each pass bends the point by the previous one and
// accumulates inverse distances; where the warps converge, light gathers into
// organic filaments of varying width. Tiles every 1.0 in uv.
float causticField(vec2 uv, float t) {
  vec2 p = mod(uv * TAU, TAU) - 250.0;
  vec2 q = p;
  float c = 1.0;
  for (int n = 0; n < 5; n++) {
    float tn = t * (1.0 - 3.5 / float(n + 1));
    q = p + vec2(cos(tn - q.x) + sin(tn + q.y), sin(tn - q.y) + cos(tn + q.x));
    c += 1.0 / length(vec2(p.x / (sin(q.x + tn) * 200.0), p.y / (cos(q.y + tn) * 200.0)));
  }
  c /= 5.0;
  c = 1.17 - pow(c, 1.4);
  return clamp(pow(abs(c), 8.0), 0.0, 3.0);
}

// Two layers at different scales, speeds and drifts; min() keeps light only
// where both converge, which breaks up any regular pattern.
float caustics(vec2 uv, float t) {
  // Each layer drifts with the pointer by a different amount: a floaty parallax.
  float a = causticField(uv + vec2(0.011, 0.006) * t - uPar * 0.012, t * 0.75);
  float b = causticField(uv * 1.35 + vec2(0.37, 0.81) - vec2(0.008, 0.012) * t - uPar * 0.03, t);
  return min(a, b);
}

void main() {
  vec2 frag = gl_FragCoord.xy;
  vec2 uv = frag / uRes;
  float aspect = uRes.x / uRes.y;
  // Calm speed; the offset skips the start, where every iteration's phase lines
  // up and the pattern collapses into straight bars.
  float t = uTime * 0.32 + 23.0;
  vec2 css = frag / uScale;

  // Gentle surface normal field: refraction wobble for both the sand and the light.
  vec2 lp = css / 260.0;
  vec2 wob = vec2(noise(lp * 1.3 + uTime * 0.05), noise(lp * 1.3 + 5.2 - uTime * 0.045)) - 0.5;

  // Pointer wake: dense, short-lived points, each a soft Gaussian bump with a
  // quadratic fade. Overlapping bumps blend into one continuous wake that
  // refracts the light and sand (displacement along the bump's slope).
  vec2 disp = vec2(0.0);
  float lift = 0.0;
  float inv2r2 = 1.0 / (2.0 * uRadius * uRadius);
  if (uTrailAlive > 0.5) for (int i = 0; i < 32; i++) {
    vec3 p = uTrail[i];
    float age = uTime - p.z;
    if (age < 0.0 || age > 0.45) continue;
    float f = 1.0 - age / 0.45;
    vec2 dv = css - p.xy;
    float g = exp(-dot(dv, dv) * inv2r2) * f * f;
    lift += g;
    disp += dv * (g / uRadius);
  }
  lift = min(lift, 1.5);

  // Caustic light with slight chromatic dispersion (R, G, B sampled a hair apart).
  vec2 cuv = css / 520.0 + wob * 0.05 + disp * (28.0 / 520.0);
  vec2 split = vec2(0.0035, -0.0025);
  vec3 light = vec3(caustics(cuv + split, t), caustics(cuv, t), caustics(cuv - split, t));

  // Seabed: warm pale sand, low-frequency variation, faint ripple streaks.
  vec2 sp = css / 420.0 + wob * 0.02 + disp * (15.0 / 420.0) - uPar * 0.006;
  float sandVar = noise(sp * 1.7) * 0.6 + noise(sp * 4.1) * 0.4;
  float ripples = sin((sp.x * 0.8 + sp.y * 0.45) * 48.0 + noise(sp * 3.0) * 6.0);
  vec3 sand = vec3(0.97, 0.94, 0.85) * (0.9 + 0.12 * sandVar) * (1.0 + 0.018 * ripples);

  // Depth: nearly flat, so the whole window stays mid turquoise (no bright
  // shallows behind the search); only slightly deeper toward the edges/bottom.
  float r = length((uv - vec2(uFocus.x, uFocus.y - 0.04)) * vec2(aspect * 0.72, 1.0));
  float depth = 0.95 + 0.55 * smoothstep(0.15, 1.1, r) + 0.35 * smoothstep(0.6, 0.0, uv.y)
              + 0.3 * (noise(css / 520.0) - 0.5);
  // Quieter, capped light behind the content: highlights stay turquoise, never white.
  float calm = 1.0 - 0.5 * exp(-r * r * 6.0);
  light = min(light * (1.0 + lift * 0.75), vec3(1.0));

  vec3 absorb = vec3(1.8, 0.40, 0.52);            // red is absorbed first, then blue: shallows turn teal-green
  vec3 col;
  if (uDark > 0.5) {
    depth += 0.8;
    vec3 lit = sand * (0.40 + light * 0.75 * calm);
    vec3 T = exp(-depth * vec3(1.8, 0.44, 0.40));   // keeps more blue: moody teal, not emerald
    col = lit * T + vec3(0.012, 0.110, 0.141) * (1.0 - T);   // #031C24 in-scatter
    col *= mix(1.0, 0.35, smoothstep(0.95, 1.5, r));        // edges fall to near black
    col += vec3(0.30, 0.75, 0.72) * lift * 0.05;
  } else {
    vec3 lit = sand * (0.9 + light * 0.75 * calm);
    vec3 T = exp(-depth * absorb);
    col = lit * T + vec3(0.024, 0.271, 0.310) * (1.0 - T);   // #06454F in-scatter
    col += vec3(0.85, 1.0, 0.97) * lift * 0.08;
  }
  gl_FragColor = vec4(col, 1.0);
}
`;

/** Unfocused but visible: 30 fps, then a full pause after this long. */
const UNFOCUSED_FPS = 30;
const UNFOCUSED_PAUSE_MS = 10_000;
const TRAIL_LIFE_S = 0.45;

function compile(gl: WebGLRenderingContext, type: number, src: string): WebGLShader | null {
  const s = gl.createShader(type);
  if (!s) return null;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    console.warn("home background: shader failed", gl.getShaderInfoLog(s));
    gl.deleteShader(s);
    return null;
  }
  return s;
}

export function HomeBackground() {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    // A fresh canvas per mount: a context released on unmount can't be reused.
    const canvas = document.createElement("canvas");
    canvas.className = "home-bg-canvas";
    host.appendChild(canvas);
    const gl = canvas.getContext("webgl", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      premultipliedAlpha: false,
      preserveDrawingBuffer: false,
      powerPreference: "low-power",
    });
    if (!gl) {
      canvas.remove();
      return;
    }

    const vs = compile(gl, gl.VERTEX_SHADER, VERT);
    const fs = compile(gl, gl.FRAGMENT_SHADER, FRAG);
    const prog = gl.createProgram();
    if (!vs || !fs || !prog) {
      canvas.remove();
      return;
    }
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      canvas.remove();
      return;
    }
    gl.useProgram(prog);

    // One oversized triangle covers the screen.
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const aPos = gl.getAttribLocation(prog, "aPos");
    gl.enableVertexAttribArray(aPos);
    gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

    const uRes = gl.getUniformLocation(prog, "uRes");
    const uTime = gl.getUniformLocation(prog, "uTime");
    const uDark = gl.getUniformLocation(prog, "uDark");
    const uFocus = gl.getUniformLocation(prog, "uFocus");
    const uScale = gl.getUniformLocation(prog, "uScale");
    const uTrail = gl.getUniformLocation(prog, "uTrail");
    const uTrailAlive = gl.getUniformLocation(prog, "uTrailAlive");
    let newestBorn = -1e4;
    const uRadius = gl.getUniformLocation(prog, "uRadius");
    const uPar = gl.getUniformLocation(prog, "uPar");

    // Pointer trail: 32 points (x, y, born) in a preallocated ring buffer.
    // Moves only write numbers into it; it's uploaded once per frame.
    const TRAIL = 32;
    const trail = new Float32Array(TRAIL * 3).fill(-1e4);
    let head = 0;
    let lastX = NaN;
    let lastY = NaN;
    let lastBorn = 0;
    let boxLeft = 0;
    let boxTop = 0;
    let boxW = 1;
    let boxH = 1;
    let spacing = 12;
    // Eased pointer for parallax: target (ix, iy) and smoothed (bx, by), 0..1.
    let ix = 0.5;
    let iy = 0.5;
    let bx = 0.5;
    let by = 0.5;

    const darkMq = matchMedia("(prefers-color-scheme: dark)");
    const motionMq = matchMedia("(prefers-reduced-motion: reduce)");
    const start = performance.now();
    let raf = 0;
    let prev = start;
    let running = false;
    let blurTimer = 0;
    // A window launched in the background never gets a "blur" event, so read focus now.
    let focused = document.hasFocus();
    let paused = false;
    let lastDraw = 0;

    const clock = (now: number) => ((now - start) / 1000) % 3600;
    const draw = (now: number) => {
      const t = clock(now);
      gl.uniform1f(uTime, t);
      const alive = t - newestBorn < TRAIL_LIFE_S;
      gl.uniform1f(uTrailAlive, alive ? 1 : 0);
      if (alive) gl.uniform3fv(uTrail, trail);
      gl.uniform2f(uPar, bx - 0.5, by - 0.5);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    };

    const push = (x: number, y: number, born: number) => {
      const o = head * 3;
      trail[o] = x;
      trail[o + 1] = y;
      trail[o + 2] = born;
      if (born > newestBorn) newestBorn = born;
      head = (head + 1) % TRAIL;
    };

    // One pointer sample: spawn points by distance travelled (not time), and
    // fill long jumps with interpolated points (positions and birth times) so a
    // fast sweep stays one continuous wake instead of beads.
    const sample = (clientX: number, clientY: number, born: number) => {
      const x = clientX - boxLeft;
      const y = boxH - (clientY - boxTop);
      if (Number.isNaN(lastX)) {
        lastX = x;
        lastY = y;
        lastBorn = born;
        push(x, y, born);
        return;
      }
      const dx = x - lastX;
      const dy = y - lastY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < spacing) return;
      const steps = Math.min(TRAIL, Math.floor(dist / spacing));
      for (let k = 1; k <= steps; k++) {
        const f = k / steps;
        push(lastX + dx * f, lastY + dy * f, lastBorn + (born - lastBorn) * f);
      }
      lastX = x;
      lastY = y;
      lastBorn = born;
    };

    const onPointer = (e: PointerEvent) => {
      const born = clock(performance.now());
      const coalesced = e.getCoalescedEvents ? e.getCoalescedEvents() : null;
      if (coalesced && coalesced.length > 1) for (let k = 0; k < coalesced.length; k++) sample(coalesced[k].clientX, coalesced[k].clientY, born);
      else sample(e.clientX, e.clientY, born);
      ix = (e.clientX - boxLeft) / boxW;
      iy = 1 - (e.clientY - boxTop) / boxH;
    };

    const resize = () => {
      // About a 1 M pixel budget (soft water hides it), never above 2x or the
      // device ratio: keeps a full-refresh frame well under 1.5 ms of GPU.
      const cw = Math.max(1, canvas.clientWidth);
      const ch = Math.max(1, canvas.clientHeight);
      const scale = Math.min(window.devicePixelRatio || 1, 2, Math.sqrt(1.0e6 / (cw * ch)));
      const w = Math.max(1, Math.round(cw * scale));
      const h = Math.max(1, Math.round(ch * scale));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      gl.viewport(0, 0, w, h);
      gl.uniform2f(uRes, w, h);
      gl.uniform1f(uScale, w / cw);
      // Anchor the calm area to the search pill.
      const pill = host.parentElement?.querySelector(".search-pill");
      const box = canvas.getBoundingClientRect();
      boxLeft = box.left;
      boxTop = box.top;
      boxW = Math.max(1, box.width);
      boxH = Math.max(1, box.height);
      spacing = Math.max(6, 0.012 * boxH);
      gl.uniform1f(uRadius, 0.055 * boxH);
      if (pill && box.height > 0) {
        const r = pill.getBoundingClientRect();
        gl.uniform2f(uFocus, (r.left + r.width / 2 - box.left) / box.width, 1 - (r.top + r.height / 2 - box.top) / box.height);
      } else gl.uniform2f(uFocus, 0.5, 0.55);
      draw(performance.now());
    };

    // Every display frame (60/120 Hz) while focused; 30 fps when visible but
    // unfocused. All motion is time-based, so both rates look the same.
    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);
      if (!focused) {
        const step = 1000 / UNFOCUSED_FPS;
        if (now - lastDraw < step - 1) return;
        lastDraw = Math.max(lastDraw + step, now - step);
      } else lastDraw = now;
      const dt = Math.min((now - prev) / 1000, 0.05);
      prev = now;
      const k = 1 - Math.exp(-dt * 6);
      bx += k * (ix - bx);
      by += k * (iy - by);
      draw(now);
    };

    const shouldRun = () => !motionMq.matches && !document.hidden && !paused;
    // Trail and parallax only with motion allowed.
    const syncPointer = () => {
      window.removeEventListener("pointermove", onPointer);
      if (!motionMq.matches) window.addEventListener("pointermove", onPointer, { passive: true });
    };
    const sync = () => {
      const want = shouldRun();
      if (want && !running) {
        running = true;
        prev = performance.now();
        raf = requestAnimationFrame(tick);
      } else if (!want && running) {
        running = false;
        cancelAnimationFrame(raf);
      }
    };

    const setTheme = () => {
      gl.uniform1f(uDark, darkMq.matches ? 1 : 0);
      draw(performance.now());
    };

    const onBlur = () => {
      focused = false;
      window.clearTimeout(blurTimer);
      blurTimer = window.setTimeout(() => {
        paused = true;
        sync();
      }, UNFOCUSED_PAUSE_MS);
    };
    const onFocus = () => {
      focused = true;
      window.clearTimeout(blurTimer);
      paused = false;
      sync();
    };
    if (!focused) onBlur();

    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    setTheme();
    resize();
    sync();
    syncPointer();
    const onMotion = () => {
      syncPointer();
      sync();
    };

    darkMq.addEventListener("change", setTheme);
    motionMq.addEventListener("change", onMotion);
    document.addEventListener("visibilitychange", sync);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);

    return () => {
      running = false;
      cancelAnimationFrame(raf);
      window.clearTimeout(blurTimer);
      ro.disconnect();
      darkMq.removeEventListener("change", setTheme);
      motionMq.removeEventListener("change", onMotion);
      window.removeEventListener("pointermove", onPointer);
      document.removeEventListener("visibilitychange", sync);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      gl.deleteBuffer(buf);
      gl.deleteProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.getExtension("WEBGL_lose_context")?.loseContext();
      canvas.remove();
    };
  }, []);

  return <div ref={hostRef} className="home-bg" aria-hidden="true" />;
}
