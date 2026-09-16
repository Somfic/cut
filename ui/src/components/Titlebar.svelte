<script lang="ts">
  import { Text } from "glow";
  import Menubar from "./Menubar.svelte";
  import { project } from "$lib/project.svelte";
  import { chrome } from "$lib/window.svelte";

  let { title }: { title?: string } = $props();
  const shown = $derived(title ?? project.name);
</script>

<header data-tauri-drag-region class:lights={!chrome.fullscreen}>
  <Text as="span" size="sm" variant="secondary">
    {shown}
    {#if !project.saved}
      <span class="unsaved" title="Unsaved changes"> • </span>
    {/if}
  </Text>
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

  .unsaved {
    margin-left: 4px;
  }
</style>
