<script lang="ts">
  import { AnimatedValue, Button, Root, Text, toast } from "glow";
  import Titlebar from "./lib/Titlebar.svelte";
  import Stage from "./lib/Stage.svelte";
  import Timeline from "./lib/Timeline.svelte";
  import { type Menu } from "./lib/Menubar.svelte";
  import { selection } from "./lib/selection.svelte";
  import api, {
    Playhead,
    timecode,
    type StatsDto,
    type TimelineDto,
  } from "./lib/api.svelte";

  const playhead = new Playhead();

  let timeline = $state<TimelineDto | null>(null);
  let readout = $state("");

  // Nothing behind the file commands yet — the menu is the shape of the API
  // the engine still has to grow.
  const soon = (what: string) => () => toast.info(`${what} isn't wired up yet`);

  // An edit refused by the engine (a trim past the source, a move onto
  // occupied frames) comes back as a message written for a person, so it is
  // worth showing rather than swallowing.
  const attempt = (edit: Promise<void>) =>
    edit.catch((e) => toast.error(String(e?.message ?? e)));

  const step = (what: string, go: () => Promise<boolean>) => async () => {
    if (!(await go())) toast.info(`Nothing to ${what}`);
  };

  // The clip the playhead is inside, which is what a menu command acts on
  // until the timeline has a selection of its own.
  const atPlayhead = $derived(
    timeline?.tracks
      .flatMap((track) => track.clips)
      .find(
        (clip) =>
          playhead.frame >= clip.position &&
          playhead.frame < clip.position + clip.length,
      ) ?? null,
  );

  // What an edit command acts on: what is selected, or the clip under the
  // playhead when nothing is. Same rule for every command, so the menu never
  // disagrees with itself about what "the clip" means.
  const targets = $derived.by(() => {
    const clips = timeline?.tracks.flatMap((track) => track.clips) ?? [];
    const selected = clips.filter((clip) => selection.has(clip.id));

    return selected.length ? selected : atPlayhead ? [atPlayhead] : [];
  });

  // Only a clip the playhead is actually inside can be cut in two.
  const splittable = $derived(
    targets.filter(
      (clip) =>
        playhead.frame > clip.position &&
        playhead.frame < clip.position + clip.length,
    ),
  );

  const many = (what: string, n: number) =>
    n > 1 ? `${what} ${n} clips` : `${what} clip`;

  const menus: Menu[] = $derived([
    {
      label: "File",
      items: [
        {
          kind: "submenu",
          label: "Import",
          icon: "Import",
          items: [
            {
              kind: "item",
              label: "From folder\u2026",
              icon: "Folder",
              onclick: soon("Import from folder"),
            },
            {
              kind: "item",
              label: "From file\u2026",
              icon: "File",
              shortcut: "mod+i",
              onclick: soon("Import from file"),
            },
          ],
        },
        "divider",
        {
          kind: "item",
          label: "Export\u2026",
          icon: "Download",
          shortcut: "mod+e",
          onclick: soon("Export"),
        },
      ],
    },
    {
      label: "Edit",
      items: [
        {
          kind: "item",
          label: "Undo",
          icon: "Undo",
          shortcut: "mod+z",
          onclick: step("undo", api.edit.undo),
        },
        {
          kind: "item",
          label: "Redo",
          icon: "Redo",
          shortcut: "mod+shift+z",
          onclick: step("redo", api.edit.redo),
        },
        "divider",
        {
          kind: "item",
          label: "Split at playhead",
          icon: "Scissors",
          shortcut: "mod+b",
          disabled: splittable.length === 0,
          // One edit each, so undoing a multi-clip split takes as many steps
          // as it made. A single edit covering several clips is the engine's
          // call, not something to fake from here.
          onclick: () =>
            splittable.forEach((clip) =>
              attempt(api.edit.split(clip.id, playhead.frame)),
            ),
        },
        {
          kind: "item",
          label: many("Delete", targets.length),
          icon: "Trash2",
          shortcut: "delete",
          danger: true,
          disabled: targets.length === 0,
          onclick: () =>
            targets.forEach((clip) => attempt(api.edit.delete(clip.id, false))),
        },
        {
          kind: "item",
          label: many("Ripple delete", targets.length),
          icon: "Trash2",
          danger: true,
          disabled: targets.length === 0,
          onclick: () =>
            targets.forEach((clip) => attempt(api.edit.delete(clip.id, true))),
        },
      ],
    },
    {
      label: "Playback",
      items: [
        {
          kind: "item",
          label: playhead.playing ? "Pause" : "Play",
          icon: playhead.playing ? "Pause" : "Play",
          shortcut: "space",
          onclick: api.transport.toggle,
        },
        "divider",
        {
          kind: "item",
          label: "Go to start",
          icon: "SkipBack",
          onclick: () => api.transport.seek(0),
        },
        {
          kind: "item",
          label: "Go to end",
          icon: "SkipForward",
          disabled: !timeline,
          onclick: () => api.transport.seek(timeline?.length ?? 0),
        },
      ],
    },
  ]);

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
  <Titlebar {menus} />
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
