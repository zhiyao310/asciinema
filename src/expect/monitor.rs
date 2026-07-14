use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::os::fd::AsRawFd;

use anyhow::{bail, Result};
use nix::libc;

use super::{ExecutionObserver, Progress};
use crate::tty::TtySize;

const STATUS_ROWS: u16 = 5;

pub(super) struct Monitor {
    size: TtySize,
    active: bool,
    panel: ProgressPanel,
}

impl Monitor {
    pub(super) fn start() -> Result<Self> {
        let stdout = std::io::stdout();
        if !stdout.is_terminal() {
            bail!("--monitor requires terminal stdout");
        }

        let mut winsize = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let result = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) };
        if result == -1 {
            return Err(std::io::Error::last_os_error().into());
        }

        let size = TtySize(winsize.ws_col, winsize.ws_row);
        if size.0 == 0 || size.1 < STATUS_ROWS + 2 {
            bail!(
                "--monitor requires a terminal at least 1 column by {} rows",
                STATUS_ROWS + 2
            );
        }

        let mut monitor = Self {
            size,
            active: true,
            panel: ProgressPanel::new(STATUS_ROWS as usize),
        };
        monitor.enter()?;
        Ok(monitor)
    }

    fn capture_size(&self) -> TtySize {
        TtySize(self.size.0, self.size.1 - STATUS_ROWS)
    }

    fn write_output(&self, bytes: &[u8]) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(bytes)?;
        stdout.flush()?;
        Ok(())
    }

    fn render_progress(&mut self, progress: Progress<'_>) -> Result<()> {
        let message = format!(
            "::: [{:>3}/{:<3}] L{:03} [{:<13}] {}",
            progress.current, progress.total, progress.line, progress.label, progress.summary
        );
        self.panel.push(message, self.size.0 as usize);

        let panel_top = self.capture_size().1 + 1;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(b"\x1b7")?;
        for offset in 0..STATUS_ROWS {
            write!(stdout, "\x1b[{};1H\x1b[2K", panel_top + offset)?;
            if let Some(message) = self.panel.get(offset as usize) {
                let color = if offset as usize + 1 == self.panel.len() {
                    "\x1b[1;36m"
                } else {
                    "\x1b[2;36m"
                };
                write!(stdout, "{color}{message}\x1b[0m")?;
            }
        }
        stdout.write_all(b"\x1b8")?;
        stdout.flush()?;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if self.active {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(b"\x1b[r\x1b[?1049l")?;
            stdout.flush()?;
            self.active = false;
        }

        Ok(())
    }

    fn enter(&mut self) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        write!(
            stdout,
            "\x1b[?1049h\x1b[2J\x1b[H\x1b[1;{}r\x1b[1;1H",
            self.capture_size().1
        )?;
        stdout.flush()?;
        Ok(())
    }
}

impl ExecutionObserver for Monitor {
    fn pty_size(&self) -> Option<TtySize> {
        Some(self.capture_size())
    }

    fn output(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_output(bytes)
    }

    fn instruction(&mut self, progress: Progress<'_>) -> Result<()> {
        self.render_progress(progress)
    }

    fn finish(&mut self) -> Result<()> {
        self.close()
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct ProgressPanel {
    lines: VecDeque<String>,
    capacity: usize,
}

impl ProgressPanel {
    fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, message: String, cols: usize) {
        self.lines
            .push_back(message.chars().take(cols).collect::<String>());
        if self.lines.len() > self.capacity {
            self.lines.pop_front();
        }
    }

    fn get(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(String::as_str)
    }

    fn len(&self) -> usize {
        self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::ProgressPanel;

    #[test]
    fn progress_panel_keeps_the_latest_entries() {
        let mut panel = ProgressPanel::new(2);
        panel.push("one".to_owned(), 80);
        panel.push("two".to_owned(), 80);
        panel.push("three".to_owned(), 80);

        assert_eq!(panel.get(0), Some("two"));
        assert_eq!(panel.get(1), Some("three"));
    }
}
