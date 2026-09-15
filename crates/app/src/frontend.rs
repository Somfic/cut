use tauri::WebviewUrl;

pub fn url() -> WebviewUrl {
    WebviewUrl::App("index.html".into())
}

#[cfg(not(debug_assertions))]
pub struct DevServer;

#[cfg(not(debug_assertions))]
impl DevServer {
    pub fn start() -> Self {
        DevServer
    }
}

#[cfg(debug_assertions)]
pub use dev::DevServer;

#[cfg(debug_assertions)]
mod dev {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::{Duration, Instant};

    const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui");
    const PORT: u16 = 5173;
    /// How long to wait for vite before loading anyway.
    const STARTUP: Duration = Duration::from_secs(20);

    /// The group to stop on a signal, or 0 for none.
    ///
    /// `RunEvent::Exit` covers closing the window, but a terminal `Ctrl-C`
    /// kills us outright — and the child has its own group, so it does not
    /// get the interrupt either. Without this it would be left holding the
    /// port.
    static GROUP: AtomicI32 = AtomicI32::new(0);

    /// The vite process, stopped when this is dropped.
    ///
    /// Left running it holds the port, and the next run fails on vite's
    /// `strictPort`.
    pub struct DevServer(Option<Child>);

    impl DevServer {
        pub fn start() -> Self {
            if listening() {
                println!("frontend: reusing the dev server on :{PORT}");
                return DevServer(None);
            }

            println!("frontend: starting vite on :{PORT}");

            let child = match Command::new("bun")
                .args(["run", "dev"])
                .current_dir(UI_DIR)
                // no stdin, vite would otherwise go interactive mode
                .stdin(Stdio::null())
                // give it its own group, so stopping it stops vite too
                .process_group(0)
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("frontend: could not start vite ({e}); is bun installed?");
                    return DevServer(None);
                }
            };

            let deadline = Instant::now() + STARTUP;
            while !listening() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }

            if listening() {
                println!("frontend: vite is up");
            } else {
                eprintln!("frontend: vite did not answer on :{PORT}; loading anyway");
            }

            GROUP.store(child.id() as i32, Ordering::Relaxed);
            install_signal_handlers();

            DevServer(Some(child))
        }
    }

    fn install_signal_handlers() {
        // `stop_group` only calls `kill` and `_exit`, both of which are async-signal-safe
        unsafe {
            let handler = stop_group as *const () as libc::sighandler_t;
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGTERM, handler);
        }
    }

    extern "C" fn stop_group(signal: libc::c_int) {
        let group = GROUP.load(Ordering::Relaxed);
        if group != 0 {
            unsafe { libc::kill(-group, libc::SIGTERM) };
        }
        // 128 + signal is "died on this signal"
        unsafe { libc::_exit(128 + signal) };
    }

    impl Drop for DevServer {
        fn drop(&mut self) {
            let Some(child) = &mut self.0 else {
                return;
            };

            // negative pid: the whole group, not just `bun`.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM) };
            let _ = child.wait();
        }
    }

    // vite binds `localhost`, which on this machine resolves to `[::1]` only
    // probing `127.0.0.1` alone waits out the whole timeout while the
    fn listening() -> bool {
        let Ok(addrs) = ("localhost", PORT).to_socket_addrs() else {
            return false;
        };

        addrs
            .into_iter()
            .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok())
    }
}
