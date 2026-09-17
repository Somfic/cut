import api from "./api";
import { sync } from "./live";

/** Which file is open, and whether it has everything the session has done. */
class Project {
  name = $state("Untitled.cut");
  path = $state<string | null>(null);
  saved = $state(true);
}

export const project = new Project();

sync(api.project.get, api.projectEvents.onChanged, (p) => {
  project.name = p.name;
  project.path = p.path ?? null;
  project.saved = p.saved;
});
