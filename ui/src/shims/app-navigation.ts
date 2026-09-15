// Glow's `Root` is written for SvelteKit and asks for its navigation hook.
// There is no router here — one window, no pages — so the hook is a no-op and
// view transitions simply never fire. Aliased in `vite.config.ts`.
export function onNavigate(_callback: unknown): void {}
