import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'

// The Rust side serves `dist` in a bundled build and proxies to the dev
// server otherwise; `tauri.conf.json` has to agree with both of these.
export default defineConfig({
  root: 'ui',
  // Relative, not absolute: the webview serves the bundle from a custom
  // protocol where a leading slash does not resolve, and a module script that
  // fails to load does so silently — the page simply never mounts.
  base: './',
  plugins: [svelte()],
  resolve: {
    alias: {
      // Glow ships SvelteKit source; this is the only bit of the framework it
      // actually reaches for.
      '$app/navigation': fileURLToPath(new URL('./ui/src/shims/app-navigation.ts', import.meta.url)),
    },
  },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: '../dist', emptyOutDir: true, target: 'safari15' },
})
