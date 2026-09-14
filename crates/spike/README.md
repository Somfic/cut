# Overlay spike

Throwaway. It exists to answer four questions about putting a web front end
over the native video surface, and should be deleted once they are answered.

The window has no webview of its own. wgpu draws decoded frames into it using
the same `cut_render::FrameRenderer` the iced front end uses — so what this
measures is the real frame path, not a mock of it — and a transparent webview
is added as a child, on top, holding the HTML.

```
cargo run --release -p cut-spike            # dev
cargo tauri build                           # bundled, from crates/spike
```

## The questions

1. **Bundled transparency.** Video visible through the webview in a `.app`, not
   just under `tauri dev`. Tauri [#13415] reports `.transparent(true)` windows
   coming out solid white after bundling, so dev alone proves nothing.
2. **No frame-path regression.** Real decoder, real renderer. The `video` and
   `present` numbers should match what the iced front end reports.
3. **Input routing.** Clicks land on HTML chrome and are not swallowed over the
   video region. The page counts both.
4. **Resize sync.** The video tracks the rect the page reserves for it, without
   tearing or lag.

## Answers

1. **Bundled transparency — pass.** The video shows through a transparent
   webview in a bundled `.app`, not just under `tauri dev`. [#13415] did not
   reproduce here on macOS 27 / Tauri 2.11.5 with `macOSPrivateApi`.
2. **No frame-path regression — pass at 24 fps.** Frames arrive and present
   1:1 with zero drops, bundled and unbundled, at ~11% CPU. The upload runs
   on its own thread (see below) and there is no longer a CPU copy at all:
   planes go from the decoder's buffer to the GPU untouched. **Untested at
   4K** — point `demo::SOURCES` at 4K footage to find out.
3. **Input routing — unverified.**
4. **Resize sync — unverified.**

## What it found

- **Metal will not create a surface off the main thread.** `create_surface`
  panics with `get_metal_layer cannot be called in non-ui thread`. The draw
  loop therefore lives in the `RunEvent::MainEventsCleared` callback, on the
  event-loop thread, rather than on a thread of its own. This is the single
  biggest constraint the design has to absorb: the video upload competes with
  the webview for the same thread.
- **`Window::add_child` is behind tauri's `unstable` feature.** The whole
  multi-surface approach rests on an API with no stability guarantee.
- **Transparency needs `macOSPrivateApi: true`** plus the `macos-private-api`
  cargo feature. That makes the app ineligible for the Mac App Store.
- **An sRGB surface double-encodes the picture.** `yuv_to_rgb` already returns
  gamma-encoded BT.709, so an `*Srgb` surface format washes it out. The config
  asks for the linear twin of whatever the default is.
- **`MainEventsCleared` is not a frame clock, and mistaking it for one was the
  stutter.** tao idles on `ControlFlow::Wait`, so with no mouse or keyboard
  input it fires two to seven times a second. Video arrived at 24 fps and
  presented at 2-7, dropping 17-21 frames a second. Frames are now handed to
  the main thread with `AppHandle::run_on_main_thread` as they arrive, which
  both wakes the loop and draws: present tracks video exactly, zero drops.
- **The repack into a `Vec` was never needed.** `Frame` used to tighten each
  plane up so rows were contiguous; `write_texture` takes a row stride, so the
  decoder's padded rows can be uploaded where they lie. Unlike a buffer copy,
  it puts no 256-byte alignment requirement on that stride. Removing the pass
  saves a full-frame copy *and* a full-frame allocation per frame — 3 MB each
  at 1080p, 25 MB at 4K P010. `Frame` now holds the mapped
  `VideoFrame<Readable>` and hands out plane slices.
- **The upload does not have to be on the main thread, and should not be.**
  Metal pins surface creation and presentation there, but `wgpu::Device` and
  `Queue` are `Send + Sync`, so `write_texture` can run on a worker that holds
  the `FrameRenderer` behind a mutex. The main thread is left with a resize
  check, an 8-byte uniform write, one draw call and the present. Both halves
  of the frame path are ordered by the queue, so no extra synchronisation is
  needed.
- **Gate the present on something actually changing.** Uploading and the
  event-loop turn both want to present; without a check on "new upload, or
  the rect moved" every frame went to the screen twice.
- **Measure with totals, not a smoothed rate.** The first pass reused the
  app's `Rate`, which seeds from its first sample — one tight burst pinned it
  at 485,000 fps and it decayed for seconds afterwards. It sent this
  investigation chasing a transport flood that was never happening. The
  counters here are running totals differenced over a known interval.

[#13415]: https://github.com/tauri-apps/tauri/issues/13415
