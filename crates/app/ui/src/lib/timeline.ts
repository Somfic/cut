// Where the document lands on screen, and what the pointer is over.
//
// Feedback only: the engine re-checks every edit in `Timeline::apply`, so a
// wrong answer here makes an edit bounce rather than corrupt anything. It
// lives in the front end because a drag needs it at pointer rate.

export const RULER_HEIGHT = 22
export const TRACK_HEIGHT = 54
export const TRACK_GAP = 6

/** How near a clip's end the pointer must be to trim it, in pixels. */
export const EDGE_GRAB = 6
/** How near an edge a dragged frame is pulled onto it, in pixels. */
export const SNAP = 8

export const MIN_ZOOM = 0.05
export const MAX_ZOOM = 40

export type Edge = 'in' | 'out'

export type Clip = {
  id: number
  position: number
  length: number
  sourceStart: number
  /** Frames the source has in total, which is how far a trim can reach. */
  sourceLength: number
  name: string
}

export type Track = { clips: Clip[] }
export type Timeline = { tracks: Track[]; length: number }

/** Which part of the document is on screen, and how big it is drawn. */
export type Viewport = { scroll: number; zoom: number }

export const xOf = (v: Viewport, frame: number) => (frame - v.scroll) * v.zoom
export const frameAt = (v: Viewport, x: number) =>
  Math.max(0, Math.floor(v.scroll + x / v.zoom))

export const topOf = (track: number) =>
  RULER_HEIGHT + track * (TRACK_HEIGHT + TRACK_GAP) + TRACK_GAP

/** The lane a pixel row is in, or null for the ruler and the gaps between. */
export function trackAt(y: number): number | null {
  if (y < RULER_HEIGHT) return null

  const track = Math.max(0, Math.floor((y - RULER_HEIGHT) / (TRACK_HEIGHT + TRACK_GAP)))
  return y >= topOf(track) ? track : null
}

export const end = (clip: Clip) => clip.position + clip.length

export const clipAt = (track: Track, frame: number) =>
  track.clips.find((clip) => frame >= clip.position && frame < end(clip))

export function locate(timeline: Timeline, id: number) {
  for (const [track, lane] of timeline.tracks.entries()) {
    const clip = lane.clips.find((c) => c.id === id)
    if (clip) return { track, clip }
  }
  return null
}

export type Hit = {
  clip: number
  track: number
  position: number
  length: number
  /** For working out where in the clip it was picked up. */
  frame: number
  edge: Edge | null
}

export function hit(
  timeline: Timeline, v: Viewport, x: number, y: number,
): Hit | null {
  const track = trackAt(y)
  if (track === null) return null

  const lane = timeline.tracks[track]
  if (!lane) return null

  const frame = frameAt(v, x)
  const clip = clipAt(lane, frame)
  if (!clip) return null

  const zone = Math.min(EDGE_GRAB, (clip.length * v.zoom) / 3)
  const edge: Edge | null =
    Math.abs(x - xOf(v, clip.position)) <= zone ? 'in'
    : Math.abs(xOf(v, end(clip)) - x) <= zone ? 'out'
    : null

  return { clip: clip.id, track, position: clip.position, length: clip.length, frame, edge }
}

/**
 * Pull `frame` onto a nearby edge — another clip's end, the playhead, or the
 * start of the document — when it is within `SNAP` pixels.
 */
export function snap(
  timeline: Timeline, playhead: number, v: Viewport,
  track: number, frame: number, ignore: number,
): number {
  const tolerance = Math.max(SNAP / v.zoom, 0.5)

  const candidates = (timeline.tracks[track]?.clips ?? [])
    .filter((clip) => clip.id !== ignore)
    .flatMap((clip) => [clip.position, end(clip)])
    .concat(0, playhead)
    .filter((edge) => Math.abs(edge - frame) <= tolerance)

  if (candidates.length === 0) return frame

  return candidates.reduce((best, edge) =>
    Math.abs(edge - frame) < Math.abs(best - frame) ? edge : best)
}

/** Either end of a carried clip landing flush is worth the same. */
export function snapCarried(
  timeline: Timeline, playhead: number, v: Viewport,
  track: number, position: number, length: number, clip: number,
): number {
  const head = snap(timeline, playhead, v, track, position, clip)
  if (head !== position) return head

  const tail = snap(timeline, playhead, v, track, position + length, clip)
  if (tail !== position + length) return Math.max(0, tail - length)

  return position
}

/** Where the clips either side leave off — the walls a trim runs into. */
export function neighbours(
  timeline: Timeline, track: number, id: number,
): [number, number] {
  const clips = timeline.tracks[track]?.clips
  const index = clips?.findIndex((clip) => clip.id === id) ?? -1
  if (!clips || index < 0) return [0, Number.MAX_SAFE_INTEGER]

  return [
    index > 0 ? end(clips[index - 1]) : 0,
    index + 1 < clips.length ? clips[index + 1].position : Number.MAX_SAFE_INTEGER,
  ]
}

/**
 * How far a trim can go: not past the end that isn't moving, not past what
 * the source has, and not into the clip next door.
 */
export function trimRange(
  timeline: Timeline, track: number, id: number, edge: Edge, anchor: number,
): [number, number] | null {
  const found = locate(timeline, id)
  if (!found) return null

  const { clip } = found
  const spare = Math.max(0, clip.sourceLength - clip.sourceStart)
  const [before, after] = neighbours(timeline, track, id)

  return edge === 'in'
    ? [Math.max(before, Math.max(0, clip.position - clip.sourceStart)), anchor - 1]
    : [anchor + 1, Math.min(after, clip.position + spare)]
}
