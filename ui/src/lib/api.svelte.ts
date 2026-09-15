import { toast } from "glow";

import { Api, tauriRpc } from "./schema";

export type {
  ClipDto,
  ViewDto,
  StatsDto,
  TimelineDto,
  TrackDto,
  TransportDto,
} from "./schema";
export default new Api(tauriRpc());

/**
 * Show what an edit the engine refused had to say.
 *
 * Its messages are written for a person — "that would land on the clip at
 * frame 240" — so the useful thing to do with one is put it on screen rather
 * than swallow it or wrap it in something vaguer.
 */
export function attempt(edit: Promise<void>): void {
  edit.catch((e) => toast.error(String(e?.message ?? e)));
}

export class Playhead {
  frame = $state(0);
  /**
   * The same position, unrounded.
   *
   * A frame is a long time at a high zoom — 40 pixels of it — so a playhead
   * drawn on frame boundaries crawls across the screen in steps while the
   * display has four times as many frames to show it moving through. This is
   * what it is drawn at; `frame` is what edits and seeks are counted in.
   */
  exact = $state(0);
  playing = $state(false);
  fps = $state(24);

  #base = 0;
  #since = performance.now();

  /**
   * Where a scrub put the playhead, and how long to keep believing it over
   * what the engine says.
   */
  #asked: { frame: number; until: number } | null = null;

  /** How long the engine is given to catch up with a scrub. */
  static #PATIENCE = 1000;

  /** Fold in what Rust says. */
  sync(t: { playhead: number; playing: boolean; fps: number }) {
    this.fps = t.fps;
    this.playing = t.playing;

    // Seeking is asynchronous: playback publishes on a timer, and until it
    // has actually moved it keeps answering with where it still is. Taking
    // that during a scrub is what throws the playhead back for a frame
    // between one pointer event and the next.
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
   * Put the playhead where the user just dragged it, before the engine has
   * answered. A scrub is the one case where the front end knows first, and
   * waiting for the round trip is what makes one feel like it is trailing
   * the pointer.
   *
   * No start and end to a scrub, just this: each one renews the claim, so a
   * drag holds the playhead for as long as it lasts and lets go once the
   * engine arrives — or once it has plainly failed to.
   */
  scrub(frame: number) {
    this.#asked = { frame, until: performance.now() + Playhead.#PATIENCE };
    this.#base = frame;
    this.#since = performance.now();
    this.frame = frame;
    this.exact = frame;
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

export function timecode(
  frame: number,
  fps: number,
): { minutes: number; seconds: number; subseconds: number } {
  const total = Math.max(0, frame);
  const subseconds = Math.floor(total % fps);
  const seconds = Math.floor(total / fps);

  return {
    minutes: Math.floor(seconds / 60),
    seconds: seconds % 60,
    subseconds,
  };
}
