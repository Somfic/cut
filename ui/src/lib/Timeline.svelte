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
    topOf,
    xOf as xOfIn,
    type Viewport,
  } from "./timeline";

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

        ctx.fillStyle = color("--glow-primary-soft-strong");
        roundRect(ctx, x, top, Math.max(w, 1), TRACK_HEIGHT, 4);
        ctx.fill();

        if (w > 40) {
          ctx.save();
          ctx.beginPath();
          ctx.rect(x, top, w - 6, TRACK_HEIGHT);
          ctx.clip();
          ctx.fillStyle = color("--glow-text-primary");
          ctx.fillText(clip.name, x + 6, top + 12);
          ctx.restore();
        }
      }
    });

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

  /** Fit the whole document once its length and our width are both known. */
  $effect(() => {
    if (fitted || !timeline || timeline.length === 0 || width === 0) return;
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, width / timeline.length));
    fitted = true;
  });

  // Redraw whenever anything it reads changes. `playhead` ticks at display
  // rate, so this is the loop that has to stay cheap.
  $effect(() => {
    void [timeline, playhead, zoom, scroll, width, height, fps];
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

  function onWheel(event: WheelEvent) {
    event.preventDefault();

    if (event.ctrlKey || event.metaKey) {
      // Zoom about the pointer.
      const anchor = scroll + event.offsetX / zoom;
      zoom = Math.min(
        MAX_ZOOM,
        Math.max(MIN_ZOOM, zoom * Math.exp(-event.deltaY / 200)),
      );
      scroll = Math.max(0, anchor - event.offsetX / zoom);
    } else {
      scroll = Math.max(0, scroll + event.deltaX / zoom);
    }
  }

  function onPointerDown(event: PointerEvent) {
    canvas.setPointerCapture(event.pointerId);
    api.transport.seek(frameAt(event.offsetX));
  }

  function onPointerMove(event: PointerEvent) {
    if (event.buttons & 1) api.transport.seek(frameAt(event.offsetX));
  }
</script>

<div class="wrap" bind:this={wrap}>
  <canvas
    bind:this={canvas}
    style="width: {width}px; height: {height}px"
    onwheel={onWheel}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
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
