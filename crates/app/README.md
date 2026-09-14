# cut

The front end: a Svelte UI in a transparent webview, over a native video
surface. `cut_engine` does the decoding, `cut_render` puts frames on the GPU.

```
cargo tauri dev      # from this directory; starts vite too
cargo tauri build
```

Plain `cargo run` does not work once `devUrl` is set — Tauri loads the dev
server, which only `cargo tauri dev` starts.

## macOS notes

Things that cost a day to find, in the order they bite:

- **Metal will not create a surface off the main thread.** Presenting stays on
  the event loop; only the upload gets its own thread.
- **`MainEventsCleared` is not a frame clock.** tao idles on
  `ControlFlow::Wait`, so it fires a few times a second. Frames are handed to
  the main thread with `AppHandle::run_on_main_thread` as they arrive.
- **A live resize runs in a nested event loop**, so nothing repaints for the
  length of the drag. `WindowEvent::Resized` paints instead, bypassing the
  dirty gate — a reconfigured swapchain has undefined contents.
- **`TitleBarStyle::Overlay`, not `decorations(false)`.** Dropping decorations
  also drops rounded corners, the resize margin and the traffic lights, and
  insets the page from the surface so every reported rect is out by the title
  bar's height.
- **`withGlobalTauri` is off by default**, and failing it is silent:
  `window.__TAURI__` is undefined, the first line throws, and the page renders
  its HTML while ignoring every click.
- **Child webviews take physical pixels.** `LogicalSize` here builds a webview
  twice the window's size on a 2x display.
- **The page cannot paint its own backdrop.** Everything it leaves transparent
  is a hole onto the surface, so the surface clears to the colour the page
  reports and the page matches it.
- **An sRGB surface double-encodes the picture** — `yuv_to_rgb` already
  returns gamma-encoded BT.709.

`Window::add_child` is behind tauri's `unstable` feature, and transparency
needs `macOSPrivateApi`, which rules out the Mac App Store.
