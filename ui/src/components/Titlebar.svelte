<script lang="ts">
  import { Text } from "glow";
  import Menubar from "./Menubar.svelte";
  import { chrome } from "$lib/window.svelte";

  let { title = "cut" }: { title?: string } = $props();
</script>

<header data-tauri-drag-region class:lights={!chrome.fullscreen}>
  <Text as="span" size="sm" variant="secondary">{title}</Text>
  <Menubar />
</header>

<style lang="scss">
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 32px;
    padding: 0 12px;
    background: var(--glow-bg-base);
    border-bottom: 1px solid var(--glow-border-color);
    flex: none;

    &.lights {
      padding-left: 80px;
    }
  }
</style>
