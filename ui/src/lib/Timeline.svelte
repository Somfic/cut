<script lang="ts">
  import type { ClipDto, TimelineDto } from "./api.svelte";
  import api, { attempt } from "./api.svelte";
  import {
    MAX_ZOOM,
    MIN_ZOOM,
    RULER_HEIGHT,
    TRACK_GAP,
    TRACK_HEIGHT,
    frameAt as frameAtIn,
    covered,
    hit,
    snapCandidates,
    snapDelta,
    topOf,
    trackAt,
    trimRange,
    xOf as xOfIn,
    type Edge,
    type Hit,
    type Placement,
    type Viewport,
  } from "./timeline";
  import { selection } from "./selection.svelte";
  import { viewport } from "./viewport.svelte";
  import { Eased, stillness } from "./animate";

  let {
    timeline,
    playhead,
    exact,
    playing,
    fps,
    onScrub,
  }: {
    timeline: TimelineDto | null;
    playhead: number;
    /** The playhead between frames, for drawing it while playback runs. */
    exact: number;
    playing: boolean;
    fps: number;
    /** Move the playhead here now, rather than when the engine answers. */
    onScrub?: (frame: number) => void;
  } = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  // Pixels per frame, and the frame at the left edge. Shared, because undo
  // puts them back where they were when the edit was made.
  const zoom = $derived(viewport.zoom);
  const scroll = $derived(viewport.scroll);
  let width = $state(0);
  let height = $state(0);
  let fitted = false;

  /**
   * A gesture over clips, and what it would do if the pointer went up now.
   *
   * Nothing reaches the engine until then: the gesture is one edit, and one
   * per pointer event would fill the undo stack with the path the mouse took
   * rather than the move the user made.
   *
   * Everything fixed for the life of the drag is worked out when it starts —
   * the group, the snap candidates, how far a trim may go — because the rest
   * runs at pointer rate.
   */
  type Drag = {
    /** The clips as they were when it started; `held` is the one grabbed. */
    group: Placement[];
    held: Placement;
    ids: ReadonlySet<number>;
    /** Which handle is being pulled, or null when the clips are carried. */
    edge: Edge | null;
    /** Frames between the grabbed clip's start and where it was picked up. */
    grab: number;
    /** Frames a gesture may not move past: the group's own limits. */
    limits: { frames: [number, number]; tracks: [number, number] };
    /** Edges that are staying put, sorted, for snapping against. */
    candidates: number[];
    /** What the whole group is offset by — the shape of the edit sent. */
    frames: number;
    tracks: number;
    /** The frame a carried edge caught on, for the line drawn down it. */
    snappedAt: number | null;
    /** Cleared on the first real movement; until then this is still a click. */
    idle: boolean;
  };

  /**
   * The ruler being dragged to slide the document under a still playhead.
   *
   * Held separately from `drag`: it changes nothing about the document, so
   * none of the room, snap or edit machinery below has anything to say
   * about it.
   */
  type Pan = { x: number; scroll: number; idle: boolean };

  let pan = $state<Pan | null>(null);
  /** Dragging the playhead's head, which seeks rather than moving the view. */
  let scrubbing = $state(false);
  let drag = $state<Drag | null>(null);
  /** The clip under the pointer, which is the one wearing handles. */
  let hover = $state<number | null>(null);
  let cursor = $state("default");

  /**
   * Put the playhead somewhere.
   *
   * Both halves: the engine, which has to decode from there, and the front
   * end's own clock, which is what is actually drawn — a scrub that waited
   * for the round trip would trail the pointer the whole way.
   */
  function seek(frame: number) {
    onScrub?.(frame);
    api.transport.seek(frame);
  }

  /** The view as the geometry helpers want it. */
  const view = (): Viewport => ({ scroll, zoom });
  const xOf = (frame: number) => xOfIn(view(), frame);
  const frameAt = (x: number) => frameAtIn(view(), x);

  /** Pixels a frame has to be worth before the ruler marks every one. */
  const FRAME_TICK_ZOOM = 4;

  /** The hazard stripes' tile, in pixels. */
  const HAZARD = 16;

  /** How wide the playhead's head is, which is also how wide it is to grab. */
  const HEAD = 17;

  /** Seconds between ruler labels, chosen so they never crowd. */
  function tickStep(): number {
    const steps = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
    return steps.find((s) => s * fps * zoom >= 90) ?? steps[steps.length - 1];
  }

  /**
   * The theme, read once a frame.
   *
   * `getPropertyValue` is a style lookup and a pair of string allocations, and
   * this used to run two or three times per clip — several hundred times a
   * frame, on a canvas that now redraws at pointer rate.
   */
  function palette() {
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
      // Red, the one colour a playhead is in every editor there has ever
      // been — and not the accent, which every clip is already wearing.
      playhead: color("--glow-color-danger"),
    };
  }

  type Palette = ReturnType<typeof palette>;

  /**
   * What is easing, and the frame loop that moves it.
   *
   * A canvas gets no CSS transitions, so every soft change on it — a border
   * arriving as a clip is selected, the snap line appearing — is a number
   * walking toward another number here, advanced once per frame while any of
   * them is still travelling.
   */
  const fades = {
    /** The dashed snap line, and the ghosts of a carry. */
    snap: new Eased(),
    carry: new Eased(),
    /** Where the playhead is drawn, which is not always where it is. */
    head: new Eased(0, 90),
  };

  /** How long a clip takes to travel to where an edit put it. */
  const GLIDE = 110;

  /**
   * A clip as it is being shown, which is not quite where the document says
   * it is: an edit moves a clip in one step, and this is what walks it there.
   *
   * It also outlives the clip. A delete, or a trim that swallowed a
   * neighbour, leaves an entry fading out with the last geometry it had —
   * which is why the paint below reads this rather than the document.
   */
  type Shown = {
    clip: ClipDto;
    track: number;
    position: Eased;
    length: Eased;
    lane: Eased;
    /** In the document, or on its way out of it. */
    present: Eased;
    selected: Eased;
    hovered: Eased;
  };

  const shown = new Map<number, Shown>();

  function show(clip: ClipDto, track: number, selected: boolean, hovered: boolean) {
    let it = shown.get(clip.id);

    if (!it) {
      // Born where it belongs. Only the presence fade starts from nothing: a
      // clip scrolling into view was already selected and already where it
      // is, and should not be seen becoming either.
      it = {
        clip,
        track,
        position: new Eased(clip.position, GLIDE),
        length: new Eased(clip.length, GLIDE),
        lane: new Eased(track, GLIDE),
        present: new Eased(0),
        selected: new Eased(+selected),
        hovered: new Eased(+hovered),
      };
      shown.set(clip.id, it);
    }

    it.clip = clip;
    it.track = track;
    it.position.to(clip.position);
    it.length.to(clip.length);
    it.lane.to(track);
    it.present.to(1);
    it.selected.to(+selected);
    it.hovered.to(+hovered);
  }

  /** Put a clip where a gesture just left it, with no journey to make. */
  function settle(placements: Placement[]) {
    for (const placement of placements) {
      const it = shown.get(placement.id);
      if (!it) continue;

      // The hand already carried it here; watching it fly the same route
      // again is the animation getting in the way of the edit.
      it.position.set(placement.position);
      it.length.set(placement.length);
      it.lane.set(placement.track);
    }
  }

  /** Where the snap line was last seen, so it has somewhere to fade out at. */
  let snapAt = 0;
  let painted = 0;
  let pending = 0;

  /**
   * Ask for a frame. Every redraw goes through here, so a burst of pointer
   * events in one frame paints once, and the easing loop cannot stack.
   */
  function schedule() {
    if (pending) return;
    pending = requestAnimationFrame(() => {
      pending = 0;
      draw();
    });
  }

  /** Move everything that is easing on. True while any of it still is. */
  function advance(present: Set<number>): boolean {
    const now = performance.now();
    // A tab that was in the background hands back a vast delta; capping it
    // means the first frame after it lands rather than leaps.
    const dt = stillness() ? 1e6 : Math.min(now - painted, 100);
    painted = now;

    let moving = [fades.snap, fades.carry, fades.head]
      .map((eased) => eased.advance(dt))
      .some(Boolean);

    for (const [id, it] of shown) {
      // Gone from the document: it fades out where it stood, and is dropped
      // once there is nothing left of it to draw.
      if (!present.has(id)) {
        it.present.to(0);
        if (it.present.value < 0.01) {
          shown.delete(id);
          continue;
        }
      }

      // Every one advanced, not the first that says yes: `||` short-circuits,
      // and a value left un-advanced is a value frozen for the frame.
      for (const eased of [
        it.position,
        it.length,
        it.lane,
        it.present,
        it.selected,
        it.hovered,
      ]) {
        moving = eased.advance(dt) || moving;
      }
    }

    return moving;
  }

  function draw() {
    const dpr = window.devicePixelRatio;
    const ctx = canvas?.getContext("2d");
    if (!ctx || width === 0) return;

    // Only on a real size change: assigning either one reallocates and clears
    // the backing store, which at pointer rate is a fresh buffer per event.
    const [pixelWidth, pixelHeight] = [
      Math.round(width * dpr),
      Math.round(height * dpr),
    ];
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const c = palette();
    // Snapshotted, so the per-clip test below is a plain set lookup rather
    // than a reactive read registering a dependency per selected id.
    const selected = new Set(selection.ids);

    // Aim everything that eases before advancing it: a target set during the
    // paint would otherwise go unnoticed until something else asked for a
    // frame, and the transition would never start.
    const present = new Set<number>();
    timeline?.tracks.forEach((lane, track) => {
      for (const clip of lane.clips) {
        present.add(clip.id);
        show(clip, track, selected.has(clip.id), clip.id === hover && !drag);
      }
    });

    // Eased only when the playhead jumps. Playing, it is drawn between frames
    // instead — see `at` — and easing would drag it behind the picture;
    // scrubbing, it has to be under the pointer and nowhere else.
    if (playing || scrubbing) fades.head.set(exact);
    else fades.head.to(playhead);

    fades.carry.to(drag && !drag.idle ? 1 : 0);
    fades.snap.to(drag?.snappedAt != null ? 1 : 0);
    if (drag?.snappedAt != null) snapAt = drag.snappedAt;

    const moving = advance(present);

    ctx.fillStyle = c.surface;
    ctx.fillRect(0, 0, width, height);

    // Ruler.
    ctx.fillStyle = c.base;
    ctx.fillRect(0, 0, width, RULER_HEIGHT);
    ctx.strokeStyle = c.border;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, RULER_HEIGHT - 0.5);
    ctx.lineTo(width, RULER_HEIGHT - 0.5);
    ctx.stroke();

    const step = tickStep() * fps;
    ctx.fillStyle = c.muted;
    ctx.font = "10px -apple-system, system-ui, sans-serif";
    ctx.textBaseline = "middle";

    for (
      let frame = Math.floor(scroll / step) * step;
      xOf(frame) < width;
      frame += step
    ) {
      const x = Math.round(xOf(frame)) + 0.5;
      if (x < 0) continue;

      ctx.beginPath();
      ctx.moveTo(x, RULER_HEIGHT - 6);
      ctx.lineTo(x, RULER_HEIGHT);
      ctx.stroke();

      const seconds = frame / fps;
      const label = `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
      ctx.fillText(label, x + 4, RULER_HEIGHT / 2);
    }

    // A mark per frame, once frames are far enough apart to read as marks
    // rather than as a smear. Below that they would say nothing except that
    // the ruler is grey.
    if (zoom >= FRAME_TICK_ZOOM) {
      ctx.save();
      ctx.globalAlpha = 0.35;
      ctx.beginPath();
      for (let frame = Math.ceil(scroll); xOf(frame) < width; frame++) {
        const at = Math.round(xOf(frame)) + 0.5;
        if (at < 0) continue;

        ctx.moveTo(at, RULER_HEIGHT - 3);
        ctx.lineTo(at, RULER_HEIGHT);
      }
      ctx.stroke();
      ctx.restore();
    }

    // Drawn from what is on screen rather than from the document: a clip an
    // edit took away is still here for as long as it takes to fade, and one
    // an edit moved is still on its way.
    for (const it of shown.values()) {
      const x = xOf(it.position.value);
      const w = it.length.value * zoom;
      if (x + w < 0 || x > width) continue;

      const top = topOf(it.lane.value);

      // The one being carried stays drawn where it still is, fading back: the
      // move has not happened yet, and a hole in the track would suggest
      // otherwise.
      const carried = drag?.ids.has(it.clip.id) ? fades.carry.value : 0;
      const alpha = it.present.value * (1 - 0.75 * carried);

      ctx.globalAlpha = alpha;
      ctx.fillStyle = c.clip;
      roundRect(ctx, x, top, Math.max(w, 1), TRACK_HEIGHT, 4);
      ctx.fill();

      // Selection reads as an outline rather than a different fill: at this
      // zoom a clip can be two pixels wide, where a fill change is invisible
      // and a border still lands.
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

      // Only on the clip under the pointer: on every clip they would read as
      // part of how a clip is drawn rather than as something to take hold of,
      // and at this zoom there can be hundreds on screen.
      if (it.hovered.value > 0.01) {
        drawHandles(ctx, c, x, w, top, it.hovered.value * alpha);
      }
    }

    ctx.globalAlpha = 1;

    if (drag && !drag.idle && timeline) {
      drawCarried(ctx, c, drag, timeline, fades.carry.value);
    }

    // The edge a drag caught on, down the full height so it can be read
    // against clips on every track — which is where it may have come from.
    if (fades.snap.value > 0.01) {
      const at = Math.round(xOf(snapAt)) + 0.5;
      ctx.save();
      ctx.globalAlpha = 0.4 * fades.snap.value;
      // Warning, not primary: the playhead is already a primary line, and
      // two of them a few pixels apart during a drag is unreadable.
      ctx.strokeStyle = c.snap;
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.moveTo(at, 0);
      ctx.lineTo(at, height);
      ctx.stroke();
      ctx.restore();
    }

    // Playhead, last so nothing covers it.
    drawPlayhead(ctx, c);

    // One more frame for whatever is still travelling.
    if (moving) schedule();
  }

  /**
   * The playhead: a head to take hold of, and a line to read against.
   *
   * The head sits in the ruler rather than over the clips, so the thing you
   * grab never covers the thing you are looking at — and its width is what
   * `HEAD` is for, since a press has to know whether it landed on it.
   */
  function drawPlayhead(ctx: CanvasRenderingContext2D, c: Palette) {
    // Not rounded to a pixel while playing: a half-pixel step is what the
    // sub-frame position is for, and snapping it back to whole pixels would
    // put the stepping straight back.
    const drawn = xOf(at());
    const x = playing ? drawn : Math.round(drawn) + 0.5;
    if (x < 0 || x > width) return;

    // The shape is drawn inset and then stroked back out to size, because
    // the stroke is what rounds it: a wider pen, softer corners.
    const round = 3;
    const half = HEAD / 2 - round;
    const top = 3 + round;
    const shoulder = RULER_HEIGHT - 10;
    // Past the ruler's edge, so the point is what meets the line rather than
    // the ruler's own border cutting across it.
    const point = RULER_HEIGHT + 2 - round;

    ctx.save();
    ctx.fillStyle = c.playhead;
    ctx.strokeStyle = c.playhead;

    // Stroked as well as filled, with round joins: the corners then come out
    // soft without an arc per corner.
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
    ctx.lineTo(x, height);
    ctx.stroke();
    ctx.restore();
  }

  /** The bars at a hovered clip's ends, which drag its in- and out-points. */
  function drawHandles(
    ctx: CanvasRenderingContext2D,
    c: Palette,
    x: number,
    w: number,
    top: number,
    fade: number,
  ) {
    // Narrower than the grab zone on purpose: a bar thinner than the target
    // it stands for is forgiving, one wider than it is a lie.
    const bar = 3;
    if (w < bar * 4) return;

    ctx.save();
    ctx.globalAlpha = fade;
    ctx.fillStyle = c.primary;
    for (const at of [x + 2, x + w - 2 - bar]) {
      roundRect(ctx, at, top + 6, bar, TRACK_HEIGHT - 12, bar / 2);
      ctx.fill();
    }

    ctx.restore();
  }

  /** Every carried clip where it would land, and any lane that isn't there. */
  function drawCarried(
    ctx: CanvasRenderingContext2D,
    c: Palette,
    d: Drag,
    document: TimelineDto,
    fade: number,
  ) {
    const landed = placements(d);
    const lanes = document.tracks.length;

    // What the drop would take off the clips underneath, drawn on them before
    // it happens. Hazard stripes rather than a flat wash: a solid red block
    // reads as another clip, and this is the opposite — frames on their way
    // out.
    ctx.save();
    const eaten = covered(document, landed);

    ctx.globalAlpha = 0.18 * fade;
    ctx.fillStyle = c.danger;
    for (const span of eaten) {
      clipRect(ctx, span);
      ctx.fill();
    }

    const stripes = hazard(ctx, c.danger);
    if (stripes) {
      ctx.globalAlpha = 0.85 * fade;
      ctx.fillStyle = stripes;
      for (const span of eaten) {
        clipRect(ctx, span);
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
      const x = xOf(clip.position);
      if (x + clip.length * zoom < 0 || x > width) continue;

      // Dropping past the last track starts a new one, so there is no lane
      // drawn underneath yet — an outline stands in for it.
      if (clip.track >= lanes) {
        ctx.save();
        ctx.strokeStyle = c.border;
        ctx.setLineDash([4, 4]);
        ctx.strokeRect(
          0.5,
          topOf(clip.track) + 0.5,
          width - 1,
          TRACK_HEIGHT - 1,
        );
        ctx.restore();
      }

      clipRect(ctx, clip);
      ctx.fill();
      ctx.stroke();
    }
    ctx.restore();
  }

  /**
   * Diagonal stripes, built once and kept.
   *
   * A tile plus `repeat` rather than clipped strokes per span: the stripes
   * then line up across neighbouring spans instead of each starting again at
   * its own left edge, and one fill draws however many frames are going.
   */
  let stripes: { pattern: CanvasPattern; of: string } | null = null;

  function hazard(ctx: CanvasRenderingContext2D, color: string): CanvasPattern | null {
    if (stripes?.of === color) return anchored(stripes.pattern);

    const size = HAZARD;
    const tile = window.document.createElement("canvas");
    tile.width = size;
    tile.height = size;

    const paint = tile.getContext("2d");
    if (!paint) return null;

    paint.strokeStyle = color;
    paint.lineWidth = 5;
    paint.beginPath();
    // `y = x + c` for three values of c, drawn well past the tile: the family
    // is unchanged by a shift of one tile in either direction, which is what
    // makes the stripes carry on across the edges.
    for (const c of [-size, 0, size]) {
      paint.moveTo(-size, -size + c);
      paint.lineTo(size * 2, size * 2 + c);
    }
    paint.stroke();

    const pattern = ctx.createPattern(tile, "repeat");
    if (!pattern) return null;

    stripes = { pattern, of: color };
    return anchored(pattern);
  }

  /**
   * Slide the pattern with the document, so the stripes stay on the frames
   * they describe instead of shimmering across them as the view scrolls.
   */
  function anchored(pattern: CanvasPattern): CanvasPattern {
    if (typeof pattern.setTransform !== "function") return pattern;

    // Frame zero's place on screen, which is where the tile's origin belongs.
    pattern.setTransform(new DOMMatrix().translateSelf(xOf(0) % HAZARD, 0));
    return pattern;
  }

  /** The box a clip occupies on screen. */
  function clipRect(ctx: CanvasRenderingContext2D, clip: Placement) {
    roundRect(
      ctx,
      xOf(clip.position),
      topOf(clip.track),
      Math.max(clip.length * zoom, 1),
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
    const radius = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, radius);
  }

  // An edit can take a clip away — a delete, or a split replacing one with
  // two — and a selection holding its id would point at nothing.
  $effect(() => {
    if (timeline && selection.size) {
      selection.keep(timeline.tracks.flatMap((t) => t.clips.map((c) => c.id)));
    }
  });

  // fit once, when the document's length and our width are both known
  $effect(() => {
    if (fitted || !timeline || timeline.length === 0 || width === 0) return;
    fit();
    fitted = true;
  });

  $effect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  /**
   * Follow the playhead: hold it in the middle and move the document past it.
   *
   * Driven by `exact`, so the scroll is as continuous as the playhead is —
   * stepping the view a frame at a time would undo the sub-frame position it
   * is being kept in step with.
   */
  $effect(() => {
    if (!viewport.following || width === 0) return;

    // Clamped like any other scroll, so the ends of the document are not
    // padded with nothing: the playhead leaves the middle there, which is
    // what every editor does and what tells you an end is near.
    viewport.to(clampScroll(at() - width / 2 / zoom), zoom);
  });

  // redraw whenever anything it reads changes
  $effect(() => {
    void [timeline, playhead, exact, playing, zoom, scroll, width, height, fps, drag, hover];
    schedule();
  });

  $effect(() => {
    const observer = new ResizeObserver(([entry]) => {
      width = entry.contentRect.width;
      height = entry.contentRect.height;
    });
    observer.observe(wrap);
    return () => observer.disconnect();
  });

  // zoom about a pixel column, keeping whatever is under it in place
  function zoomBy(factor: number, at: number) {
    const anchor = scroll + at / zoom;
    const next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * factor));

    viewport.to(clampScroll(anchor - at / next, next), next);
  }

  // fit the whole document, or one second if there is nothing in it
  function fit() {
    const frames = timeline?.length || fps;
    viewport.to(0, width / frames);
  }

  // never past the end of the document, never before the start
  function clampScroll(to: number, at = zoom): number {
    const frames = timeline?.length ?? 0;
    return Math.max(0, Math.min(to, Math.max(0, frames - width / at)));
  }

  // A trackpad's wheel is a stream of small, often fractional deltas and can
  // carry a horizontal axis; a mouse wheel arrives as discrete notches of
  // ±120 with nothing on deltaX. They get different meanings below, so the
  // kind has to be guessed — and remembered for a moment, since a fast flick
  // on a trackpad does produce the occasional round, axis-aligned event that
  // would otherwise read as a mouse mid-gesture.
  let trackpadUntil = 0;

  function isTrackpad(event: WheelEvent): boolean {
    const now = performance.now();
    const fine = Math.abs(event.deltaY) < 50 || !Number.isInteger(event.deltaY);

    if (event.deltaMode === 0 && (fine || event.deltaX !== 0)) {
      trackpadUntil = now + 500;
      return true;
    }

    return now < trackpadUntil;
  }

  function onWheel(event: WheelEvent) {
    event.preventDefault();

    // a trackpad pinch arrives as a wheel with `ctrlKey`; `alt` is the
    // equivalent for a mouse, which has no pinch and no horizontal axis
    if (event.ctrlKey || event.metaKey || event.altKey) {
      zoomBy(Math.exp(-event.deltaY / 200), event.offsetX);
      return;
    }

    // Two fingers sideways is the pan, so two fingers up and down is free to
    // be the zoom — the same mapping as the pinch above, and the gesture a
    // trackpad reaches for most. A mouse keeps its wheel for panning: it has
    // no horizontal axis, so zooming it would leave nothing to scroll with.
    const sideways = Math.abs(event.deltaX) > Math.abs(event.deltaY);
    if (!sideways && isTrackpad(event)) {
      zoomBy(Math.exp(-event.deltaY / 200), event.offsetX);
      return;
    }

    const delta = event.deltaX !== 0 ? event.deltaX : event.deltaY;
    viewport.following = false;
    viewport.to(clampScroll(scroll + delta / zoom), zoom);
  }

  // zoom from the keyboard, about the playhead rather than the pointer. */
  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape" && (drag || selection.size)) {
      drag = null;
      cursor = "default";
      fades.carry.set(0);
      selection.clear();
      event.preventDefault();
      return;
    }

    // Arrows nudge: the selection when there is one, the playhead otherwise.
    // Shift takes the bigger step, which for frames is a tenth of a second
    // rather than a round ten — a step in frames means nothing at 24fps.
    const nudge = NUDGES[event.key];
    if (nudge && !event.ctrlKey && !event.metaKey) {
      const [frames, tracks] = nudge;
      // Shift is the coarse step: a second, which is a unit that means
      // something, where "ten frames" means something different per project.
      const step = event.shiftKey ? Math.round(fps) : 1;

      // `repeat` is the browser telling us the key never came back up, which
      // is exactly the question the undo stack wants answered: everything a
      // held key does is one gesture, and undo should take back the hold.
      if (selection.size) shift(frames * step, tracks, event.repeat);
      else if (frames) seek(Math.max(0, playhead + frames * step));

      event.preventDefault();
      return;
    }

    if (!(event.ctrlKey || event.metaKey)) return;

    const at = xOf(playhead);
    const centre = at >= 0 && at <= width ? at : width / 2;

    switch (event.key) {
      case "=":
      case "+":
        zoomBy(1.4, centre);
        break;
      case "-":
        zoomBy(1 / 1.4, centre);
        break;
      case "0":
        fit();
        break;
      default:
        return;
    }

    event.preventDefault();
  }

  function onPointerDown(event: PointerEvent) {
    canvas.setPointerCapture(event.pointerId);

    if (event.offsetY < RULER_HEIGHT) {
      // The head is the playhead's own handle; the rest of the ruler is the
      // handle for the view. Taking hold of the ruler and sliding is how you
      // get somewhere else without moving where you are.
      if (onHead(event.offsetX)) {
        scrubbing = true;
        cursor = "grabbing";
        return;
      }

      viewport.following = false;
      pan = { x: event.offsetX, scroll, idle: true };
      cursor = "grabbing";
      return;
    }

    const grabbed =
      timeline && hit(timeline, view(), event.offsetX, event.offsetY);
    if (grabbed) {
      // Shift keeps what is already selected; a plain press replaces it —
      // but only when the clip is not already in the selection, so dragging
      // a group does not collapse it to whichever clip was taken hold of.
      if (event.shiftKey || event.metaKey) selection.toggle(grabbed.clip);
      else if (!selection.has(grabbed.clip)) selection.set(grabbed.clip);

      drag = start(grabbed);
      cursor = grabbed.edge ? "ew-resize" : "grabbing";
      return;
    }

    // Empty space: the press is a scrub, and it lets go of the selection the
    // way clicking away from a thing does everywhere else.
    if (!event.shiftKey && !event.metaKey) selection.clear();
    seek(frameAt(event.offsetX));
  }

  /**
   * Take hold of a gesture: which clips it carries, and how far they may go.
   *
   * The group is the whole selection when the grabbed clip is part of it, and
   * otherwise just that clip. Copied as it stands now, so every offset is
   * measured from where the clips started rather than from wherever the last
   * pointer event left them.
   */
  function start(grabbed: Hit): Drag {
    // The selection when the grabbed clip is part of it, and otherwise just
    // that clip.
    const ids = selection.has(grabbed.clip)
      ? new Set(selection.ids)
      : new Set([grabbed.clip]);
    const group = placementsOf(ids);
    const held = group.find((clip) => clip.id === grabbed.clip)!;

    return {
      group,
      held,
      ids,
      edge: grabbed.edge,
      grab: grabbed.frame - grabbed.position,
      limits: grabbed.edge
        ? trimLimits(group, grabbed.edge)
        : moveLimits(group),
      candidates: snapCandidates(timeline!, ids),
      frames: 0,
      tracks: 0,
      snappedAt: null,
      idle: true,
    };
  }

  /**
   * How far a carried group may go: not before the first frame, and not off
   * either end of the tracks. Lane limits are a count of lanes, not frames.
   */
  function moveLimits(group: Placement[]): Drag["limits"] {
    const tracks = group.map((clip) => clip.track);

    return {
      frames: [-Math.min(...group.map((clip) => clip.position)), Infinity],
      tracks: [
        -Math.min(...tracks),
        (timeline?.tracks.length ?? 0) - Math.max(...tracks),
      ],
    };
  }

  /**
   * How far a trim may go, as the tightest of what each clip allows.
   *
   * `trimRange` is a clip's own source head and tail plus whatever its
   * neighbours leave; the engine refuses the whole group if any one of them
   * cannot do it, so the handle has to stop where the first of them would.
   * Fixed for the drag — the document does not change under it.
   */
  function trimLimits(group: Placement[], edge: Edge): Drag["limits"] {
    let low = -Infinity;
    let high = Infinity;

    for (const clip of group) {
      const from = edgeOf(clip, edge);
      const anchor =
        edge === "in" ? clip.position + clip.length : clip.position;
      const range =
        timeline && trimRange(timeline, clip.track, clip.id, edge, anchor);

      // A clip with nowhere to go pins the whole group where it is.
      if (!range) return { frames: [0, 0], tracks: [0, 0] };

      low = Math.max(low, range[0] - from);
      high = Math.min(high, range[1] - from);
    }

    return { frames: [Math.min(low, 0), Math.max(high, 0)], tracks: [0, 0] };
  }

  const edgeOf = (clip: Placement, edge: Edge) =>
    edge === "in" ? clip.position : clip.position + clip.length;

  /**
   * Where the playhead is drawn: between frames while playback runs, and
   * wherever the easing has reached when it jumped.
   */
  const at = () => (playing ? exact : fades.head.value);

  /** Whether a press in the ruler landed on the playhead's head. */
  const onHead = (x: number) => Math.abs(x - xOf(at())) <= HEAD / 2 + 2;

  function onPointerMove(event: PointerEvent) {
    if (scrubbing) {
      seek(frameAt(event.offsetX));
      return;
    }

    if (pan) {
      // Direct: the frame under the pointer stays under the pointer, so the
      // document follows the hand rather than moving against it.
      const moved = event.offsetX - pan.x;
      if (moved !== 0) pan.idle = false;
      viewport.to(clampScroll(pan.scroll - moved / zoom), zoom);
      return;
    }

    if (drag) {
      carry(event, drag);
      return;
    }

    if (event.buttons & 1) {
      seek(frameAt(event.offsetX));
      return;
    }

    if (event.offsetY < RULER_HEIGHT) {
      hover = null;
      cursor = onHead(event.offsetX) ? "ew-resize" : "grab";
      return;
    }

    const over =
      timeline && hit(timeline, view(), event.offsetX, event.offsetY);
    hover = over ? over.clip : null;
    cursor = !over ? "default" : over.edge ? "ew-resize" : "grab";
  }

  /** Where every carried clip would land, given where the pointer is. */
  function carry(event: PointerEvent, d: Drag) {
    // The group keeps its shape, so one clip decides the offset and the rest
    // come along: the grabbed one, which is the only clip the pointer is
    // actually over.
    const tracks = d.edge
      ? 0
      : clamp(
          (trackAt(event.offsetY) ?? d.held.track) - d.held.track,
          d.limits.tracks,
        );

    // A trim moves the end that was taken hold of; a move takes the whole
    // clip, from wherever inside it the pointer went down.
    const pointer = d.edge
      ? frameAt(event.offsetX) - edgeOf(d.held, d.edge)
      : frameAt(event.offsetX) - d.grab - d.held.position;

    // Both ends of every carried clip are offered to the snap — the clip
    // meeting its new neighbour is rarely the one under the pointer. A trim
    // offers only the end it is moving.
    const edges = d.group.flatMap((clip) =>
      d.edge
        ? [edgeOf(clip, d.edge) + pointer]
        : [clip.position + pointer, clip.position + clip.length + pointer],
    );
    const caught = snapDelta(d.candidates, playhead, view(), edges);

    const pulled = pointer + caught.pull;
    const frames = clamp(pulled, d.limits.frames);

    // The ghost is the hand's own doing, so it arrives with the hand rather
    // than easing in behind it.
    if (d.idle && (frames !== 0 || tracks !== 0)) fades.carry.set(1);

    drag = {
      ...d,
      frames,
      tracks,
      // Only when the snap survived the clamp: a line against an edge the
      // gesture was not allowed to reach would be describing nothing.
      snappedAt: frames === pulled ? caught.at : null,
      idle: d.idle && frames === 0 && tracks === 0,
    };
  }

  /** Which way each arrow key moves things: frames, then lanes. */
  const NUDGES: Record<string, [number, number]> = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };

  /**
   * Move the selection by hand.
   *
   * The same edit a drag sends, clamped by the same limits — so the keyboard
   * cannot put clips anywhere the pointer could not, and a nudge into a
   * neighbour overwrites it exactly as a drop does.
   */
  function shift(frames: number, tracks: number, continuing = false) {
    const group = placementsOf(selection.ids);
    if (group.length === 0) return;

    const limits = moveLimits(group);
    const by = clamp(frames, limits.frames);
    const lanes = clamp(tracks, limits.tracks);
    if (by === 0 && lanes === 0) return;

    attempt(
      api.edit.moveClips(
        group.map((clip) => ({
          clip: clip.id,
          track: clip.track + lanes,
          position: clip.position + by,
        })),
        continuing,
      ),
    );
  }

  const clamp = (value: number, [low, high]: [number, number]) =>
    Math.min(Math.max(value, low), high);

  /** The named clips as they stand, which is what every gesture starts from. */
  function placementsOf(ids: ReadonlySet<number>): Placement[] {
    const group: Placement[] = [];

    timeline?.tracks.forEach((lane, track) => {
      for (const clip of lane.clips) {
        if (!ids.has(clip.id)) continue;
        group.push({ id: clip.id, track, position: clip.position, length: clip.length });
      }
    });

    return group;
  }

  /** Where the drag would leave every clip it carries. */
  function placements(d: Drag): Placement[] {
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

  function onPointerUp(event: PointerEvent) {
    if (scrubbing) {
      scrubbing = false;
      cursor = "ew-resize";
      return;
    }

    if (pan) {
      // A press on the ruler that never moved is a click, and a click on the
      // ruler is still the quickest way to put the playhead somewhere.
      const { idle } = pan;
      pan = null;
      cursor = "grab";
      if (idle) seek(frameAt(event.offsetX));
      return;
    }

    const carried = drag;
    drag = null;
    cursor = "grab";
    // The document is about to show the result, so there is nothing for the
    // ghosts to fade out of.
    fades.carry.set(0);

    if (!carried) return;

    // Never moved: the press was a click on a clip, which has already
    // selected it. The playhead stays where it is — a selection that also
    // scrubbed would make picking a clip cost the position you were at.
    if (carried.idle) return;

    // One edit for the group, however many clips are in it: the engine checks
    // them together, and undo then takes the gesture back in one. What is
    // sent is what was drawn — the same placements, not an offset the engine
    // would have to work them out from again.
    const landed = placements(carried);
    settle(landed);

    if (carried.edge) {
      const edge = carried.edge;
      attempt(
        api.edit.trim(
          landed.map((clip) => ({
            clip: clip.id,
            edge,
            frame: edgeOf(clip, edge),
          })),
        ),
      );
      return;
    }

    attempt(
      api.edit.moveClips(
        landed.map(({ id, track, position }) => ({
          clip: id,
          track,
          position,
        })),
        false,
      ),
    );
  }

  /** Abandon a carry, leaving the document alone. */
  function onPointerCancel() {
    scrubbing = false;
    pan = null;
    drag = null;
    cursor = "default";
    fades.carry.set(0);
  }
</script>

<div class="wrap" bind:this={wrap}>
  <canvas
    bind:this={canvas}
    style="width: {width}px; height: {height}px; cursor: {cursor}"
    onwheel={onWheel}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerCancel}
  ></canvas>
</div>

<style>
  .wrap {
    flex: none;
    height: 180px;
    background: var(--glow-bg-surface);
    border-top: 1px solid var(--glow-border-color);
    overflow: hidden;
  }

  canvas {
    display: block;
  }
</style>
