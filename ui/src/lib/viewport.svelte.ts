import api from './api.svelte'
import { Eased, stillness } from './animate'
import { MAX_ZOOM, MIN_ZOOM } from './timeline'

/** How long the view has to hold still before the engine is told about it. */
const SETTLED = 200

/** How quickly a glide covers the ground between two views. */
const GLIDE = 130

/**
 * Where the timeline is looking: the frame at the left edge, and how many
 * pixels a frame is worth.
 *
 * Shared rather than held in the canvas, because undo restores it — the
 * version the engine hands back carries the view it was recorded with, and
 * that arrives at the menu, not at the canvas.
 */
class Viewport {
  scroll = $state(0)
  zoom = $state(0.2)

  /**
   * Keep the playhead in the middle and move the document under it.
   *
   * Lives here rather than in the canvas because it is a property of the
   * view, and because the menu that switches it on is nowhere near the
   * canvas. Panning by hand turns it off — the two are the same argument
   * about where the view should be, and the hand wins it.
   */
  following = $state(false)

  #mark: ReturnType<typeof setTimeout> | null = null
  #frame: number | null = null

  /** Put the view where it belongs now: what a pan, a zoom or a fit does. */
  to(scroll: number, zoom: number) {
    this.#stop()
    this.#set(scroll, zoom)

    // Debounced: the engine reads this when an edit is recorded, so a value
    // per frame of a pan would be a value per frame thrown away.
    if (this.#mark) clearTimeout(this.#mark)
    this.#mark = setTimeout(() => {
      this.#mark = null
      api.view.mark(this.scroll, this.zoom)
    }, SETTLED)
  }

  /**
   * Travel to a view over the next few frames, rather than arriving in one.
   *
   * For a jump the user did not make with their own hand — undo landing in
   * another part of the document — where a cut would leave them working out
   * what moved. Zoom travels in log space: a third of the way from 1 to 8 is
   * 2, not 3.3, which is what makes a zoom feel even rather than front-loaded.
   */
  glide(scroll: number, zoom: number) {
    this.#stop()

    const target = { scroll, zoom: this.#clamp(zoom) }
    if (stillness()) return this.to(target.scroll, target.zoom)

    const pan = new Eased(this.scroll, GLIDE)
    const magnify = new Eased(Math.log(this.zoom), GLIDE)
    pan.to(target.scroll)
    magnify.to(Math.log(target.zoom))

    let painted = performance.now()
    const travel = () => {
      const now = performance.now()
      const dt = Math.min(now - painted, 100)
      painted = now

      // Both advanced, not one or the other: `||` would leave whichever came
      // second frozen for a frame whenever the first had settled.
      const moving = [pan.advance(dt), magnify.advance(dt)].some(Boolean)
      this.#set(pan.value, Math.exp(magnify.value))

      if (moving) {
        this.#frame = requestAnimationFrame(travel)
        return
      }

      this.#frame = null
      this.to(target.scroll, target.zoom)
    }

    this.#frame = requestAnimationFrame(travel)
  }

  /** Whatever was travelling is no longer where the view is going. */
  #stop() {
    if (this.#frame === null) return

    cancelAnimationFrame(this.#frame)
    this.#frame = null
  }

  #set(scroll: number, zoom: number) {
    this.scroll = scroll
    this.zoom = this.#clamp(zoom)
  }

  #clamp = (zoom: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom))
}

export const viewport = new Viewport()
