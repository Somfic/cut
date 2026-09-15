<script lang="ts">
  import { Text } from 'glow'
  import Menubar, { type Menu } from './Menubar.svelte'

  // macOS draws the real title bar over this strip; this keeps clear of the
  // traffic lights and gives the window somewhere to be dragged by.
  let { title = 'cut', menus = [] }: { title?: string; menus?: Menu[] } =
    $props()
</script>

<header data-tauri-drag-region>
  <span data-tauri-drag-region>
    <Text as="span" size="sm" variant="secondary">{title}</Text>
  </span>
  <Menubar {menus} />
</header>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    /* Room for the traffic lights, which the system puts over this strip. */
    padding: 0 12px 0 78px;
    height: 32px;
    background: var(--glow-bg-base);
    border-bottom: 1px solid var(--glow-border-color);
    flex: none;
  }
</style>
