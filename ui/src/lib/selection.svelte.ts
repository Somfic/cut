import { SvelteSet } from "svelte/reactivity";

class Selection {
  #ids = new SvelteSet<number>();

  get ids(): ReadonlySet<number> {
    return this.#ids;
  }

  get size(): number {
    return this.#ids.size;
  }

  has(id: number): boolean {
    return this.#ids.has(id);
  }

  set(id: number) {
    this.#ids.clear();
    this.#ids.add(id);
  }

  all(ids: Iterable<number>) {
    this.#ids.clear();
    for (const id of ids) this.#ids.add(id);
  }

  toggle(id: number) {
    if (!this.#ids.delete(id)) this.#ids.add(id);
  }

  clear() {
    this.#ids.clear();
  }

  /** An id is never handed out twice, so a stale one targets nothing. */
  keep(present: Iterable<number>) {
    const alive = new Set(present);
    for (const id of this.#ids) if (!alive.has(id)) this.#ids.delete(id);
  }
}

export const selection = new Selection();
