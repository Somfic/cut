<script lang="ts">
  import { setVideoRect } from './engine.svelte'

  // The hole: it reserves the rect and stays transparent. Anything painted
  // here would cover the video surface behind it.
  let hole: HTMLDivElement

  $effect(() => {
    const report = () => setVideoRect(hole)
    const observer = new ResizeObserver(report)

    observer.observe(hole)
    // The rect moves when anything above it resizes, not only the hole.
    window.addEventListener('resize', report)
    report()

    return () => {
      observer.disconnect()
      window.removeEventListener('resize', report)
    }
  })
</script>

<div class="stage" bind:this={hole}></div>

<style>
  .stage { flex: 1; min-height: 0; }
</style>
