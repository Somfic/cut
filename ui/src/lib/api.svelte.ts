import { Api, tauriRpc } from "./schema";

export type {
  ClipDto,
  StatsDto,
  TimelineDto,
  TrackDto,
  TransportDto,
} from "./schema";
export default new Api(tauriRpc());

export class Playhead {
  frame = $state(0);
  playing = $state(false);
  fps = $state(24);

  #base = 0;
  #since = performance.now();

  /** Fold in what Rust says. */
  sync(t: { playhead: number; playing: boolean; fps: number }) {
    this.fps = t.fps;
    this.playing = t.playing;
    this.#base = t.playhead;
    this.#since = performance.now();
    this.frame = t.playhead;
  }

  /** Advance to now. Call once per animation frame. */
  tick() {
    if (!this.playing) return;
    const elapsed = (performance.now() - this.#since) / 1000;
    this.frame = this.#base + Math.floor(elapsed * this.fps);
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
