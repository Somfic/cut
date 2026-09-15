<script lang="ts" module>
  import type { PopoverMenuEntry } from 'glow'

  export type Menu = { label: string; items: PopoverMenuEntry[] }
</script>

<script lang="ts">
  import { PopoverMenu, rovingFocus } from 'glow'

  let { menus }: { menus: Menu[] } = $props()

  // `bindShortcuts` below makes each item's spec a live accelerator, so the
  // keys work without the menu being opened first — the bar is the place they
  // are documented, not the place they are handled.

  // Which menu is down, if any. Held here rather than per-menu so the bar can
  // behave like a menu bar: once one is open, hovering a sibling switches to
  // it instead of needing a second click.
  let openIndex = $state<number | null>(null)

  const hover = (i: number) => {
    if (openIndex !== null) openIndex = i
  }
</script>

<nav use:rovingFocus={{ orientation: 'horizontal' }}>
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

  .menu {
    appearance: none;
    border: none;
    background: none;
    border-radius: var(--glow-radius-sm, 4px);
    padding: 3px 8px;
    font: inherit;
    font-size: var(--glow-text-sm, 13px);
    color: var(--glow-fg-secondary, inherit);
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
