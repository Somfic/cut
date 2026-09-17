import api, { type TimelineDto } from "./api";
import { sync } from "./live";

/** The document, as the engine last sent it. */
class Document {
  timeline = $state<TimelineDto | null>(null);

  /** Every clip, flattened, which is what most commands want. */
  get clips() {
    return this.timeline?.tracks.flatMap((track) => track.clips) ?? [];
  }

  get length() {
    return this.timeline?.length ?? 0;
  }
}

export const document = new Document();

sync(api.timeline.get, api.timelineEvents.onChanged, (t) => {
  document.timeline = t;
});
