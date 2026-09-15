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

import type { ClipDto, TimelineDto, TrackDto } from './schema'

export type Clip = ClipDto
export type Track = TrackDto
export type Timeline = TimelineDto

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

/**
 * A clip to leave out of a calculation, or several — a group being dragged
 * must not snap to itself, nor count itself as being in its own way.
 */
export type Ignored = number | ReadonlySet<number>

const ignored = (ignore: Ignored, id: number) =>
  typeof ignore === 'number' ? ignore === id : ignore.has(id)

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

/** Every frame a gesture could land flush against. */
function edgesOf(timeline: Timeline, playhead: number, ignore: Ignored): number[] {
  // Every track, not just the one being dragged in: clips are cut against
  // each other across lanes as much as along them, and an edge that lines up
  // with the track above is exactly the one worth catching.
  const edges = [0, playhead]

  for (const lane of timeline.tracks) {
    for (const clip of lane.clips) {
      if (ignored(ignore, clip.id)) continue
      edges.push(clip.position, end(clip))
    }
  }

  return edges.sort((a, b) => a - b)
}

/** What a snap found: how far to pull, and the edge it would land on. */
export type Snap = { pull: number; at: number | null }

/**
 * How far to pull a gesture so that one of the edges it carries lands flush
 * on something that is staying put.
 *
 * Every carried edge is a candidate, not just the grabbed clip's: with three
 * clips selected, the one whose end meets the next clip is usually the one
 * the eye is on, and a drag that snapped only by the clip under the pointer
 * would slide the other two through their own alignments. The nearest match
 * across the group wins, and `at` is what it caught — which is what the
 * dashed line on the canvas is drawn from.
 */
export function snapDelta(
  timeline: Timeline, playhead: number, v: Viewport,
  moving: number[], ignore: Ignored,
): Snap {
  const tolerance = Math.max(SNAP / v.zoom, 0.5)
  const edges = edgesOf(timeline, playhead, ignore)
  let best: Snap = { pull: 0, at: null }

  for (const frame of moving) {
    // Sorted, so the search can start at the neighbours either side of this
    // edge and stop as soon as it is out of reach — a document of hundreds
    // of clips is searched rather than swept.
    for (let i = nearest(edges, frame); i < edges.length; i++) {
      const pull = edges[i] - frame
      if (pull > tolerance) break
      if (Math.abs(pull) > tolerance) continue
      if (best.at === null || Math.abs(pull) < Math.abs(best.pull)) {
        best = { pull, at: edges[i] }
      }
    }
  }

  return best
}

/** The first index in a sorted list that could be within reach of `frame`. */
function nearest(edges: number[], frame: number): number {
  let low = 0
  let high = edges.length

  while (low < high) {
    const mid = (low + high) >> 1
    if (edges[mid] < frame) low = mid + 1
    else high = mid
  }

  // One back, so an edge just before `frame` is still considered.
  return Math.max(0, low - 1)
}

/** Pull one frame onto a nearby edge, when there is one within reach. */
export function snap(
  timeline: Timeline, playhead: number, v: Viewport, frame: number, ignore: Ignored,
): number {
  return frame + snapDelta(timeline, playhead, v, [frame], ignore).pull
}

/**
 * Whether a clip of `length` fits at `position` on `track`.
 *
 * The same question `Timeline::apply` asks before it allows a move — asked
 * here only so a drag can say no while it is still a drag, rather than
 * bouncing off the engine after the pointer is up.
 */
export function hasRoom(
  timeline: Timeline, track: number, position: number, length: number, ignore: Ignored,
): boolean {
  const lane = timeline.tracks[track]
  if (!lane) return true

  return !lane.clips.some(
    (clip) =>
      !ignored(ignore, clip.id) && position < end(clip) && clip.position < position + length,
  )
}

/** Where a clip would sit once a drag lands. */
export type Placement = { id: number; track: number; position: number; length: number }

/**
 * The spans a drop would take off the clips it lands on.
 *
 * Dropping a clip over another shortens the one underneath rather than being
 * refused, so this is what a drag can show before it happens: the frames
 * that would stop being played, over the clips that would lose them.
 */
export function covered(timeline: Timeline, group: Placement[]): Placement[] {
  const moving = new Set(group.map((p) => p.id))
  const eaten: Placement[] = []

  for (const p of group) {
    for (const clip of timeline.tracks[p.track]?.clips ?? []) {
      if (moving.has(clip.id)) continue

      const from = Math.max(clip.position, p.position)
      const to = Math.min(end(clip), p.position + p.length)
      if (from >= to) continue

      eaten.push({ id: clip.id, track: p.track, position: from, length: to - from })
    }
  }

  return eaten
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
  const spare = Math.max(0, clip.source_length - clip.source_start)
  const [before, after] = neighbours(timeline, track, id)

  return edge === 'in'
    ? [Math.max(before, Math.max(0, clip.position - clip.source_start)), anchor - 1]
    : [anchor + 1, Math.min(after, clip.position + spare)]
}
