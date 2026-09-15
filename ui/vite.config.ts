import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  // Relative, not absolute: the webview serves the bundle from a custom
  // protocol where a leading slash does not resolve, and a module script that
  // fails to load does so silently — the page simply never mounts.
  base: './',
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
      $components: fileURLToPath(new URL('./src/components', import.meta.url)),
      // Glow ships SvelteKit source; this is the only bit of the framework it
      // actually reaches for.
      '$app/navigation': fileURLToPath(new URL('./src/shims/app-navigation.ts', import.meta.url)),
    },
  },
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true, target: 'safari15' },
})
