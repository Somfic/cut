import api from "./api";
import { sync } from "./live";

class Chrome {
  fullscreen = $state(false);
}

export const chrome = new Chrome();

sync(api.window.state, api.windowEvents.onChanged, (w) => {
  chrome.fullscreen = w.fullscreen;
});
