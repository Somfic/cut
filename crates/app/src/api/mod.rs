//! The API the page calls, as draad traits.
//!
//! Each `#[api]` trait generates a `#[tauri::command]` wrapper and a typed
//! TypeScript client, so the two sides cannot drift.

pub mod surface;
pub mod timeline;
pub mod transport;
