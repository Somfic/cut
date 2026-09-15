export type {
  ClipDto,
  EdgeDto,
  ViewDto,
  StatsDto,
  TimelineDto,
  TrackDto,
  TransportDto,
} from "./schema";

import { toast } from "glow";
import { Api, tauriRpc } from "./schema";

const api = new Api(tauriRpc());
api.onError((error) => toast.error(error.message));

export default api;
