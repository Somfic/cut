/** One fetch for what is already there, then every change the engine pushes. */
export function sync<T>(
  get: () => Promise<T>,
  changed: (take: (value: T) => void) => unknown,
  take: (value: T) => void,
) {
  get().then(take);
  changed(take);
}
