<script lang="ts">
  import { AnimatedValue, Button, registerShortcut, Root, Text, toast } from "glow";
  import Titlebar from "./lib/Titlebar.svelte";
  import Stage from "./lib/Stage.svelte";
  import Timeline from "./lib/Timeline.svelte";
  import { type Menu } from "./lib/Menubar.svelte";
  import { selection } from "./lib/selection.svelte";
  import { viewport } from "./lib/viewport.svelte";
  import api, {
    attempt,
    Playhead,
    timecode,
    type ClipDto,
    type StatsDto,
    type ViewDto,
    type TimelineDto,
  } from "./lib/api.svelte";
  import { inside } from "./lib/timeline";

  const playhead = new Playhead();

  let timeline = $state<TimelineDto | null>(null);
  let readout = $state("");

  // Nothing behind the file commands yet — the menu is the shape of the API
  // the engine still has to grow.
  const soon = (what: string) => () => toast.info(`${what} isn't wired up yet`);

  /**
   * Undo and redo hand back where the timeline was when that version was
   * current, so going back a step goes back to the part of the document the
   * step was about rather than leaving you to find it.
   */
  const step =
    (what: string, go: () => Promise<ViewDto | null>) => async () => {
      const view = await go();
      if (!view) return toast.info(`Nothing to ${what}`);

      viewport.glide(view.scroll, view.zoom);
      api.transport.seek(view.playhead);
    };

  const clips = $derived(
    timeline?.tracks.flatMap((track) => track.clips) ?? [],
  );

  // What an edit command acts on: what is selected, or the clip under the
  // playhead when nothing is. Same rule for every command, so the menu never
  // disagrees with itself about what "the clip" means.
  const targets = $derived.by(() => {
    const selected = clips.filter((clip) => selection.has(clip.id));
    if (selected.length) return selected;

    const under = clips.find((clip) => inside(clip, playhead.frame));
    return under ? [under] : [];
  });

  // A cut lands between two frames, so the playhead being on a clip's first
  // frame is not the playhead being inside it.
  const splittable = $derived(
    targets.filter(
      (clip) => playhead.frame > clip.position && inside(clip, playhead.frame),
    ),
  );

  const many = (what: string, n: number) =>
    n > 1 ? `${what} ${n} clips` : `${what} clip`;

  const ids = (clips: ClipDto[]) => clips.map((clip) => clip.id);

  const remove = (ripple: boolean) => attempt(api.edit.delete(ids(targets), ripple));

  // Backspace does what Delete does. The menu row can only print one of them,
  // and printing the one this keyboard has a key for is the less useful half.
  $effect(() =>
    registerShortcut("backspace", () => remove(false), {
      disabled: targets.length === 0,
    }),
  );

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
        {
          kind: "item",
          label: "Split at playhead",
          icon: "Scissors",
          shortcut: "mod+b",
          disabled: splittable.length === 0,
          onclick: () =>
            attempt(api.edit.split(ids(splittable), playhead.frame)),
        },
        "divider",
        {
          kind: "item",
          label: "Select all",
          icon: "SquareDashedMousePointer",
          shortcut: "mod+a",
          disabled: clips.length === 0,
          onclick: () => selection.all(ids(clips)),
        },
        "divider",
        {
          kind: "item",
          label: many("Delete", targets.length),
          icon: "Trash2",
          shortcut: "delete",
          danger: true,
          disabled: targets.length === 0,
          onclick: () => remove(false),
        },
        {
          kind: "item",
          label: many("Ripple delete", targets.length),
          icon: "Trash2",
          danger: true,
          disabled: targets.length === 0,
          onclick: () => remove(true),
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
        {
          kind: "toggle",
          label: "Follow playhead",
          description: "Keep it centred and move the timeline past it",
          checked: viewport.following,
          onChange: (on) => (viewport.following = on),
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

  <Timeline
    {timeline}
    playhead={playhead.frame}
    exact={playhead.exact}
    playing={playhead.playing}
    fps={playhead.fps}
    onScrub={(frame) => playhead.scrub(frame)}
  />
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
