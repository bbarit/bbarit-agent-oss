//! Live streaming display: an animated spinner while waiting for the first
//! token, then live token output. The spinner thread and the token sink both
//! write to stdout, so a shared mutex serializes them to avoid garbled output.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct StreamGuard {
    stop: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    lock: Arc<Mutex<()>>,
    handle: Option<JoinHandle<()>>,
}

/// Install the streaming display. `header` is the prefix shown before the
/// assistant text (already colorized). Call [`StreamGuard::finish`] after the
/// turn to stop the spinner and clear any leftover line.
pub fn start(header: String) -> StreamGuard {
    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let lock = Arc::new(Mutex::new(()));

    let spinner_stop = stop.clone();
    let spinner_started = started.clone();
    let spinner_lock = lock.clone();
    let spinner_header = header.clone();
    let handle = std::thread::spawn(move || {
        let mut frame = 0usize;
        loop {
            if spinner_stop.load(Ordering::Relaxed) {
                break;
            }
            if !spinner_started.load(Ordering::Relaxed) {
                let _guard = spinner_lock.lock().unwrap();
                if !spinner_started.load(Ordering::Relaxed) {
                    print!(
                        "\r\x1b[K{spinner_header}{} thinking…",
                        FRAMES[frame % FRAMES.len()]
                    );
                    let _ = std::io::stdout().flush();
                }
            }
            frame += 1;
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let sink_started = started.clone();
    let sink_lock = lock.clone();
    crate::llm::set_stream_sink(Some(Box::new(move |chunk: &str| {
        let _guard = sink_lock.lock().unwrap();
        if !sink_started.swap(true, Ordering::Relaxed) {
            // First token: erase the spinner line and reprint the header.
            print!("\r\x1b[K{header}");
        }
        print!("{chunk}");
        let _ = std::io::stdout().flush();
    })));

    StreamGuard {
        stop,
        started,
        lock,
        handle: Some(handle),
    }
}

impl StreamGuard {
    /// Stop the spinner, uninstall the sink, and clear the line if nothing
    /// streamed. Returns whether any token was streamed.
    pub fn finish(mut self) -> bool {
        self.stop.store(true, Ordering::Relaxed);
        crate::llm::set_stream_sink(None);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _guard = self.lock.lock().unwrap();
        let streamed = self.started.load(Ordering::Relaxed);
        if !streamed {
            // No token ever arrived; clear the spinner placeholder.
            print!("\r\x1b[K");
            let _ = std::io::stdout().flush();
        }
        streamed
    }
}
