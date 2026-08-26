use std::{
    io::{IsTerminal, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];

#[derive(Clone)]
pub struct Progress {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    interactive: bool,
    quiet: bool,
}

struct State {
    title: String,
    message: String,
    percent: Option<u8>,
    started: Instant,
    last_progress: Instant,
}

impl Progress {
    pub fn new(title: impl Into<String>, quiet: bool) -> Self {
        let interactive = std::io::stderr().is_terminal() && !quiet;
        let progress = Self {
            state: Arc::new(Mutex::new(State {
                title: title.into(),
                message: "Fetching grapes...".into(),
                percent: None,
                started: Instant::now(),
                last_progress: Instant::now(),
            })),
            stop: Arc::new(AtomicBool::new(false)),
            interactive,
            quiet,
        };
        if interactive {
            let cloned = progress.clone();
            thread::spawn(move || cloned.animate());
        } else if !quiet {
            eprintln!("🥃 {}", progress.state.lock().expect("progress lock").title);
        }
        progress
    }

    pub fn update(&self, message: impl Into<String>, percent: Option<u8>) {
        let message = message.into();
        let mut state = self.state.lock().expect("progress lock");
        if !self.interactive && !self.quiet && state.message != message {
            eprintln!("  {message}");
        }
        if state.percent != percent {
            state.last_progress = Instant::now();
        }
        state.message = message;
        state.percent = percent;
    }

    fn animate(&self) {
        let mut frame = 0;
        while !self.stop.load(Ordering::Relaxed) {
            let state = self.state.lock().expect("progress lock");
            let percent = state
                .percent
                .map(|p| format!(" {p:3}%"))
                .unwrap_or_default();
            let elapsed = state.started.elapsed().as_secs();
            let remaining = state
                .percent
                .filter(|percent| {
                    *percent >= 5
                        && *percent < 100
                        && elapsed >= 2
                        && state.last_progress.elapsed() < Duration::from_secs(3)
                })
                .map(|percent| {
                    let seconds = elapsed.saturating_mul((100 - percent) as u64) / percent as u64;
                    format!(
                        " · ~{}–{}s remaining",
                        seconds.saturating_mul(3) / 4,
                        seconds.saturating_mul(5) / 4 + 1
                    )
                })
                .unwrap_or_default();
            eprint!(
                "\r\x1b[2K{} {}{} · {} ({}s){}",
                FRAMES[frame % FRAMES.len()],
                state.title,
                percent,
                state.message,
                elapsed,
                remaining
            );
            let _ = std::io::stderr().flush();
            drop(state);
            frame += 1;
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.interactive {
            thread::sleep(Duration::from_millis(110));
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
        }
    }
}
