import api from "./api";
import { sync } from "./live";

class Playhead {
  frame = $state(0);
  /** Unrounded, for drawing: a frame can be forty pixels wide. */
  exact = $state(0);
  playing = $state(false);
  fps = $state(24);

  #base = 0;
  #since = performance.now();

  /** Where a scrub put it, and how long to believe that over the engine. */
  #asked: { frame: number; until: number } | null = null;

  /** How long the engine is given to catch up with a scrub. */
  static #PATIENCE = 1000;

  /** Fold in what Rust says. */
  sync(t: { playhead: number; playing: boolean; fps: number }) {
    this.fps = t.fps;
    this.playing = t.playing;

    // Seeking is asynchronous: until playback moves it answers with where it
    // still is, which throws a scrub back a frame at a time.
    if (this.#asked) {
      const arrived = Math.abs(t.playhead - this.#asked.frame) <= 2;
      if (!arrived && performance.now() < this.#asked.until) return;

      this.#asked = null;
    }

    this.#base = t.playhead;
    this.#since = performance.now();
    this.frame = t.playhead;
    this.exact = t.playhead;
  }

  /**
   * Where the user just dragged it, before the engine has answered. Each call
   * renews the claim, so a drag holds it for as long as it lasts.
   */
  scrub(frame: number) {
    // Moving it by hand takes over from playback, rather than fighting it.
    if (this.playing) {
      this.playing = false;
      api.transport.pause();
    }

    this.#asked = { frame, until: performance.now() + Playhead.#PATIENCE };
    this.#base = frame;
    this.#since = performance.now();
    this.frame = frame;
    this.exact = frame;
  }

  /** Both halves: the engine, which decodes, and the clock that is drawn. */
  seek(frame: number) {
    this.scrub(frame);
    api.transport.seek(frame);
  }

  /** Advance to now. Call once per animation frame. */
  tick() {
    if (!this.playing) return;

    const elapsed = (performance.now() - this.#since) / 1000;
    const at = this.#base + elapsed * this.fps;

    this.exact = at;
    this.frame = Math.floor(at);
  }
}

export const playhead = new Playhead();

// The clock runs whether or not anything is watching it, and every correction
// the engine pushes lands in one place.
sync(api.transport.state, api.transportEvents.onChanged, (t) => playhead.sync(t));

const tick = () => {
  playhead.tick();
  requestAnimationFrame(tick);
};
requestAnimationFrame(tick);
