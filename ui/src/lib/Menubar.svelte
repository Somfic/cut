<script lang="ts" module>
  import type { PopoverMenuEntry } from "glow";

  export type Menu = { label: string; items: PopoverMenuEntry[] };
</script>

<script lang="ts">
  import { PopoverMenu, rovingFocus } from "glow";

  let { menus }: { menus: Menu[] } = $props();

  let openIndex = $state<number | null>(null);

  const hover = (i: number) => {
    if (openIndex !== null) openIndex = i;
  };
</script>

<nav use:rovingFocus={{ orientation: "horizontal" }}>
  {#each menus as menu, i (menu.label)}
    <PopoverMenu
      items={menu.items}
      bindShortcuts
      fullWidth={false}
      align="left"
      offset={2}
      bind:open={
        () => openIndex === i,
        (v) => (openIndex = v ? i : openIndex === i ? null : openIndex)
      }
    >
      {#snippet trigger()}
        <button
          type="button"
          class="menu"
          class:open={openIndex === i}
          data-roving-item
          onmouseenter={() => hover(i)}
        >
          {menu.label}
        </button>
      {/snippet}
    </PopoverMenu>
  {/each}
</nav>

<style>
  nav {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  /* Glow's own tokens: invented names here would silently fall back to their
     literals and stop following the theme. */
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
