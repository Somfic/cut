<script lang="ts">
  import api from "$lib/api";
  import { document } from "$lib/document.svelte";
  import { mode } from "$lib/mode.svelte";
  import { playhead } from "$lib/playhead.svelte";
  import { selection } from "$lib/selection.svelte";
  import { Scene } from "$lib/timeline/scene";
  import { HEAD, paint, palette } from "$lib/timeline/paint";
  import { viewport } from "$lib/timeline/viewport.svelte";
  import {
    carry,
    clamp,
    edgeOf,
    moveLimits,
    placements,
    placementsOf,
    start,
    type Drag,
  } from "$lib/timeline/gesture";
  import {
    MAX_ZOOM,
    MIN_ZOOM,
    RULER_HEIGHT,
    frameAt as frameAtIn,
    hit,
    trackAt,
    xOf as xOfIn,
    type Viewport,
  } from "$lib/timeline/geometry";

  const timeline = $derived(document.timeline);

  let canvas: HTMLCanvasElement;
  let wrap: HTMLDivElement;
  let width = $state(0);
  let height = $state(0);

  const zoom = $derived(viewport.zoom);
  const scroll = $derived(viewport.scroll);
  const view = (): Viewport => ({ scroll, zoom });
  const xOf = (frame: number) => xOfIn(view(), frame);
  const frameAt = (x: number) => frameAtIn(view(), x);

  const scene = new Scene();

  /** The ruler dragged to slide the document under a still playhead. */
  type Pan = { x: number; scroll: number; idle: boolean };

  let pan = $state<Pan | null>(null);
  let scrubbing = $state(false);
  let drag = $state<Drag | null>(null);
  let hover = $state<number | null>(null);
  let cursor = $state("default");
  let fitted = false;

  /** Where the playhead is drawn: between frames, or wherever easing reached. */
  const at = () =>
    playhead.playing ? playhead.exact : scene.fades.head.value;
  const onHead = (x: number) => Math.abs(x - xOf(at())) <= HEAD / 2 + 2;

  let pending = 0;

  /** One paint per frame, however many events asked for it. */
  function schedule() {
    if (pending) return;
    pending = requestAnimationFrame(() => {
      pending = 0;
      draw();
    });
  }

  function draw() {
    const ctx = canvas?.getContext("2d");
    if (!ctx || width === 0) return;

    // Only on a real size change: assigning either reallocates the buffer.
    const dpr = window.devicePixelRatio;
    const [pixels, lines] = [Math.round(width * dpr), Math.round(height * dpr)];
    if (canvas.width !== pixels || canvas.height !== lines) {
      canvas.width = pixels;
      canvas.height = lines;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // Aimed before advancing, or a target set mid-paint waits a frame.
    const selected = new Set(selection.ids);
    const present = new Set<number>();
    timeline?.tracks.forEach((lane, track) => {
      for (const clip of lane.clips) {
        present.add(clip.id);
        scene.show(clip, track, selected.has(clip.id), clip.id === hover && !drag);
      }
    });

    if (playhead.playing || scrubbing) scene.fades.head.set(playhead.exact);
    else scene.fades.head.to(playhead.frame);

    scene.fades.carry.to(drag && !drag.idle ? 1 : 0);
    scene.fades.snap.to(drag?.snappedAt != null ? 1 : 0);
    if (drag?.snappedAt != null) scene.snapAt = drag.snappedAt;

    const moving = scene.advance(present);

    paint({
      ctx,
      c: palette(),
      view: view(),
      width,
      height,
      fps: playhead.fps,
      scene,
      timeline,
      drag,
      head: at(),
      playing: playhead.playing,
    });

    if (moving) schedule();
  }

  $effect(() => {
    void [
      timeline,
      playhead.frame,
      playhead.exact,
      playhead.playing,
      playhead.fps,
      zoom,
      scroll,
      width,
      height,
      drag,
      hover,
    ];
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

  $effect(() => {
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  // Fit once, when the document's length and our width are both known.
  $effect(() => {
    if (fitted || !timeline || timeline.length === 0 || width === 0) return;
    fit();
    fitted = true;
  });

  // An edit can take a clip away, and a selection holding its id would point
  // at nothing.
  $effect(() => {
    if (timeline && selection.size) {
      selection.keep(timeline.tracks.flatMap((t) => t.clips.map((c) => c.id)));
    }
  });

  /**
   * Driven by `exact`, so the scroll is as continuous as the playhead. The
   * clamp is what lets it leave the middle at the ends of the document.
   */
  $effect(() => {
    if (!viewport.following || width === 0) return;
    viewport.to(clampScroll(at() - width / 2 / zoom), zoom);
  });

  function zoomBy(factor: number, about: number) {
    const anchor = scroll + about / zoom;
    const next = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * factor));

    viewport.to(clampScroll(anchor - about / next, next), next);
  }

  function fit() {
    viewport.to(0, width / (timeline?.length || playhead.fps));
  }

  function clampScroll(to: number, at = zoom): number {
    const frames = timeline?.length ?? 0;
    return Math.max(0, Math.min(to, Math.max(0, frames - width / at)));
  }

  /**
   * A trackpad sends small, often fractional deltas and a horizontal axis; a
   * mouse sends ±120 notches. Remembered for a moment, since a fast flick
   * does produce the occasional round, axis-aligned event.
   */
  let trackpadUntil = 0;

  function isTrackpad(event: WheelEvent): boolean {
    const fine = Math.abs(event.deltaY) < 50 || !Number.isInteger(event.deltaY);

    if (event.deltaMode === 0 && (fine || event.deltaX !== 0)) {
      trackpadUntil = performance.now() + 500;
      return true;
    }

    return performance.now() < trackpadUntil;
  }

  function onWheel(event: WheelEvent) {
    event.preventDefault();

    // A pinch is a wheel with `ctrlKey`; `alt` is the mouse's stand-in.
    if (event.ctrlKey || event.metaKey || event.altKey) {
      zoomBy(Math.exp(-event.deltaY / 200), event.offsetX);
      return;
    }

    // Sideways pans, so up and down is free to zoom. A mouse keeps its
    // wheel for panning: it has no horizontal axis.
    const sideways = Math.abs(event.deltaX) > Math.abs(event.deltaY);
    if (!sideways && isTrackpad(event)) {
      zoomBy(Math.exp(-event.deltaY / 200), event.offsetX);
      return;
    }

    viewport.following = false;
    const delta = event.deltaX !== 0 ? event.deltaX : event.deltaY;
    viewport.to(clampScroll(scroll + delta / zoom), zoom);
  }

  /** Which way each arrow key moves things: frames, then lanes. */
  const NUDGES: Record<string, [number, number]> = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape" && (drag || selection.size)) {
      abandon();
      selection.clear();
      event.preventDefault();
      return;
    }

    const nudge = NUDGES[event.key];
    if (nudge && !event.ctrlKey && !event.metaKey) {
      const [frames, tracks] = nudge;
      const step = event.shiftKey ? Math.round(playhead.fps) : 1;

      // `repeat` means the key never came back up: a held key is one
      // gesture, and one undo step.
      if (selection.size) shift(frames * step, tracks, event.repeat);
      else if (frames) playhead.seek(Math.max(0, playhead.frame + frames * step));

      event.preventDefault();
      return;
    }

    if (!(event.ctrlKey || event.metaKey)) return;

    const on = xOf(playhead.frame);
    const centre = on >= 0 && on <= width ? on : width / 2;

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

  /** The same edit a drag sends, clamped by the same limits. */
  function shift(frames: number, tracks: number, continuing = false) {
    const group = placementsOf(timeline, selection.ids);
    if (group.length === 0) return;

    const limits = moveLimits(timeline, group);
    const by = clamp(frames, limits.frames);
    const lanes = clamp(tracks, limits.tracks);
    if (by === 0 && lanes === 0) return;

    api.edit.moveClips(
      group.map((clip) => ({
        clip: clip.id,
        track: clip.track + lanes,
        position: clip.position + by,
      })),
      continuing,
    );
  }

  function onPointerDown(event: PointerEvent) {
    canvas.setPointerCapture(event.pointerId);

    if (event.offsetY < RULER_HEIGHT) {
      // The head is the playhead's handle; the ruler is the view's.
      if (onHead(event.offsetX)) {
        scrubbing = true;
      } else {
        viewport.following = false;
        pan = { x: event.offsetX, scroll, idle: true };
      }

      cursor = "grabbing";
      return;
    }

    const grabbed = timeline && hit(timeline, view(), event.offsetX, event.offsetY);
    if (grabbed) {
      // A plain press replaces the selection, unless the clip is in it —
      // dragging a group must not collapse it to the clip taken hold of.
      if (event.shiftKey || event.metaKey) selection.toggle(grabbed.clip);
      else if (!selection.has(grabbed.clip)) selection.set(grabbed.clip);

      drag = start(timeline!, grabbed, selection.ids);
      cursor = grabbed.edge ? "ew-resize" : "grabbing";
      return;
    }

    if (!event.shiftKey && !event.metaKey) selection.clear();
    playhead.seek(frameAt(event.offsetX));
  }

  function onPointerMove(event: PointerEvent) {
    if (scrubbing) {
      playhead.seek(frameAt(event.offsetX));
      return;
    }

    if (pan) {
      // The frame under the pointer stays under it.
      const moved = event.offsetX - pan.x;
      if (moved !== 0) pan.idle = false;
      viewport.to(clampScroll(pan.scroll - moved / zoom), zoom);
      return;
    }

    if (drag) {
      const was = drag.idle;
      drag = carry(
        drag,
        { frame: frameAt(event.offsetX), lane: trackAt(event.offsetY) },
        playhead.frame,
        view(),
      );

      // The ghost is the hand's doing: it arrives with the hand.
      if (was && !drag.idle) scene.fades.carry.set(1);
      return;
    }

    if (event.buttons & 1) {
      playhead.seek(frameAt(event.offsetX));
      return;
    }

    if (event.offsetY < RULER_HEIGHT) {
      hover = null;
      cursor = onHead(event.offsetX) ? "ew-resize" : "grab";
      return;
    }

    const over = timeline && hit(timeline, view(), event.offsetX, event.offsetY);
    hover = over ? over.clip : null;
    cursor = !over ? "default" : over.edge ? "ew-resize" : "grab";
  }

  function onPointerUp(event: PointerEvent) {
    if (scrubbing) {
      scrubbing = false;
      cursor = "ew-resize";
      return;
    }

    if (pan) {
      // A press that never moved is a click, which still seeks.
      const { idle } = pan;
      pan = null;
      cursor = "grab";
      if (idle) playhead.seek(frameAt(event.offsetX));
      return;
    }

    const carried = drag;
    abandon();
    cursor = "grab";

    // A click on a clip has already selected it; the playhead stays put.
    if (!carried || carried.idle) return;

    // One edit for the group, and what is sent is what was drawn.
    const landed = placements(carried);
    scene.settle(landed);

    if (carried.edge) {
      const edge = carried.edge;

      // A ripple closes one gap, so it is only an answer for one clip — and
      // only when the trim took frames away. A clip growing has no gap to
      // close; it overwrites what it grows into, as it does with the mode off.
      const [only] = landed;
      if (mode.ripple && landed.length === 1 && only.length < carried.held.length) {
        api.edit.ripple(only.id, edge, edgeOf(only, edge));
        return;
      }

      api.edit.trim(
        landed.map((clip) => ({ clip: clip.id, edge, frame: edgeOf(clip, edge) })),
      );
      return;
    }

    api.edit.moveClips(
      landed.map(({ id, track, position }) => ({ clip: id, track, position })),
      false,
    );
  }

  function abandon() {
    scrubbing = false;
    pan = null;
    drag = null;
    // The document is about to show the result.
    scene.fades.carry.set(0);
  }

  function onPointerCancel() {
    abandon();
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
