import type { Scene } from "./scene";
import type { Drag } from "./gesture";
import { placements } from "./gesture";
import type { TimelineDto } from "../api";
import {
  RULER_HEIGHT,
  TRACK_HEIGHT,
  covered,
  topOf,
  xOf as xOfIn,
  type Placement,
  type Viewport,
} from "./geometry";

/** Pixels a frame has to be worth before the ruler marks every one. */
const FRAME_TICK = 4;

/** The hazard stripes' tile, in pixels. */
const HAZARD = 16;

/** How wide the playhead's head is, which is also how wide it is to grab. */
export const HEAD = 17;

export type Palette = ReturnType<typeof palette>;

/** Read once a frame: `getPropertyValue` is a lookup and two allocations. */
export function palette() {
  const css = getComputedStyle(document.documentElement);
  const color = (name: string) => css.getPropertyValue(name).trim();

  return {
    surface: color("--glow-bg-surface"),
    base: color("--glow-bg-base"),
    border: color("--glow-border-color"),
    muted: color("--glow-text-secondary"),
    label: color("--glow-text-primary"),
    clip: color("--glow-primary-soft-strong"),
    primary: color("--glow-primary"),
    danger: color("--glow-color-danger"),
    snap: color("--glow-color-warning"),
    playhead: color("--glow-color-danger"),
  };
}

/** Everything one paint needs. */
export type Frame = {
  ctx: CanvasRenderingContext2D;
  c: Palette;
  view: Viewport;
  width: number;
  height: number;
  fps: number;
  scene: Scene;
  timeline: TimelineDto | null;
  drag: Drag | null;
  /** Where the playhead is drawn, which is not always a whole frame. */
  head: number;
  playing: boolean;
};

export function paint(f: Frame) {
  const { ctx, c, width, height } = f;

  ctx.fillStyle = c.surface;
  ctx.fillRect(0, 0, width, height);

  ruler(f);
  clips(f);
  if (f.drag && !f.drag.idle && f.timeline) carried(f, f.drag, f.timeline);
  snapLine(f);
  playhead(f);
}

const xOf = (f: Frame, frame: number) => xOfIn(f.view, frame);

/** Seconds between ruler labels, chosen so they never crowd. */
function tickStep(f: Frame): number {
  const steps = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
  return (
    steps.find((s) => s * f.fps * f.view.zoom >= 90) ?? steps[steps.length - 1]
  );
}

