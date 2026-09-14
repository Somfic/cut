<script lang="ts">
  import { Text } from 'glow'

  // macOS draws the real title bar over this strip; this keeps clear of the
  // traffic lights and gives the window somewhere to be dragged by.
  let { title = 'cut' }: { title?: string } = $props()
</script>

<header data-tauri-drag-region>
  <span data-tauri-drag-region>
    <Text as="span" size="sm" variant="secondary">{title}</Text>
  </span>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    /* Room for the traffic lights, which the system puts over this strip. */
    padding: 0 12px 0 78px;
    height: 32px;
    background: var(--glow-bg-base);
    border-bottom: 1px solid var(--glow-border-color);
    flex: none;
  }
</style>
