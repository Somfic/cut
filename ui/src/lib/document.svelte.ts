import api, { type TimelineDto } from "./api";

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

function live(): Document {
  const document = new Document();

  // Pushed on every edit; this call is only the fetch before the first event.
  api.timeline.get().then((t) => (document.timeline = t));
  api.timelineEvents.onChanged((t) => (document.timeline = t));

  return document;
}

export const document = live();