function ruler(f: Frame) {
  const { ctx, c, width, view } = f;

  ctx.fillStyle = c.base;
  ctx.fillRect(0, 0, width, RULER_HEIGHT);
  ctx.strokeStyle = c.border;
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, RULER_HEIGHT - 0.5);
  ctx.lineTo(width, RULER_HEIGHT - 0.5);
  ctx.stroke();

  const step = tickStep(f) * f.fps;
  ctx.fillStyle = c.muted;
  ctx.font = "10px -apple-system, system-ui, sans-serif";
  ctx.textBaseline = "middle";

  for (
    let frame = Math.floor(view.scroll / step) * step;
    xOf(f, frame) < width;
    frame += step
  ) {
    const x = Math.round(xOf(f, frame)) + 0.5;
    if (x < 0) continue;

    ctx.beginPath();
    ctx.moveTo(x, RULER_HEIGHT - 6);
    ctx.lineTo(x, RULER_HEIGHT);
    ctx.stroke();

    const seconds = frame / f.fps;
    const label = `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
    ctx.fillText(label, x + 4, RULER_HEIGHT / 2);
  }

  // A mark per frame, once frames are far enough apart to read as marks
  // rather than as a smear.
  if (view.zoom < FRAME_TICK) return;

  ctx.save();
  ctx.globalAlpha = 0.35;
  ctx.beginPath();
  for (let frame = Math.ceil(view.scroll); xOf(f, frame) < width; frame++) {
    const at = Math.round(xOf(f, frame)) + 0.5;
    if (at < 0) continue;

    ctx.moveTo(at, RULER_HEIGHT - 3);
    ctx.lineTo(at, RULER_HEIGHT);
  }
  ctx.stroke();
  ctx.restore();
}

/** From the scene, not the document: a deleted clip is still fading out. */
function clips(f: Frame) {
  const { ctx, c, width, view } = f;

  for (const it of f.scene.clips.values()) {
    const x = xOf(f, it.position.value);
    const w = it.length.value * view.zoom;
    if (x + w < 0 || x > width) continue;

    const top = topOf(it.lane.value);

    // Carried clips fade where they still are: the move has not happened,
    // and a hole in the track would say it had.
    const dimmed = f.drag?.ids.has(it.clip.id) ? f.scene.fades.carry.value : 0;
    // Still edited, so its clips stay legible rather than ghosting out the
    // way a carried one does.
    const off = 1 - 0.45 * (1 - it.enabled.value);
    const alpha = it.present.value * (1 - 0.75 * dimmed) * off;

    ctx.globalAlpha = alpha;
    ctx.fillStyle = c.clip;
    roundRect(ctx, x, top, Math.max(w, 1), TRACK_HEIGHT, 4);
    ctx.fill();

    // An outline, not a fill: a clip can be two pixels wide.
    if (it.selected.value > 0.01) {
      ctx.globalAlpha = alpha * it.selected.value;
      ctx.strokeStyle = c.primary;
      ctx.lineWidth = 2;
      roundRect(ctx, x + 1, top + 1, Math.max(w - 2, 1), TRACK_HEIGHT - 2, 3);
      ctx.stroke();
      ctx.globalAlpha = alpha;
    }

    if (w > 40) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(x, top, w - 6, TRACK_HEIGHT);
      ctx.clip();
      ctx.fillStyle = c.label;
      ctx.fillText(it.clip.name, x + 6, top + 12);
      ctx.restore();
    }

    if (it.hovered.value > 0.01)
      handles(f, x, w, top, it.hovered.value * alpha);
  }

  ctx.globalAlpha = 1;
}

/** The bars at a hovered clip's ends, which drag its in- and out-points. */
function handles(f: Frame, x: number, w: number, top: number, fade: number) {
  // Narrower than the grab zone: a bar wider than its target is a lie.
  const bar = 3;
  if (w < bar * 4) return;

  const { ctx } = f;
  ctx.save();
  ctx.globalAlpha = fade;
  ctx.fillStyle = f.c.primary;
  for (const at of [x + 2, x + w - 2 - bar]) {
    roundRect(ctx, at, top + 6, bar, TRACK_HEIGHT - 12, bar / 2);
    ctx.fill();
  }
  ctx.restore();
}

/** Every carried clip where it would land, and any lane that isn't there. */
function carried(f: Frame, drag: Drag, document: TimelineDto) {
  const { ctx, c, width, view } = f;
  const fade = f.scene.fades.carry.value;
  const landed = placements(drag);

  // Stripes, not a wash: a solid red block reads as another clip.
  const eaten = covered(document, landed);
  ctx.save();
  ctx.globalAlpha = 0.18 * fade;
  ctx.fillStyle = c.danger;
  for (const span of eaten) {
    clipRect(f, span);
    ctx.fill();
  }

  const stripes = hazard(f, c.danger);
  if (stripes) {
    ctx.globalAlpha = 0.85 * fade;
    ctx.fillStyle = stripes;
    for (const span of eaten) {
      clipRect(f, span);
      ctx.fill();
    }
  }
  ctx.restore();

  ctx.save();
  ctx.globalAlpha = 0.9 * fade;
  ctx.fillStyle = c.clip;
  ctx.strokeStyle = c.primary;
  ctx.lineWidth = 1;

  for (const clip of landed) {
    const x = xOf(f, clip.position);
    if (x + clip.length * view.zoom < 0 || x > width) continue;

    // A new lane has nothing drawn under it yet.
    if (clip.track >= document.tracks.length) {
      ctx.save();
      ctx.strokeStyle = c.border;
      ctx.setLineDash([4, 4]);
      ctx.strokeRect(0.5, topOf(clip.track) + 0.5, width - 1, TRACK_HEIGHT - 1);
      ctx.restore();
    }

    clipRect(f, clip);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}

/** Full height: the edge it caught on may belong to another track. */
function snapLine(f: Frame) {
  const fade = f.scene.fades.snap.value;
  if (fade <= 0.01) return;

  const { ctx } = f;
  const at = Math.round(xOf(f, f.scene.snapAt)) + 0.5;

  ctx.save();
  ctx.globalAlpha = 0.4 * fade;
  ctx.strokeStyle = f.c.snap;
  ctx.setLineDash([3, 3]);
  ctx.beginPath();
  ctx.moveTo(at, 0);
  ctx.lineTo(at, f.height);
  ctx.stroke();
  ctx.restore();
}

/** A head to take hold of in the ruler, and a line to read against below it. */
function playhead(f: Frame) {
  const { ctx, c } = f;

  // Not pixel-snapped while playing: half-pixel steps are the point.
  const drawn = xOf(f, f.head);
  const x = f.playing ? drawn : Math.round(drawn) + 0.5;
  if (x < 0 || x > f.width) return;

  // Inset, then stroked back out to size: the pen is what rounds it.
  const round = 3;
  const half = HEAD / 2 - round;
  const top = 3 + round;
  const shoulder = RULER_HEIGHT - 10;
  const point = RULER_HEIGHT + 2 - round;

  ctx.save();
  ctx.fillStyle = c.playhead;
  ctx.strokeStyle = c.playhead;
  ctx.lineJoin = "round";
  ctx.lineWidth = round * 2;
  ctx.beginPath();
  ctx.moveTo(x - half, top);
  ctx.lineTo(x + half, top);
  ctx.lineTo(x + half, shoulder);
  ctx.lineTo(x, point);
  ctx.lineTo(x - half, shoulder);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(x, point + round);
  ctx.lineTo(x, f.height);
  ctx.stroke();
  ctx.restore();
}

/** A tile, so stripes line up across spans instead of restarting in each. */
let stripes: { pattern: CanvasPattern; of: string } | null = null;

function hazard(f: Frame, color: string): CanvasPattern | null {
  if (!stripes || stripes.of !== color) {
    const tile = window.document.createElement("canvas");
    tile.width = HAZARD;
    tile.height = HAZARD;

    const paint = tile.getContext("2d");
    if (!paint) return null;

    paint.strokeStyle = color;
    paint.lineWidth = 5;
    paint.beginPath();
    // `y = x + c`, a family unchanged by a shift of one tile either way —
    // which is what carries the stripes across the tile's edges.
    for (const c of [-HAZARD, 0, HAZARD]) {
      paint.moveTo(-HAZARD, -HAZARD + c);
      paint.lineTo(HAZARD * 2, HAZARD * 2 + c);
    }
    paint.stroke();

    const pattern = f.ctx.createPattern(tile, "repeat");
    if (!pattern) return null;

    stripes = { pattern, of: color };
  }

  // Anchored to frame zero, so stripes stay on the frames they describe.
  if (typeof stripes.pattern.setTransform === "function") {
    stripes.pattern.setTransform(
      new DOMMatrix().translateSelf(xOf(f, 0) % HAZARD, 0),
    );
  }

  return stripes.pattern;
}

function clipRect(f: Frame, clip: Placement) {
  roundRect(
    f.ctx,
    xOf(f, clip.position),
    topOf(clip.track),
    Math.max(clip.length * f.view.zoom, 1),
    TRACK_HEIGHT,
    4,
  );
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, Math.min(r, w / 2, h / 2));
}
