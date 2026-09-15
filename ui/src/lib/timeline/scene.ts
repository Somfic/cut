import { EasedNumber, wants_no_animations } from "../ease";
import type { ClipDto } from "../api";
import type { Placement } from "./geometry";

/** How long a clip takes to travel to where an edit put it. */
const GLIDE = 110;

export type Shown = {
  clip: ClipDto;
  track: number;
  position: EasedNumber;
  length: EasedNumber;
  lane: EasedNumber;
  present: EasedNumber;
  selected: EasedNumber;
  hovered: EasedNumber;
};

/**
 * What the canvas shows, which lags what the document says: an edit moves a
 * clip in one step and this walks it there. Entries outlive their clips, so a
 * deleted one fades out where it stood.
 */
export class Scene {
  readonly clips = new Map<number, Shown>();
  readonly fades = {
    snap: new EasedNumber(),
    carry: new EasedNumber(),
    head: new EasedNumber(0, 90),
  };

  /** Where the snap line was last seen, so it has somewhere to fade out at. */
  snapAt = 0;

  #painted = 0;

  show(clip: ClipDto, track: number, selected: boolean, hovered: boolean) {
    let it = this.clips.get(clip.id);

    if (!it) {
      // Only presence starts from nothing: a clip scrolling into view was
      // already selected and already where it is.
      it = {
        clip,
        track,
        position: new EasedNumber(clip.position, GLIDE),
        length: new EasedNumber(clip.length, GLIDE),
        lane: new EasedNumber(track, GLIDE),
        present: new EasedNumber(0),
        selected: new EasedNumber(+selected),
        hovered: new EasedNumber(+hovered),
      };
      this.clips.set(clip.id, it);
    }

    it.clip = clip;
    it.track = track;
    it.position.to(clip.position);
    it.length.to(clip.length);
    it.lane.to(track);
    it.present.to(1);
    it.selected.to(+selected);
    it.hovered.to(+hovered);
  }

  /** Leave clips where a gesture put them, with no journey to make. */
  settle(placements: Placement[]) {
    for (const placement of placements) {
      const it = this.clips.get(placement.id);
      if (!it) continue;

      it.position.set(placement.position);
      it.length.set(placement.length);
      it.lane.set(placement.track);
    }
  }

  /** Move everything on. True while any of it is still travelling. */
  advance(present: Set<number>): boolean {
    // A backgrounded tab hands back a vast delta; capped, so the first frame
    // back lands rather than leaps.
    const now = performance.now();
    const dt = wants_no_animations() ? 1e6 : Math.min(now - this.#painted, 100);
    this.#painted = now;

    let moving = Object.values(this.fades)
      .map((eased) => eased.advance(dt))
      .some(Boolean);

    for (const [id, it] of this.clips) {
      if (!present.has(id)) {
        it.present.to(0);
        if (it.present.value < 0.01) {
          this.clips.delete(id);
          continue;
        }
      }

      // Every one advanced: `||` would freeze whichever came second.
      moving =
        [it.position, it.length, it.lane, it.present, it.selected, it.hovered]
          .map((eased) => eased.advance(dt))
          .some(Boolean) || moving;
    }

    return moving;
  }
}
