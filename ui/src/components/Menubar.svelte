<script lang="ts">
  import { PopoverMenu, rovingFocus } from "glow";

  import { document } from "$lib/document.svelte";
  import { build_menu } from "$lib/menu";
  import { playhead } from "$lib/playhead.svelte";
  import { selection } from "$lib/selection.svelte";
  import { covers } from "$lib/timeline/geometry";

  const selected_clips = $derived.by(() => {
    const picked = document.clips.filter((clip) => selection.has(clip.id));
    if (picked.length) return picked;

    const under = document.clips.find((clip) => covers(clip, playhead.frame));
    return under ? [under] : [];
  });

  const playhead_clips = $derived(
    selected_clips.filter(
      (clip) => playhead.frame > clip.position && covers(clip, playhead.frame),
    ),
  );

  const menu = $derived(
    build_menu({
      timeline: document.timeline,
      all_clips: document.clips,
      selected_clips: selected_clips,
      playhead_clips: playhead_clips,
      current_frame: playhead.frame,
      is_playing: playhead.playing,
    }),
  );

  let open_submenu_index = $state<number | null>(null);

  const hover = (i: number) => {
    if (open_submenu_index !== null) open_submenu_index = i;
  };
</script>

<nav use:rovingFocus={{ orientation: "horizontal" }}>
  {#each menu as submenu, i (submenu.label)}
    <PopoverMenu
      items={submenu.items}
      bindShortcuts
      fullWidth={false}
      align="left"
      offset={2}
      bind:open={
        () => open_submenu_index === i,
        (v) =>
          (open_submenu_index = v
            ? i
            : open_submenu_index === i
              ? null
              : open_submenu_index)
      }
    >
      {#snippet trigger()}
        <button
          type="button"
          class="menu"
          class:open={open_submenu_index === i}
          data-roving-item
          onmouseenter={() => hover(i)}
        >
          {submenu.label}
        </button>
      {/snippet}
    </PopoverMenu>
  {/each}
</nav>

<style lang="scss">
  nav {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .menu {
    appearance: none;
    border: none;
    background: none;
    border-radius: 4px;
    padding: 3px 8px;
    font: inherit;
    font-size: 13px;
    color: var(--glow-text-secondary);
    cursor: pointer;
  }

  .menu:hover,
  .menu:focus-visible {
    background: color-mix(in oklab, var(--glow-fg) 8%, transparent);
    color: var(--glow-fg);
  }

  .menu.open {
    background: color-mix(in oklab, var(--glow-fg) 12%, transparent);
    color: var(--glow-fg);
  }
</style>
