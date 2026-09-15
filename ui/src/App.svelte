<script lang="ts">
  import { AnimatedValue, Button, Root, Text } from "glow";
  import Titlebar from "./lib/Titlebar.svelte";
  import Stage from "./lib/Stage.svelte";
  import Timeline from "./lib/Timeline.svelte";
  import api, {
    Playhead,
    timecode,
    type StatsDto,
    type TimelineDto,
  } from "./lib/api.svelte";

  const playhead = new Playhead();

  let timeline = $state<TimelineDto | null>(null);
  let readout = $state("");

  // The document and the playhead's basis are pushed, so nothing polls for
  // them. `get`/`state` are only the initial fetch, before the first event.
  $effect(() => {
    api.timeline.get().then((t) => (timeline = t));
    api.transport.state().then((t) => playhead.sync(t));

    const stop = [
      api.timelineEvents.onChanged((t) => (timeline = t)),
      api.transportEvents.onChanged((t) => playhead.sync(t)),
    ];

    return () => stop.forEach((off) => off());
  });

  // The playhead runs on the local clock between those corrections. The
  // readout still polls: it is diagnostics, and pushing it would be traffic
  // for its own sake.
  $effect(() => {
    let running = true;
    let last: StatsDto | null = null;
    let since = performance.now();
    let uiFrames = 0;

    const frame = () => {
      if (!running) return;
      playhead.tick();
      uiFrames++;

      const now = performance.now();
      if (now - since > 500) {
        const secs = (now - since) / 1000;
        const ui = uiFrames / secs;
        uiFrames = 0;
        since = now;

        api.surface
          .stats()
          .then((s) => {
            const rate = (to: number, from: number) =>
              last ? (to - from) / secs : 0;
            readout =
              `ui ${ui.toFixed(0)} · video ${rate(s.frames, last?.frames ?? 0).toFixed(1)}` +
              ` · present ${rate(s.presents, last?.presents ?? 0).toFixed(1)}` +
              ` · dropped ${rate(s.dropped, last?.dropped ?? 0).toFixed(1)}/s`;
            last = s;
          })
          .catch((e) => (readout = `ipc failed: ${e}`));
      }

      requestAnimationFrame(frame);
    };

    requestAnimationFrame(frame);
    return () => (running = false);
  });
</script>

<Root theme="dark">
  <Titlebar />
  <Stage />

  <footer>
    <Button
      size="sm"
      variant="ghost"
      icon={playhead.playing ? "Pause" : "Play"}
      label={playhead.playing ? "Pause" : "Play"}
      onclick={api.transport.toggle}
    />
    <Text
      as="span"
      size="sm"
      variant="primary"
      style="font-variant-numeric: tabular-nums"
    >
      {@const time = timecode(playhead.frame, playhead.fps)}
      <AnimatedValue value={time.minutes} pad={2} mode="odometer" />
      :
      <AnimatedValue value={time.seconds} pad={2} mode="odometer" />
      .
      <AnimatedValue value={time.subseconds} pad={2} />
    </Text>
    <span class="readout">
      <Text
        as="span"
        size="xs"
        variant="secondary"
        style="font-variant-numeric: tabular-nums"
      >
        {readout}
      </Text>
    </span>
  </footer>

  <Timeline {timeline} playhead={playhead.frame} fps={playhead.fps} />
</Root>

<style>
  footer {
    display: flex;
    align-items: center;
    gap: var(--glow-space-3, 12px);
    padding: var(--glow-space-2, 8px) var(--glow-space-3, 12px);
    background: var(--glow-bg-base);
    border-top: 1px solid var(--glow-border-color);
    flex: none;
  }

  .readout {
    margin-left: auto;
  }
</style>
