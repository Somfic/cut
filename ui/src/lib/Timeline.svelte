<script lang="ts">
  import type { TimelineDto } from "./api.svelte";
  import api from "./api.svelte";
  import {
    MAX_ZOOM,
    MIN_ZOOM,
    RULER_HEIGHT,
    TRACK_GAP,
    TRACK_HEIGHT,
    frameAt as frameAtIn,
    covered,
    hit,
    snapDelta,
    topOf,
    trackAt,
    trimRange,
    xOf as xOfIn,
    type Edge,
    type Placement,
    type Viewport,
  } from "./timeline";
  import { toast } from "glow";
  import { selection } from "./selection.svelte";

  let {
    timeline,
    playhead,
    fps,
  }: {
    timeline: TimelineDto | null;
    playhead: number;
    fps: number;
  } = $props();

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;

  // Pixels per frame, and the frame at the left edge.
  let zoom = $state(0.2);
  let scroll = $state(0);
  let width = $state(0);
  let height = $state(0);
  let fitted = false;

  /**
   * A clip being carried, and where it would land if the pointer went up now.
   *
   * Nothing is sent to the engine until then: a move is a single edit, and
   * one per pointer event would fill the undo stack with the path the mouse
   * took rather than the move the user made.
   */
  /** A clip taking part in a drag, as it was before the drag started. */
  type Carried = { id: number; track: number; position: number; length: number };

  type Drag = {
    /** The clip actually taken hold of; the rest of the group follows it. */
    clip: number;
    group: Carried[];
    /** Which handle is being pulled, or null when the clips are carried. */
    edge: Edge | null;
    /** Frames between the grabbed clip's start and where it was picked up. */
    grab: number;
    /** What the whole group is offset by — the shape of the edit sent. */
    frames: number;
    tracks: number;
    /** Whether it may — the engine asks this again. */
    ok: boolean;
    /** The frame a carried edge caught on, for the line drawn down it. */
    snappedAt: number | null;
    /** Cleared on the first real movement; until then this is still a click. */
    idle: boolean;
  };

  let drag = $state<Drag | null>(null);
  /** The clip under the pointer, which is the one wearing handles. */
  let hover = $state<number | null>(null);
  let cursor = $state("default");

  const viewport = (): Viewport => ({ scroll, zoom });
  const xOf = (frame: number) => xOfIn(viewport(), frame);
  const frameAt = (x: number) => frameAtIn(viewport(), x);

  /** Seconds between ruler labels, chosen so they never crowd. */
  function tickStep(): number {
    const steps = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
    return steps.find((s) => s * fps * zoom >= 90) ?? steps[steps.length - 1];
  }

  function draw() {
    const dpr = window.devicePixelRatio;
    const ctx = canvas?.getContext("2d");
    if (!ctx || width === 0) return;

    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const css = getComputedStyle(document.documentElement);
    const color = (name: string) => css.getPropertyValue(name).trim();

    ctx.fillStyle = color("--glow-bg-surface");
    ctx.fillRect(0, 0, width, height);

    // Ruler.
    ctx.fillStyle = color("--glow-bg-base");
    ctx.fillRect(0, 0, width, RULER_HEIGHT);
    ctx.strokeStyle = color("--glow-border-color");
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, RULER_HEIGHT - 0.5);
    ctx.lineTo(width, RULER_HEIGHT - 0.5);
    ctx.stroke();

    const step = tickStep() * fps;
    ctx.fillStyle = color("--glow-text-secondary");
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

    // Only what is on screen: the document runs to hundreds of clips.
    timeline?.tracks.forEach((track, index) => {
      const top = topOf(index);

      for (const clip of track.clips) {
        const x = xOf(clip.position);
        const w = clip.length * zoom;
        if (x + w < 0) continue;
        if (x > width) break;

        // The one being carried stays drawn where it still is, faintly: the
        // move has not happened yet, and a hole in the track would suggest
        // otherwise.
        ctx.globalAlpha =
          drag && !drag.idle && drag.group.some((c) => c.id === clip.id) ? 0.25 : 1;
        ctx.fillStyle = color("--glow-primary-soft-strong");
        roundRect(ctx, x, top, Math.max(w, 1), TRACK_HEIGHT, 4);
        ctx.fill();

        // Selection reads as an outline rather than a different fill: at this
        // zoom a clip can be two pixels wide, where a fill change is
        // invisible and a border still lands.
        if (selection.has(clip.id)) {
          ctx.strokeStyle = color("--glow-primary");
          ctx.lineWidth = 2;
          roundRect(ctx, x + 1, top + 1, Math.max(w - 2, 1), TRACK_HEIGHT - 2, 3);
          ctx.stroke();
        }

        if (w > 40) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(x, top, w - 6, TRACK_HEIGHT);
          ctx.clip();
          ctx.fillStyle = color("--glow-text-primary");
          ctx.fillText(clip.name, x + 6, top + 12);
          ctx.restore();
        }

        // Handles only on the clip under the pointer: on every clip they
        // would read as part of how a clip is drawn rather than as something
        // to take hold of, and at this zoom there can be hundreds on screen.
        if (clip.id === hover && !drag) drawHandles(ctx, color, x, w, top);
        if (clip.id === hover && !drag) drawGrip(ctx, color, x, w, top);
      }

      ctx.globalAlpha = 1;
    });

    if (drag && !drag.idle) drawCarried(ctx, color);

    // The edge a drag caught on, down the full height so it can be read
    // against clips on every track — which is where it may have come from.
    if (drag?.snappedAt != null) {
      const at = Math.round(xOf(drag.snappedAt)) + 0.5;
      ctx.save();
      // Warning, not primary: the playhead is already a primary line, and
      // two of them a few pixels apart during a drag is unreadable.
      ctx.strokeStyle = color("--glow-color-warning");
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.moveTo(at, 0);
      ctx.lineTo(at, height);
      ctx.stroke();
      ctx.restore();
    }

    // Playhead, last so nothing covers it.
    const x = Math.round(xOf(playhead)) + 0.5;
    if (x >= 0 && x <= width) {
      ctx.strokeStyle = color("--glow-primary");
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
    }
  }

  /** The two bars at a clip's ends, which drag its in- and out-points. */
  function drawHandles(
    ctx: CanvasRenderingContext2D,
    color: (name: string) => string,
    x: number,
    w: number,
    top: number,
  ) {
    // Narrower than the grab zone on purpose: a bar thinner than the target
    // it stands for is forgiving, one wider than it is a lie.
    const bar = 3;
    const inset = 2;
    if (w < bar * 4) return;

    ctx.save();
    ctx.fillStyle = color("--glow-primary");
    for (const at of [x + inset, x + w - inset - bar]) {
      roundRect(ctx, at, top + 6, bar, TRACK_HEIGHT - 12, bar / 2);
      ctx.fill();
    }
    ctx.restore();
  }

  /**
   * The grip in the middle of a hovered clip: the affordance for moving it,
   * as distinct from the two end bars that resize it.
   */
  function drawGrip(
    ctx: CanvasRenderingContext2D,
    color: (name: string) => string,
    x: number,
    w: number,
    top: number,
  ) {
    const rows = 3;
    const gap = 4;
    const dot = 2;
    // Below this the grip would sit on top of the two handles, and a clip
    // that small is dragged by its body anyway.
    if (w < 40) return;

    ctx.save();
    ctx.fillStyle = color("--glow-text-secondary");
    ctx.globalAlpha = 0.7;

    const cx = x + w / 2;
    const cy = top + TRACK_HEIGHT - 14;
    for (let i = 0; i < rows; i++) {
      const at = cx + (i - (rows - 1) / 2) * gap;
      ctx.beginPath();
      ctx.arc(at, cy, dot / 2, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();
  }

  /** Every carried clip where it would land, and any lane that isn't there. */
  function drawCarried(
    ctx: CanvasRenderingContext2D,
    color: (name: string) => string,
  ) {
    if (!drag) return;

    const lanes = timeline?.tracks.length ?? 0;
    const fill = color(drag.ok ? "--glow-primary-soft-strong" : "--glow-color-danger");
    const line = color(drag.ok ? "--glow-primary" : "--glow-color-danger");

    // What the drop would take off the clips underneath, drawn on them
    // before it happens.
    ctx.save();
    ctx.globalAlpha = 0.45;
    ctx.fillStyle = color("--glow-color-danger");
    for (const eaten of covered(timeline!, placements(drag))) {
      const x = xOf(eaten.position);
      roundRect(ctx, x, topOf(eaten.track), Math.max(eaten.length * zoom, 1), TRACK_HEIGHT, 4);
      ctx.fill();
    }
    ctx.restore();

    for (const clip of placements(drag)) {
      const top = topOf(clip.track);
      const x = xOf(clip.position);
      const w = Math.max(clip.length * zoom, 1);
      if (x + w < 0 || x > width) continue;

      // Dropping past the last track starts a new one, so there is no lane
      // drawn underneath yet — an outline stands in for it.
      if (clip.track >= lanes) {
        ctx.save();
        ctx.strokeStyle = color("--glow-border-color");
        ctx.setLineDash([4, 4]);
        ctx.strokeRect(0.5, top + 0.5, width - 1, TRACK_HEIGHT - 1);
        ctx.restore();
      }

      ctx.save();
      ctx.globalAlpha = 0.9;
      ctx.fillStyle = fill;
      roundRect(ctx, x, top, w, TRACK_HEIGHT, 4);
      ctx.fill();

      ctx.strokeStyle = line;
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.restore();
    }
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
    if (timeline) {
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

  // redraw whenever anything it reads changes. `playhead`
  $effect(() => {
    void [
      timeline,
      playhead,
      zoom,
      scroll,
      width,
      height,
      fps,
      drag,
      hover,
      selection.size,
      ...selection.ids,
    ];
    draw();
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
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * factor));
    scroll = clampScroll(anchor - at / zoom);
  }

  // fit the whole document, or one second if there is nothing in it
  function fit() {
    const frames = timeline?.length || fps;
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, width / frames));
    scroll = 0;
  }

  // never past the end of the document, never before the start
  function clampScroll(to: number): number {
    const frames = timeline?.length ?? 0;
    return Math.max(0, Math.min(to, Math.max(0, frames - width / zoom)));
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
    const fine =
      Math.abs(event.deltaY) < 50 || !Number.isInteger(event.deltaY);

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
    scroll = clampScroll(scroll + delta / zoom);
  }

  // zoom from the keyboard, about the playhead rather than the pointer. */
  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape" && (drag || selection.size)) {
      drag = null;
      cursor = "default";
      selection.clear();
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

    const grabbed = timeline && hit(timeline, viewport(), event.offsetX, event.offsetY);
    if (grabbed) {
      // Shift keeps what is already selected; a plain press replaces it —
      // but only when the clip is not already in the selection, so dragging
      // a group does not collapse it to whichever clip was taken hold of.
      if (event.shiftKey || event.metaKey) selection.toggle(grabbed.clip);
      else if (!selection.has(grabbed.clip)) selection.set(grabbed.clip);

      drag = {
        clip: grabbed.clip,
        group: carriedGroup(grabbed.clip),
        edge: grabbed.edge,
        grab: grabbed.frame - grabbed.position,
        frames: 0,
        tracks: 0,
        ok: true,
        snappedAt: null,
        idle: true,
      };
      cursor = grabbed.edge ? "ew-resize" : "grabbing";
      return;
    }

    // Empty space: the press is a scrub, and it lets go of the selection the
    // way clicking away from a thing does everywhere else.
    if (!event.shiftKey && !event.metaKey) selection.clear();
    api.transport.seek(frameAt(event.offsetX));
  }

  /**
   * Everything a drag carries: the whole selection when the grabbed clip is
   * part of it, and otherwise just that clip. Copied as it stands now, so
   * every offset below is measured from where the clips actually started
   * rather than from wherever the last pointer event left them.
   */
  function carriedGroup(grabbed: number): Carried[] {
    const carried: Carried[] = [];

    timeline?.tracks.forEach((lane, track) => {
      for (const clip of lane.clips) {
        const taken = selection.has(clip.id) ? selection.has(grabbed) : clip.id === grabbed;
        if (!taken) continue;

        carried.push({ id: clip.id, track, position: clip.position, length: clip.length });
      }
    });

    return carried;
  }

  function onPointerMove(event: PointerEvent) {
    if (drag) {
      carry(event);
      return;
    }

    if (event.buttons & 1) {
      api.transport.seek(frameAt(event.offsetX));
      return;
    }

    const over = timeline && hit(timeline, viewport(), event.offsetX, event.offsetY);
    hover = over ? over.clip : null;
    cursor = !over ? "default" : over.edge ? "ew-resize" : "grab";
  }

  /** Where every carried clip would land, given where the pointer is. */
  function carry(event: PointerEvent) {
    if (!drag || !timeline) return;
    if (drag.edge) return stretch(event, drag.edge);

    const held = drag.group.find((c) => c.id === drag?.clip);
    if (!held) return;

    // The group keeps its shape, so one clip decides the offset and the rest
    // come along: the grabbed one, which is the only clip the pointer is
    // actually over.
    const lane = trackAt(event.offsetY) ?? held.track;
    const highest = Math.min(...drag.group.map((c) => c.track));
    const lowest = Math.max(...drag.group.map((c) => c.track));
    const tracks = Math.max(
      -highest,
      Math.min(lane - held.track, timeline.tracks.length - lowest),
    );

    const loose = Math.max(0, frameAt(event.offsetX) - drag.grab) - held.position;

    // Both ends of every carried clip are offered to the snap — the clip
    // meeting its new neighbour is rarely the one under the pointer.
    const edges = drag.group.flatMap((clip) => [
      clip.position + loose,
      clip.position + clip.length + loose,
    ]);
    const caught = snapDelta(timeline, playhead, viewport(), edges, moving(drag));

    // Never before the first frame, whichever clip gets there first.
    const earliest = Math.min(...drag.group.map((c) => c.position));
    const frames = Math.max(loose + caught.pull, -earliest);

    drag = {
      ...drag,
      frames,
      tracks,
      // Landing on something is allowed: the drop shortens what is under it
      // rather than being refused, and `covered` is what that will cost.
      ok: true,
      snappedAt: frames === loose + caught.pull ? caught.at : null,
      idle: drag.idle && frames === 0 && tracks === 0,
    };
  }

  /**
   * Pull one end of every carried clip by the same number of frames.
   *
   * Clamped to what the tightest clip in the group allows: `trimRange` is a
   * clip's own source head and tail plus whatever its neighbours leave, and
   * the engine refuses the whole group if any one of them cannot do it — so
   * the handle stops where the first of them would.
   */
  function stretch(event: PointerEvent, edge: Edge) {
    if (!drag || !timeline) return;

    const held = drag.group.find((c) => c.id === drag?.clip);
    if (!held) return;

    const edgeOf = (c: Carried) => (edge === "in" ? c.position : c.position + c.length);

    let low = -Infinity;
    let high = Infinity;
    for (const clip of drag.group) {
      const anchor = edge === "in" ? clip.position + clip.length : clip.position;
      const range = trimRange(timeline, clip.track, clip.id, edge, anchor);
      if (!range) return;

      low = Math.max(low, range[0] - edgeOf(clip));
      high = Math.min(high, range[1] - edgeOf(clip));
    }

    const loose = frameAt(event.offsetX) - edgeOf(held);
    const edges = drag.group.map((clip) => edgeOf(clip) + loose);
    const caught = snapDelta(timeline, playhead, viewport(), edges, moving(drag));

    const pulled = loose + caught.pull;
    const frames = Math.min(Math.max(pulled, low), high);

    drag = {
      ...drag,
      frames,
      ok: low <= high,
      // Only when the snap survived the clamp: a line drawn against an edge
      // the trim was not allowed to reach would be describing nothing.
      snappedAt: frames === pulled ? caught.at : null,
      idle: drag.idle && frames === 0,
    };
  }

  /** The ids taking part, for the calculations that must skip themselves. */
  const moving = (d: Drag) => new Set(d.group.map((c) => c.id));

  /** Where the drag would leave every clip it carries. */
  function placements(d: Drag): Placement[] {
    return d.group.map((clip) => {
      if (d.edge === "in") {
        return {
          id: clip.id,
          track: clip.track,
          position: clip.position + d.frames,
          length: clip.length - d.frames,
        };
      }
      if (d.edge === "out") {
        return {
          id: clip.id,
          track: clip.track,
          position: clip.position,
          length: clip.length + d.frames,
        };
      }

      return {
        id: clip.id,
        track: clip.track + d.tracks,
        position: clip.position + d.frames,
        length: clip.length,
      };
    });
  }

  function onPointerUp(_event: PointerEvent) {
    const carried = drag;
    drag = null;
    cursor = "grab";

    if (!carried) return;

    // Never moved: the press was a click on a clip, which has already
    // selected it. The playhead stays where it is — a selection that also
    // scrubbed would make picking a clip cost the position you were at.
    if (carried.idle) return;

    // Refused here rather than sent and bounced — the engine would say the
    // same thing, but only after the clips had already snapped back.
    if (!carried.ok) {
      toast.error("There is a clip in the way");
      return;
    }

    const clips = carried.group.map((clip) => clip.id);
    const failed = (e: { message?: string }) => toast.error(String(e?.message ?? e));

    // One edit for the group, however many clips are in it: the engine
    // checks them together, and undo then takes the gesture back in one.
    if (carried.edge) {
      api.edit.stretch(clips, carried.edge, carried.frames).catch(failed);
      return;
    }

    api.edit.nudge(clips, carried.frames, carried.tracks, true).catch(failed);
  }

  /** Abandon a carry, leaving the document alone. */
  function onPointerCancel() {
    drag = null;
    cursor = "default";
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
