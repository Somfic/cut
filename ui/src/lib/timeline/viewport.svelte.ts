import api from "../api";
import { EasedNumber, wants_no_animations } from "../ease";
import { MAX_ZOOM, MIN_ZOOM } from "./geometry";

/** How long the view has to hold still before the engine is told about it. */
const SETTLED = 200;

/** How quickly a glide covers the ground between two views. */
const GLIDE = 130;

/** Shared rather than held in the canvas, because undo restores it. */
class Viewport {
  scroll = $state(0);
  zoom = $state(0.2);

  /** Panning by hand turns this off: the hand wins that argument. */
  following = $state(false);

  #mark: ReturnType<typeof setTimeout> | null = null;
  #frame: number | null = null;

  /** Arrive now: what a pan, a zoom or a fit does. */
  to(scroll: number, zoom: number) {
    this.#stop();
    this.#set(scroll, zoom);

    // The engine reads this only when an edit is recorded.
    if (this.#mark) clearTimeout(this.#mark);
    this.#mark = setTimeout(() => {
      this.#mark = null;
      api.view.mark(this.scroll, this.zoom);
    }, SETTLED);
  }

  /**
   * Travel there, for a jump the user did not make by hand. Zoom eases in log
   * space: a third of the way from 1 to 8 is 2, not 3.3.
   */
  glide(scroll: number, zoom: number) {
    this.#stop();

    const target = { scroll, zoom: this.#clamp(zoom) };
    if (wants_no_animations()) return this.to(target.scroll, target.zoom);

    const pan = new EasedNumber(this.scroll, GLIDE);
    const magnify = new EasedNumber(Math.log(this.zoom), GLIDE);
    pan.to(target.scroll);
    magnify.to(Math.log(target.zoom));

    let painted = performance.now();
    const travel = () => {
      const now = performance.now();
      const dt = Math.min(now - painted, 100);
      painted = now;

      // Both advanced: `||` would freeze whichever came second.
      const moving = [pan.advance(dt), magnify.advance(dt)].some(Boolean);
      this.#set(pan.value, Math.exp(magnify.value));

      if (moving) {
        this.#frame = requestAnimationFrame(travel);
        return;
      }

      this.#frame = null;
      this.to(target.scroll, target.zoom);
    };

    this.#frame = requestAnimationFrame(travel);
  }

  #stop() {
    if (this.#frame === null) return;

    cancelAnimationFrame(this.#frame);
    this.#frame = null;
  }

  #set(scroll: number, zoom: number) {
    this.scroll = scroll;
    this.zoom = this.#clamp(zoom);
  }

  #clamp = (zoom: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

export const viewport = new Viewport();
