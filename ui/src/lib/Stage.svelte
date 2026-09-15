<script lang="ts">
  import api from "./api.svelte";

  let stage: HTMLDivElement;

  function report() {
    const rect = stage.getBoundingClientRect();
    const scale = window.devicePixelRatio;

    return api.surface.setStage(
      rect.x * scale,
      rect.y * scale,
      rect.width * scale,
      rect.height * scale,
      window.innerWidth * scale,
      window.innerHeight * scale,
    );
  }

  $effect(() => {
    // when the window resizes
    window.addEventListener("resize", report);

    // when the element resizes
    const observer = new ResizeObserver(report);
    observer.observe(stage);

    // once on mount
    report();

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
    };
  });
</script>

<div class="stage" bind:this={stage}></div>

<style lang="scss">
  .stage {
    flex: 1;
    min-height: 0;
  }
</style>
