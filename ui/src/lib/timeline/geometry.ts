// Where the document lands on screen, and what the pointer is over. Feedback
// only: the engine re-checks every edit.

export const RULER_HEIGHT = 26;
export const TRACK_HEIGHT = 54;
export const TRACK_GAP = 6;

/** How near a clip's end the pointer must be to trim it, in pixels. */
export const EDGE_GRAB = 6;
/** How near an edge a dragged frame is pulled onto it, in pixels. */
export const SNAP = 8;

export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 40;

export type Edge = "in" | "out";

import type { ClipDto, TimelineDto, TrackDto } from "../schema";

export type Clip = ClipDto;
export type Track = TrackDto;
export type Timeline = TimelineDto;

/** Which part of the document is on screen, and how big it is drawn. */
export type Viewport = { scroll: number; zoom: number };

export const xOf = (v: Viewport, frame: number) => (frame - v.scroll) * v.zoom;
export const frameAt = (v: Viewport, x: number) =>
  Math.max(0, Math.floor(v.scroll + x / v.zoom));

export const topOf = (track: number) =>
  RULER_HEIGHT + track * (TRACK_HEIGHT + TRACK_GAP) + TRACK_GAP;

/** The lane a pixel row is in, or null for the ruler and the gaps between. */
export function trackAt(y: number): number | null {
  if (y < RULER_HEIGHT) return null;

  const track = Math.max(
    0,
    Math.floor((y - RULER_HEIGHT) / (TRACK_HEIGHT + TRACK_GAP)),
  );
  return y >= topOf(track) ? track : null;
}

export type Span = { position: number; length: number };

export const end = (span: Span) => span.position + span.length;

export const covers = (span: Span, frame: number) =>
  frame >= span.position && frame < end(span);

export const overlaps = (a: Span, b: Span) =>
  a.position < end(b) && b.position < end(a);

export const clipAt = (track: Track, frame: number) =>
  track.clips.find((clip) => covers(clip, frame));

export function locate(timeline: Timeline, id: number) {
  for (const [track, lane] of timeline.tracks.entries()) {
    const clip = lane.clips.find((c) => c.id === id);
    if (clip) return { track, clip };
  }
  return null;
}

export type Hit = {
  clip: number;
  track: number;
  position: number;
  length: number;
  /** For working out where in the clip it was picked up. */
  frame: number;
  edge: Edge | null;
};

export function hit(
  timeline: Timeline,
  v: Viewport,
  x: number,
  y: number,
): Hit | null {
  const track = trackAt(y);
  if (track === null) return null;

  const lane = timeline.tracks[track];
  if (!lane) return null;

  const frame = frameAt(v, x);
  const clip = clipAt(lane, frame);
  if (!clip) return null;

  const zone = Math.min(EDGE_GRAB, (clip.length * v.zoom) / 3);
  const edge: Edge | null =
    Math.abs(x - xOf(v, clip.position)) <= zone
      ? "in"
      : Math.abs(xOf(v, end(clip)) - x) <= zone
        ? "out"
        : null;

  return {
    clip: clip.id,
    track,
    position: clip.position,
    length: clip.length,
    frame,
    edge,
  };
}

/**
 * Every frame a gesture could land flush against, sorted, from every track.
 * Built once per drag: at pointer rate it was the most expensive thing one
 * did.
 */
export function snapCandidates(
  timeline: Timeline,
  ignore: ReadonlySet<number>,
): number[] {
  const edges = [0];

  for (const lane of timeline.tracks) {
    for (const clip of lane.clips) {
      if (ignore.has(clip.id)) continue;
      edges.push(clip.position, end(clip));
    }
  }

  return edges.sort((a, b) => a - b);
}

/** What a snap found: how far to pull, and the edge it would land on. */
export type Snap = { pull: number; at: number | null };

/**
 * How far to pull a gesture so an edge it carries lands flush on one staying
 * put. Every carried edge counts: snapping by the clip under the pointer
 * slides the rest through their own alignments.
 */
export function snapDelta(
  candidates: number[],
  playhead: number,
  v: Viewport,
  moving: number[],
): Snap {
  const tolerance = Math.max(SNAP / v.zoom, 0.5);
  let best: Snap = { pull: 0, at: null };

  const offer = (at: number, frame: number) => {
    const pull = at - frame;
    if (Math.abs(pull) > tolerance) return;
    if (best.at === null || Math.abs(pull) < Math.abs(best.pull))
      best = { pull, at };
  };

  for (const frame of moving) {
    // The one candidate that moves during a drag, so it is offered live.
    offer(playhead, frame);

    // Sorted, so this starts at the nearest edge and stops out of reach.
    for (let i = nearest(candidates, frame); i < candidates.length; i++) {
      if (candidates[i] - frame > tolerance) break;
      offer(candidates[i], frame);
    }
  }

  return best;
}

/** The first index in a sorted list that could be within reach of `frame`. */
function nearest(edges: number[], frame: number): number {
  let low = 0;
  let high = edges.length;

  while (low < high) {
    const mid = (low + high) >> 1;
    if (edges[mid] < frame) low = mid + 1;
    else high = mid;
  }

  // One back, so an edge just before `frame` is still considered.
  return Math.max(0, low - 1);
}

/** Where a clip sits, or would sit once a drag lands. */
export type Placement = Span & { id: number; track: number };

/** The spans a drop would take off the clips it lands on. */
export function covered(timeline: Timeline, group: Placement[]): Placement[] {
  const moving = new Set(group.map((p) => p.id));
  const eaten: Placement[] = [];

  for (const p of group) {
    for (const clip of timeline.tracks[p.track]?.clips ?? []) {
      // Sorted, so once a clip starts past the span nothing else can overlap.
      if (clip.position >= end(p)) break;
      if (moving.has(clip.id) || !overlaps(clip, p)) continue;

      const from = Math.max(clip.position, p.position);
      eaten.push({
        id: clip.id,
        track: p.track,
        position: from,
        length: Math.min(end(clip), end(p)) - from,
      });
    }
  }

  return eaten;
}

/**
 * How far a trim can go: its own end, and what the source has. Neighbours are
 * not walls — growing into one shortens it — or a handle could not move at
 * all in a document with no gaps.
 */
export function trimRange(
  timeline: Timeline,
  track: number,
  id: number,
  edge: Edge,
  anchor: number,
): [number, number] | null {
  const found = locate(timeline, id);
  if (!found) return null;

  const { clip } = found;
  const spare = Math.max(0, clip.source_length - clip.source_start);

  return edge === "in"
    ? [Math.max(0, clip.position - clip.source_start), anchor - 1]
    : [anchor + 1, clip.position + spare];
}

export function timecode(
  frame: number,
  fps: number,
): { minutes: number; seconds: number; subseconds: number } {
  const total = Math.max(0, frame);
  const subseconds = Math.floor(total % fps);
  const seconds = Math.floor(total / fps);

  return {
    minutes: Math.floor(seconds / 60),
    seconds: seconds % 60,
    subseconds,
  };
}
