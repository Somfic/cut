// The only place that talks to Rust. Per-edit state is pulled on demand;
// per-frame state is predicted here and corrected occasionally, so a scrub
// never waits on IPC.

const { invoke } = (window as any).__TAURI__.core

export type { Clip, Track } from './timeline'
export type TimelineDto = import('./timeline').Timeline
export type TransportDto = { playhead: number; playing: boolean; fps: number }
export type Stats = { frames: number; presents: number; dropped: number }

export const getTimeline = () => invoke('timeline') as Promise<TimelineDto | null>
export const getTransport = () => invoke('transport') as Promise<TransportDto>
export const getStats = () => invoke('stats') as Promise<Stats>
export const togglePlayback = () => invoke('toggle_playback') as Promise<void>
export const seek = (frame: number) => invoke('seek', { frame }) as Promise<void>

/**
 * Tell Rust what to clear the surface to, so the area around the letterboxed
 * video matches the chrome. Probed rather than read from the token, which can
 * be any colour expression.
 */
export function syncChrome() {
  const probe = document.createElement('div')
  probe.style.cssText = 'position:fixed;visibility:hidden;background:var(--glow-bg-base)'
  document.body.appendChild(probe)

  const [r, g, b] = getComputedStyle(probe)
    .backgroundColor.match(/[\d.]+/g)!
    .slice(0, 3)
    .map(Number)

  probe.remove()

  return invoke('set_chrome', { r: r / 255, g: g / 255, b: b / 255 }) as Promise<void>
}

/** Tell Rust where on the surface to put the video, in device pixels. */
export function setVideoRect(el: HTMLElement) {
  const r = el.getBoundingClientRect()
  const s = window.devicePixelRatio

  return invoke('set_video_rect', {
    x: r.x * s,
    y: r.y * s,
    width: r.width * s,
    height: r.height * s,
    // So Rust can check the two still share a coordinate space.
    pageWidth: window.innerWidth * s,
    pageHeight: window.innerHeight * s,
  }) as Promise<void>
}

/**
 * The playhead, run locally: it advances on its own clock and takes a
 * correction a few times a second, so display-rate updates cost no IPC.
 */
export class Playhead {
  frame = $state(0)
  playing = $state(false)
  fps = $state(24)

  #base = 0
  #since = performance.now()

  /** Fold in what Rust says. */
  sync(t: TransportDto) {
    this.fps = t.fps
    this.playing = t.playing
    this.#base = t.playhead
    this.#since = performance.now()
    this.frame = t.playhead
  }

  /** Advance to now. Call once per animation frame. */
  tick() {
    if (!this.playing) return
    const elapsed = (performance.now() - this.#since) / 1000
    this.frame = this.#base + Math.floor(elapsed * this.fps)
  }
}

export function timecode(frame: number, fps: number): { minutes: number, seconds: number, subseconds: number} {
  const total = Math.max(0, frame)
  const f = Math.floor(total % fps)
  const seconds = Math.floor(total / fps)

  return { minutes: Math.floor(seconds / 60), seconds: seconds % 60, subseconds: f };
}
