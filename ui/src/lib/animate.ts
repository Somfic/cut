/**
 * Easing for things drawn on a canvas, where CSS transitions do not reach.
 *
 * Exponential rather than a duration and a curve: a value can be re-aimed
 * mid-flight — a selection toggled twice, a snap that jumps to another edge —
 * and this just bends toward the new target from wherever it is, with no
 * bookkeeping about when the last transition started.
 */
export class Eased {
  #value: number
  #target: number
  /** Milliseconds to cover about two thirds of the remaining distance. */
  #tau: number

  constructor(value = 0, tau = 70) {
    this.#value = value
    this.#target = value
    this.#tau = tau
  }

  get value(): number {
    return this.#value
  }

  /** Ease toward `target` from wherever the value is now. */
  to(target: number) {
    this.#target = target
  }

  /** Jump, for a value that should not be seen travelling. */
  set(value: number) {
    this.#value = value
    this.#target = value
  }

  /** Advance by `dt` milliseconds. True while there is still ground to cover. */
  advance(dt: number): boolean {
    const remaining = this.#target - this.#value

    // Below a quarter of a percent nothing on screen would change, and
    // chasing it forever would keep the animation loop awake.
    if (Math.abs(remaining) < 0.0025) {
      this.#value = this.#target
      return false
    }

    this.#value += remaining * (1 - Math.exp(-dt / this.#tau))
    return true
  }
}

/**
 * Whether the viewer asked for less movement. Read per call rather than
 * cached: it is only consulted when something starts, and a preference
 * changed mid-session should take effect then.
 */
export const stillness = () =>
  typeof window !== 'undefined' &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches
