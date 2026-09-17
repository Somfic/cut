<script lang="ts">
  import { Button, Text } from "glow";

  import api, { type TrackDto } from "$lib/api";
  import {
    RULER_HEIGHT,
    TRACK_GAP,
    TRACK_HEIGHT,
  } from "$lib/timeline/geometry";

  let { tracks }: { tracks: TrackDto[] } = $props();
</script>

<!-- Laid out from the constants the canvas paints with, so a header always
     sits against its own lane. -->
<aside
  style="--ruler: {RULER_HEIGHT}px; --height: {TRACK_HEIGHT}px; --gap: {TRACK_GAP}px"
>
  <div class="ruler"></div>

  {#each tracks as track, i (i)}
    <div class="track" class:off={!track.enabled}>
      <Text
        as="span"
        size="sm"
        variant={track.enabled ? "primary" : "secondary"}
      >
        Track {i + 1}
      </Text>

      <Button
        size="sm"
        variant="ghost"
        icon={track.enabled ? "Eye" : "EyeOff"}
        tooltip={track.enabled ? "Disable track" : "Enable track"}
        onclick={() => api.edit.enableTrack(i, !track.enabled)}
      />
    </div>
  {/each}
</aside>

<style lang="scss">
  aside {
    flex: none;
    width: 150px;
    border-right: 1px solid var(--glow-border-color);
    background: var(--glow-bg-base);
    overflow: hidden;
  }

  .ruler {
    height: var(--ruler);
  }

  .track {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    height: var(--height);
    margin-top: var(--gap);
    padding: 0 6px 0 10px;
    border-radius: 4px;
    background: var(--glow-bg-surface);

    /* The canvas fades its clips over the same 150ms, so the header and its
       lane go together. */
    transition: opacity 150ms ease;

    &.off {
      opacity: 0.55;
    }

    @media (prefers-reduced-motion: reduce) {
      transition: none;
    }
  }
</style>
