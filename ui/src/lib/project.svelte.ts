import api, { type ProjectDto } from "./api";

/** Which file is open, and whether it has everything the session has done. */
class Project {
  name = $state("Untitled");
  path = $state<string | null>(null);
  saved = $state(true);
}

function live(): Project {
  const project = new Project();

  const take = (p: ProjectDto) => {
    project.name = p.name;
    project.path = p.path ?? null;
    project.saved = p.saved;
  };

  api.project.get().then(take);
  api.projectEvents.onChanged(take);

  return project;
}

export const project = live();
