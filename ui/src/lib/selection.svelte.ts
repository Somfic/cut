import { SvelteSet } from 'svelte/reactivity'

/**
 * Which clips a command acts on.
 *
 * Ids rather than clips: everything on screen is re-sent whole on every edit,
 * so a held clip would be a stale copy the moment anything moved. `ClipId`
 * outlives an edit by design — that is what it is for.
 *
 * One instance for the app, imported where it is needed. Selection is read by
 * the canvas and by the menus, which have no other relationship; threading it
 * through props would put it in the way of everything in between.
 */
class Selection {
  #ids = new SvelteSet<number>()

  get ids(): ReadonlySet<number> {
    return this.#ids
  }

  get size(): number {
    return this.#ids.size
  }

  has(id: number): boolean {
    return this.#ids.has(id)
  }

  /** Replace the selection with one clip. */
  set(id: number) {
    this.#ids.clear()
    this.#ids.add(id)
  }

  /** Replace the selection wholesale, the way select-all does. */
  all(ids: Iterable<number>) {
    this.#ids.clear()
    for (const id of ids) this.#ids.add(id)
  }

  /** Add or remove one, the way a shift-click does. */
  toggle(id: number) {
    if (!this.#ids.delete(id)) this.#ids.add(id)
  }

  clear() {
    this.#ids.clear()
  }

  /**
   * Drop whatever is no longer in the document. A deleted clip's id is never
   * handed out again, so a selection that kept it would silently target
   * nothing for the rest of the session.
   */
  keep(present: Iterable<number>) {
    const alive = new Set(present)
    for (const id of this.#ids) if (!alive.has(id)) this.#ids.delete(id)
  }
}

export const selection = new Selection()
