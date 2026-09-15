<script lang="ts">
  import { AnimatedValue, Button, registerShortcut, Root, Text } from "glow";

  import Stage from "$components/Stage.svelte";
  import Timeline from "$components/Timeline.svelte";
  import Titlebar from "$components/Titlebar.svelte";
  import api, { type StatsDto } from "$lib/api";
  import { document } from "$lib/document.svelte";
  import { mode } from "$lib/mode.svelte";
  import { playhead } from "$lib/playhead.svelte";
  import { selection } from "$lib/selection.svelte";
  import { timecode } from "$lib/timeline/geometry";

  let readout = $state("");

  // Delete and Backspace do what the menu's X does; a row can only print one.
  $effect(() => {
    const clips = document.clips.filter((clip) => selection.has(clip.id));
    const remove = () =>
      api.edit.delete(
        clips.map((clip) => clip.id),
        mode.ripple,
      );

    const stop = ["delete", "backspace"].map((key) =>
      registerShortcut(key, remove, { disabled: clips.length === 0 }),
    );

    return () => stop.forEach((off) => off());
  });

  // The readout polls: it is diagnostics, and pushing it would be traffic
  // for its own sake.
  $effect(() => {
    let running = true;
    let last: StatsDto | null = null;
    let since = performance.now();
    let uiFrames = 0;

    const frame = () => {
      if (!running) return;
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
    <Button
      size="sm"
      variant={mode.ripple ? "primary" : "ghost"}
      icon="Waves"
      ariaLabel="Ripple edits"
      tooltip="Ripple: close the gap an edit leaves"
      selected={mode.ripple}
      onclick={() => {
        mode.ripple = !mode.ripple;
      }}
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

  <Timeline />
</Root>

<style lang="scss">
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
