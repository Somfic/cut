import { toast, type PopoverMenuEntry } from "glow";
import api, {
  type ClipDto,
  type EdgeDto,
  type TimelineDto,
  type ViewDto,
} from "./api";
import { mode } from "./mode.svelte";
import { playhead } from "./playhead.svelte";
import { selection } from "./selection.svelte";
import { viewport } from "./timeline/viewport.svelte";

export type Menu = { label: string; items: PopoverMenuEntry[] };

export type MenuContext = {
  timeline: TimelineDto | null;
  all_clips: ClipDto[];
  selected_clips: ClipDto[];
  playhead_clips: ClipDto[];
  current_frame: number;
  is_playing: boolean;
};

const ids_of = (clips: ClipDto[]) => clips.map((clip) => clip.id);

const clips = (what: string, n: number) =>
  n > 1 ? `${what} ${n} clips` : `${what} clip`;

const soon = (what: string) => () => toast.info(`${what} isn't wired up yet`);

const step = (what: string, go: () => Promise<ViewDto | null>) => async () => {
  const view = await go();
  if (!view) return toast.info(`Nothing to ${what}`);

  viewport.glide(view.scroll, view.zoom);
  api.transport.seek(view.playhead);
};

/** A ripple closes one gap, so it acts on one clip under the playhead. */
/** One clip: the selected one under the playhead, or whichever is under it. */
const trim_target = (c: MenuContext) =>
  c.playhead_clips.find((clip) => c.selected_clips.includes(clip)) ??
  c.playhead_clips[0];

/**
 * Trim an edge to the playhead, closing the gap or leaving it depending on
 * the mode. Rippling is one clip's business either way: it closes one gap,
 * and two edit points give two answers for how far the rest should move.
 */
function trim(c: MenuContext, edge: EdgeDto) {
  const clip = trim_target(c);
  if (!clip) return;

  if (!mode.ripple) {
    api.edit.trim([{ clip: clip.id, edge, frame: c.current_frame }]);
    return;
  }

  api.edit.ripple(clip.id, edge, c.current_frame).then(
    () => edge === "in" && playhead.seek(clip.position),
    () => {},
  );
}

export function build_menu(c: MenuContext): Menu[] {
  return [
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
              label: "From folder…",
              icon: "Folder",
              onclick: soon("Import from folder"),
            },
            {
              kind: "item",
              label: "From file…",
              icon: "File",
              shortcut: "mod+i",
              onclick: soon("Import from file"),
            },
          ],
        },
        "divider",
        {
          kind: "item",
          label: "Export…",
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
          label: "Split clip",
          icon: "Scissors",
          shortcut: "s",
          disabled: c.playhead_clips.length === 0,
          onclick: () => {
            api.edit.split(ids_of(c.playhead_clips), c.current_frame);
          },
        },
        {
          kind: "item",
          label: "Trim start",
          icon: "ArrowLeftToLine",
          shortcut: "q",
          disabled: !trim_target(c),
          onclick: () => trim(c, "in"),
        },
        {
          kind: "item",
          label: "Trim end",
          icon: "ArrowRightToLine",
          shortcut: "w",
          disabled: !trim_target(c),
          onclick: () => trim(c, "out"),
        },
        "divider",
        {
          kind: "item",
          label: "Select all",
          icon: "SquareDashedMousePointer",
          shortcut: "mod+a",
          disabled: c.all_clips.length === 0,
          onclick: () => selection.all(ids_of(c.all_clips)),
        },
        "divider",
        {
          kind: "item",
          label: clips("Delete", c.selected_clips.length),
          icon: "Trash2",
          shortcut: "x",
          danger: true,
          disabled: c.selected_clips.length === 0,
          onclick: () => api.edit.delete(ids_of(c.selected_clips), mode.ripple),
        },
      ],
    },
    {
      label: "Playback",
      items: [
        {
          kind: "item",
          label: c.is_playing ? "Pause" : "Play",
          icon: c.is_playing ? "Pause" : "Play",
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
          onclick: () => playhead.seek(0),
        },
        {
          kind: "item",
          label: "Go to end",
          icon: "SkipForward",
          disabled: !c.timeline,
          onclick: () => playhead.seek(c.timeline?.length ?? 0),
        },
      ],
    },
  ];
}
