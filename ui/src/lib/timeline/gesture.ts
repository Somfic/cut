import type { TimelineDto } from "../api";
import {
  end,
  snapCandidates,
  snapDelta,
  trimRange,
  type Edge,
  type Hit,
  type Placement,
  type Viewport,
} from "./geometry";

/** What a drag may not go past: frames, and a count of lanes. */
type Limits = { frames: [number, number]; tracks: [number, number] };

/**
 * A gesture over clips. Everything fixed for its life is worked out at the
 * start, because the rest runs at pointer rate.
 */
export type Drag = {
  group: Placement[];
  held: Placement;
  ids: ReadonlySet<number>;
  /** Which handle is being pulled, or null when the clips are carried. */
  edge: Edge | null;
  /** Frames between the grabbed clip's start and where it was picked up. */
  grab: number;
  limits: Limits;
  /** Edges staying put, sorted, to snap against. */
  candidates: number[];
  frames: number;
  tracks: number;
  /** The frame a carried edge caught on, for the line drawn down it. */
  snappedAt: number | null;
  /** Cleared on the first real movement; until then this is still a click. */
  idle: boolean;
};

export const clamp = (value: number, [low, high]: [number, number]) =>
  Math.min(Math.max(value, low), high);

export const edgeOf = (clip: Placement, edge: Edge) =>
  edge === "in" ? clip.position : end(clip);

/** The named clips as they stand, which is what every gesture starts from. */
export function placementsOf(
  timeline: TimelineDto | null,
  ids: ReadonlySet<number>,
): Placement[] {
  const group: Placement[] = [];

  timeline?.tracks.forEach((lane, track) => {
    for (const clip of lane.clips) {
      if (!ids.has(clip.id)) continue;
      group.push({
        id: clip.id,
        track,
        position: clip.position,
        length: clip.length,
      });
    }
  });

  return group;
}

/** The group is copied as it stands, so offsets measure from the start. */
export function start(
  timeline: TimelineDto,
  grabbed: Hit,
  selected: ReadonlySet<number>,
): Drag {
  const ids = selected.has(grabbed.clip)
    ? new Set(selected)
    : new Set([grabbed.clip]);
  const group = placementsOf(timeline, ids);

  return {
    group,
    held: group.find((clip) => clip.id === grabbed.clip)!,
    ids,
    edge: grabbed.edge,
    grab: grabbed.frame - grabbed.position,
    limits: grabbed.edge
      ? trimLimits(timeline, group, grabbed.edge)
      : moveLimits(timeline, group),
    candidates: snapCandidates(timeline, ids),
    frames: 0,
    tracks: 0,
    snappedAt: null,
    idle: true,
  };
}

/** Where the pointer is, in the terms a gesture works in. */
export type At = { frame: number; lane: number | null };

/** Where the gesture would leave things, given where the pointer is. */
export function carry(d: Drag, at: At, playhead: number, view: Viewport): Drag {
  const tracks = d.edge
    ? 0
    : clamp((at.lane ?? d.held.track) - d.held.track, d.limits.tracks);
  const pointer = d.edge
    ? at.frame - edgeOf(d.held, d.edge)
    : at.frame - d.grab - d.held.position;

  // Every carried edge is offered: the clip meeting its new neighbour is
  // rarely the one under the pointer.
  const edges = d.group.flatMap((clip) =>
    d.edge
      ? [edgeOf(clip, d.edge) + pointer]
      : [clip.position + pointer, end(clip) + pointer],
  );

  const caught = snapDelta(d.candidates, playhead, view, edges);
  const pulled = pointer + caught.pull;
  const frames = clamp(pulled, d.limits.frames);

  return {
    ...d,
    frames,
    tracks,
    // Only if the snap survived the clamp, or the line describes nothing.
    snappedAt: frames === pulled ? caught.at : null,
    idle: d.idle && frames === 0 && tracks === 0,
  };
}

/** Where the drag would leave every clip it carries. */
export function placements(d: Drag): Placement[] {
  return d.group.map((clip) => ({
    id: clip.id,
    track: clip.track + d.tracks,
    position: clip.position + (d.edge === "out" ? 0 : d.frames),
    length:
      d.edge === "in"
        ? clip.length - d.frames
        : d.edge === "out"
          ? clip.length + d.frames
          : clip.length,
  }));
}

/** Not before the first frame, and not off either end of the tracks. */
export function moveLimits(
  timeline: TimelineDto | null,
  group: Placement[],
): Limits {
  const tracks = group.map((clip) => clip.track);

  return {
    frames: [-Math.min(...group.map((clip) => clip.position)), Infinity],
    tracks: [
      -Math.min(...tracks),
      (timeline?.tracks.length ?? 0) - Math.max(...tracks),
    ],
  };
}

/** The tightest of what each clip allows: the engine refuses the group. */
function trimLimits(
  timeline: TimelineDto,
  group: Placement[],
  edge: Edge,
): Limits {
  let low = -Infinity;
  let high = Infinity;

  for (const clip of group) {
    const from = edgeOf(clip, edge);
    const anchor = edge === "in" ? end(clip) : clip.position;
    const range = trimRange(timeline, clip.track, clip.id, edge, anchor);

    // A clip with nowhere to go pins the whole group where it is.
    if (!range) return { frames: [0, 0], tracks: [0, 0] };

    low = Math.max(low, range[0] - from);
    high = Math.min(high, range[1] - from);
  }

  return { frames: [Math.min(low, 0), Math.max(high, 0)], tracks: [0, 0] };
}
