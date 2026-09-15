import api from "./api";

class Chrome {
  fullscreen = $state(false);
}

function live(): Chrome {
  const chrome = new Chrome();

  api.window.state().then((w) => (chrome.fullscreen = w.fullscreen));
  api.windowEvents.onChanged((w) => (chrome.fullscreen = w.fullscreen));

  return chrome;
}

export const chrome = live();
