import { toast, type PopoverMenuEntry } from "glow";
import api, { type ClipDto, type TimelineDto, type ViewDto } from "./api";
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
          shortcut: "delete",
          danger: true,
          disabled: c.selected_clips.length === 0,
          onclick: () => api.edit.delete(ids_of(c.selected_clips), false),
        },
        {
          kind: "item",
          label: clips("Ripple delete", c.selected_clips.length),
          icon: "Trash2",
          danger: true,
          disabled: c.selected_clips.length === 0,
          onclick: () => api.edit.delete(ids_of(c.selected_clips), true),
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
          onclick: () => api.transport.seek(0),
        },
        {
          kind: "item",
          label: "Go to end",
          icon: "SkipForward",
          disabled: !c.timeline,
          onclick: () => api.transport.seek(c.timeline?.length ?? 0),
        },
      ],
    },
  ];
}
