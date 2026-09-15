export class EasedNumber {
  #value: number;
  #target: number;
  #tau: number; // ms to cover ~2/3 of the remaining distance

  constructor(value = 0, tau = 70) {
    this.#value = value;
    this.#target = value;
    this.#tau = tau;
  }

  get value(): number {
    return this.#value;
  }

  to(target: number) {
    this.#target = target;
  }

  // jumps
  set(value: number) {
    this.#value = value;
    this.#target = value;
  }

  advance(dt: number): boolean {
    const remaining = this.#target - this.#value;

    if (Math.abs(remaining) < 0.0025) {
      this.#value = this.#target;
      return false;
    }

    this.#value += remaining * (1 - Math.exp(-dt / this.#tau));
    return true;
  }
}

export const wants_no_animations = () =>
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;
