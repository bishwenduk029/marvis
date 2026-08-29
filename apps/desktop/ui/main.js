// Marvis — Siri-style wavy line at the bottom of a small transparent window.
// rAF loop runs ONLY while active; idle draws one static frame and stops.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const canvas = document.getElementById("orb");
const ctx = canvas.getContext("2d");
const hint = document.getElementById("hint");
const caption = document.getElementById("caption");

const STYLE = {
  idle:      { hue: 215, accent: 260, sat: 70 },
  listening: { hue: 185, accent: 200, sat: 95 },
  thinking:  { hue: 42,  accent: 20,  sat: 95 },
  speaking:  { hue: 155, accent: 175, sat: 90 },
};

let state = "idle";
let hue = STYLE.idle.hue;
let accent = STYLE.idle.accent;
let sat = STYLE.idle.sat;
let energy = 0;
let targetEnergy = 0;
let rafId = null;
let t = 0;
let captionTimer = null;

function size() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = innerWidth * dpr;
  canvas.height = innerHeight * dpr;
  canvas.style.width = innerWidth + "px";
  canvas.style.height = innerHeight + "px";
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  draw();
}
addEventListener("resize", size);

// Siri-style wavy line: a smooth glowing ribbon that flows over time and
// rises with voice energy.
function drawWave(amplitude, glowBoost) {
  const cx = innerWidth / 2;
  const cy = innerHeight * 0.72; // wave sits near the bottom
  const width = innerWidth * 0.86;
  const n = 72;

  // soft glow under the wave
  const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, innerWidth * 0.55);
  g.addColorStop(0, `hsla(${hue},${sat}%,60%,${0.30 + glowBoost * 0.25})`);
  g.addColorStop(1, "hsla(0,0%,0%,0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, innerWidth, innerHeight);

  const amp = amplitude * (0.35 + energy * 0.65);
  const pts = [];
  for (let i = 0; i <= n; i++) {
    const x = cx - width / 2 + (i / n) * width;
    const e1 = Math.sin(i * 0.24 - t * 2.1);
    const e2 = Math.sin(i * 0.10 + t * 1.5) * 0.6;
    const e3 = Math.sin(i * 0.05 - t * 0.8) * 0.35;
    const y = cy - ((e1 + e2 + e3) / 1.95) * amp;
    pts.push({ x, y });
  }

  // filled soft area under the line
  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  for (const p of pts) ctx.lineTo(p.x, p.y);
  ctx.lineTo(pts[pts.length - 1].x, cy + amp);
  ctx.lineTo(pts[0].x, cy + amp);
  ctx.closePath();
  const fill = ctx.createLinearGradient(0, cy - amp, 0, cy + amp);
  fill.addColorStop(0, `hsla(${hue},${sat}%,65%,0.30)`);
  fill.addColorStop(1, "hsla(0,0%,0%,0)");
  ctx.fillStyle = fill;
  ctx.fill();

  // the glowing line
  ctx.beginPath();
  ctx.moveTo(pts[0].x, pts[0].y);
  for (const p of pts) ctx.lineTo(p.x, p.y);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  const line = ctx.createLinearGradient(cx - width / 2, 0, cx + width / 2, 0);
  line.addColorStop(0, `hsla(${accent},100%,75%,0.9)`);
  line.addColorStop(0.5, `hsla(${hue},100%,70%,1)`);
  line.addColorStop(1, `hsla(${accent},100%,75%,0.9)`);
  ctx.strokeStyle = line;
  ctx.lineWidth = 2.6;
  ctx.shadowColor = `hsla(${hue},100%,65%,0.95)`;
  ctx.shadowBlur = 16 + glowBoost * 14;
  ctx.stroke();
  ctx.shadowBlur = 0;

  return { cx, cy, amp };
}

// Small glowing sphere floating above the wave (the Siri orb).
function drawOrb(cx, cy, r) {
  const g = ctx.createRadialGradient(cx - r * 0.3, cy - r * 0.35, r * 0.1, cx, cy, r);
  g.addColorStop(0, `hsla(${accent},100%,94%,0.95)`);
  g.addColorStop(0.45, `hsla(${hue},${sat}%,65%,0.9)`);
  g.addColorStop(1, `hsla(${hue},${sat}%,45%,0.8)`);
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fillStyle = g;
  ctx.shadowColor = `hsla(${hue},${sat}%,60%,0.9)`;
  ctx.shadowBlur = 22;
  ctx.fill();
  ctx.shadowBlur = 0;
}

function drawListening() {
  ctx.clearRect(0, 0, innerWidth, innerHeight);
  const { cx, cy, amp } = drawWave(innerHeight * 0.20, 0.4);
  drawOrb(cx, cy - amp - innerHeight * 0.16, innerHeight * 0.11);
}

function drawThinking() {
  ctx.clearRect(0, 0, innerWidth, innerHeight);
  const { cx, cy, amp } = drawWave(innerHeight * 0.14 * (0.7 + 0.3 * Math.sin(t * 2.5)), 0.25);
  drawOrb(cx, cy - amp - innerHeight * 0.16, innerHeight * 0.10);
}

function drawSpeaking() {
  ctx.clearRect(0, 0, innerWidth, innerHeight);
  const { cx, cy, amp } = drawWave(innerHeight * (0.16 + energy * 0.16), energy);
  drawOrb(cx, cy - amp - innerHeight * 0.16, innerHeight * (0.10 + energy * 0.03));
}

function drawIdle() {
  ctx.clearRect(0, 0, innerWidth, innerHeight);
  const { cx, cy, amp } = drawWave(innerHeight * 0.10, 0.15);
  drawOrb(cx, cy - amp - innerHeight * 0.16, innerHeight * 0.10);
}

function draw() {
  if (state === "listening") drawListening();
  else if (state === "thinking") drawThinking();
  else if (state === "speaking") drawSpeaking();
  else drawIdle();
}

function loop(ts) {
  t = ts / 1000;
  energy += (targetEnergy - energy) * 0.3;
  draw();
  if (state === "idle" && Math.abs(energy - targetEnergy) < 0.012) {
    drawIdle();
    rafId = null;
    return;
  }
  rafId = requestAnimationFrame(loop);
}

function play() {
  if (rafId == null) rafId = requestAnimationFrame(loop);
}

function setState(s) {
  state = STYLE[s] ? s : "idle";
  const st = STYLE[state];
  hue = st.hue;
  accent = st.accent;
  sat = st.sat;
  if (state !== "speaking") targetEnergy = 0;
  hint.style.opacity = state === "idle" ? "1" : "0";
  play();
}

listen("state", (e) => setState(e.payload));
listen("energy", (e) => {
  targetEnergy = Math.max(0, Math.min(1, +e.payload || 0));
  if (state === "speaking") play();
});

function showCaption(who, text) {
  caption.textContent = `${who === "you" ? "you" : "marvis"}  ·  ${text}`;
  caption.style.opacity = "1";
  clearTimeout(captionTimer);
  captionTimer = setTimeout(() => (caption.style.opacity = "0"), 6000);
}
listen("transcript", (e) => showCaption("you", e.payload));
listen("reply", (e) => showCaption("marvis", e.payload));

function toggle() {
  if (state === "idle") invoke("start");
  else invoke("interrupt");
}
canvas.addEventListener("click", toggle);
addEventListener("keydown", (e) => {
  if (e.code === "Space") {
    e.preventDefault();
    toggle();
  }
});

size();
