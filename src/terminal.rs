use std::{
    env,
    ffi::CStr,
    io::{self, Read, Write},
    mem,
    os::fd::AsRawFd,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use crate::{
    Effect,
    prompt::{Cell, Placement},
};

const SIGNALS: [libc::c_int; 4] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGWINCH];
const RESTORE: &[u8] = b"\x1b[0m\x1b[?25h\x1b[?1049l";
static CANCELLED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub columns: usize,
    pub rows: usize,
}

impl Viewport {
    pub fn new(columns: usize, rows: usize) -> Option<Self> {
        (columns > 0 && rows > 0).then_some(Self {
            columns: columns.min(500),
            rows: rows.min(200),
        })
    }

    pub fn area(self) -> usize {
        self.columns * self.rows
    }

    pub fn scale(self) -> f64 {
        (self.area() as f64 / (80.0 * 24.0)).sqrt().clamp(0.6, 2.5)
    }
}

pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

extern "C" fn stop(_: libc::c_int) {
    CANCELLED.store(true, Ordering::Relaxed);
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "animation cancelled")
}

fn write_all_with(
    mut bytes: &[u8],
    mut write: impl FnMut(&[u8]) -> io::Result<usize>,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn raw_write(bytes: &[u8]) -> io::Result<usize> {
    let written = unsafe { libc::write(libc::STDOUT_FILENO, bytes.as_ptr().cast(), bytes.len()) };
    if written < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(written as usize)
    }
}

fn restore() {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        let _ = write_all_with(RESTORE, raw_write);
    }
}

pub struct Terminal {
    viewport: Viewport,
    dimensions: (usize, usize),
    previous_signals: Vec<(libc::c_int, libc::sigaction)>,
}

impl Terminal {
    pub fn open() -> io::Result<Self> {
        // ioctl initializes winsize on success and does not retain its pointer.
        let mut size: libc::winsize = unsafe { mem::zeroed() };
        if unsafe { libc::isatty(libc::STDOUT_FILENO) } != 1 {
            return Err(io::Error::other("stdout is not a terminal"));
        }
        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let dimensions = (usize::from(size.ws_col), usize::from(size.ws_row));
        let viewport = Viewport::new(dimensions.0, dimensions.1)
            .ok_or_else(|| io::Error::other("terminal has no usable dimensions"))?;
        let mut terminal = Self {
            viewport,
            dimensions,
            previous_signals: Vec::new(),
        };
        CANCELLED.store(false, Ordering::Relaxed);
        for signal in SIGNALS {
            let mut action: libc::sigaction = unsafe { mem::zeroed() };
            let mut previous: libc::sigaction = unsafe { mem::zeroed() };
            action.sa_sigaction = stop as *const () as usize;
            unsafe { libc::sigemptyset(&mut action.sa_mask) };
            if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                return Err(io::Error::last_os_error());
            }
            terminal.previous_signals.push((signal, previous));
        }
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous_hook(info);
        }));
        Ok(terminal)
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn native_origin(&self) -> Option<Cell> {
        let pane = env::var("TMUX_PANE").ok()?;
        if env::var_os("TMUX").is_none_or(|s| s.is_empty())
            || !pane
                .strip_prefix('%')
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        let mut child = Command::new("tmux")
            .args([
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{cursor_x} #{cursor_y} #{pane_width} #{pane_height} #{pane_tty}",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let result = (|| {
            let mut stdout = child.stdout.take()?;
            let fd = stdout.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return None;
            }
            let deadline = Instant::now() + Duration::from_millis(300);
            let mut bytes = Vec::with_capacity(128);
            loop {
                let mut buffer = [0; 256];
                match stdout.read(&mut buffer) {
                    Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => return None,
                }
                if bytes.len() > 256 || cancelled() || Instant::now() >= deadline {
                    return None;
                }
                if let Some(status) = child.try_wait().ok()? {
                    if !status.success() {
                        return None;
                    }
                    match stdout.read(&mut buffer) {
                        Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                        Err(_) => return None,
                    }
                    let text = std::str::from_utf8(&bytes).ok()?;
                    let tty = unsafe { libc::ttyname(libc::STDOUT_FILENO) };
                    if tty.is_null() {
                        return None;
                    }
                    let tty = unsafe { CStr::from_ptr(tty) }.to_str().ok()?;
                    return parse_origin(text, tty, self.dimensions, self.viewport);
                }
                self.wait_for(Duration::from_millis(1)).ok()?;
            }
        })();
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    pub fn enter(&mut self, effect: Effect, placement: Option<Placement>) -> io::Result<()> {
        if let Some(Placement::Native { origin, overflow }) = placement
            && overflow != 0
        {
            let mut scroll = Vec::new();
            write!(scroll, "\x1b[{};1H", self.viewport.rows).unwrap();
            scroll.resize(scroll.len() + overflow, b'\n');
            write!(scroll, "\x1b[{};{}H", origin.row + 1, origin.col + 1).unwrap();
            self.write(&scroll)?;
        }
        let mut entry = Vec::from(b"\x1b[?1049h".as_slice());
        if matches!(placement, Some(Placement::Relative { .. })) {
            entry.extend_from_slice(b"\x1b7");
        }
        entry.extend_from_slice(b"\x1b[?25l");
        entry.extend_from_slice(if effect == Effect::Shimmer {
            b"\x1b[48;5;233m"
        } else {
            b"\x1b[0m"
        });
        entry.extend_from_slice(b"\x1b[2J");
        let mask = SignalMask::block()?;
        if cancelled() {
            return Err(interrupted());
        }
        ACTIVE.store(true, Ordering::SeqCst);
        drop(mask);
        self.write(&entry)
    }

    pub fn write(&self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            if cancelled() {
                return Err(interrupted());
            }
            match raw_write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(written) => bytes = &bytes[written..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn wait_for(&self, duration: Duration) -> io::Result<()> {
        self.wait_until(Instant::now() + duration)
    }

    pub fn wait_until(&self, deadline: Instant) -> io::Result<()> {
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if cancelled() {
                return Err(interrupted());
            }
            // poll has no descriptors and returns on signals without consuming terminal input.
            let milliseconds = remaining.as_millis().clamp(1, 10) as i32;
            unsafe { libc::poll(std::ptr::null_mut(), 0, milliseconds) };
        }
        if cancelled() {
            Err(interrupted())
        } else {
            Ok(())
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore();
        for (signal, previous) in &self.previous_signals {
            unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()) };
        }
    }
}

struct SignalMask(libc::sigset_t);

impl SignalMask {
    fn block() -> io::Result<Self> {
        let mut mask: libc::sigset_t = unsafe { mem::zeroed() };
        let mut previous: libc::sigset_t = unsafe { mem::zeroed() };
        unsafe { libc::sigemptyset(&mut mask) };
        for signal in SIGNALS {
            unsafe { libc::sigaddset(&mut mask, signal) };
        }
        if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &mask, &mut previous) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(previous))
    }
}

impl Drop for SignalMask {
    fn drop(&mut self) {
        unsafe { libc::sigprocmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut()) };
    }
}

fn parse_origin(
    text: &str,
    tty: &str,
    dimensions: (usize, usize),
    viewport: Viewport,
) -> Option<Cell> {
    let mut values = text.trim_end_matches('\n').split(' ');
    let mut number = || {
        let value = values.next()?;
        (!value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()))
            .then(|| value.parse::<usize>().ok())
            .flatten()
    };
    let col = number()?;
    let row = number()?;
    let width = number()?;
    let height = number()?;
    if values.next()? != tty
        || values.next().is_some()
        || (width, height) != dimensions
        || dimensions != (viewport.columns, viewport.rows)
        || col >= viewport.columns
        || row >= viewport.rows
    {
        return None;
    }
    Some(Cell { col, row })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::File, os::fd::FromRawFd};

    #[test]
    fn validates_tmux_tty_dimensions_and_cursor() {
        let viewport = Viewport::new(80, 24).unwrap();
        assert_eq!(
            parse_origin("12 7 80 24 /dev/pts/3\n", "/dev/pts/3", (80, 24), viewport),
            Some(Cell { col: 12, row: 7 })
        );
        for invalid in [
            "12 7 80 24 /dev/pts/4",
            "12 7 81 24 /dev/pts/3",
            "80 7 80 24 /dev/pts/3",
            "0 -1 80 24 /dev/pts/3",
            "0 0 80 24 /dev/pts/3 extra",
        ] {
            assert!(parse_origin(invalid, "/dev/pts/3", (80, 24), viewport).is_none());
        }
    }

    #[test]
    fn oversized_terminals_use_relative_placement() {
        for ((width, height), origin) in [
            ((100, 300), "4 199 100 300 /dev/tty"),
            ((600, 24), "4 3 600 24 /dev/tty"),
        ] {
            assert_eq!(
                parse_origin(
                    origin,
                    "/dev/tty",
                    (width, height),
                    Viewport::new(width, height).unwrap()
                ),
                None
            );
        }
    }

    #[test]
    fn cleanup_retries_interrupted_and_partial_writes() {
        let mut output = Vec::new();
        let mut calls = 0;
        write_all_with(RESTORE, |bytes| {
            calls += 1;
            if calls == 2 {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let count = bytes.len().min(3);
            output.extend_from_slice(&bytes[..count]);
            Ok(count)
        })
        .unwrap();
        assert_eq!(output, RESTORE);
        assert!(write_all_with(RESTORE, |_| Ok(0)).is_err());
    }

    #[test]
    fn panic_cleanup_child() {
        if env::var_os("SHIMMER_TEST_PANIC_CHILD").is_none() {
            return;
        }
        let mut terminal = Terminal::open().unwrap();
        terminal.enter(Effect::Assemble, None).unwrap();
        assert!(std::panic::catch_unwind(|| panic!("terminal panic fixture")).is_err());
        terminal.write(b"AFTER_PANIC").unwrap();
    }

    #[test]
    fn panic_restores_terminal_before_diagnostics_without_duplicate_cleanup() {
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(size),
                )
            },
            0
        );
        // openpty returns two owned descriptors; File closes each exactly once.
        let mut master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let status = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal::tests::panic_cleanup_child",
                "--nocapture",
            ])
            .env("SHIMMER_TEST_PANIC_CHILD", "1")
            .stdin(Stdio::null())
            .stderr(slave.try_clone().unwrap())
            .stdout(slave)
            .status()
            .unwrap();
        assert!(status.success());
        let mut bytes = Vec::new();
        if let Err(error) = master.read_to_end(&mut bytes) {
            assert_eq!(error.raw_os_error(), Some(libc::EIO));
        }
        let output = String::from_utf8(bytes).unwrap();
        let restore = std::str::from_utf8(RESTORE).unwrap();
        assert_eq!(output.matches(restore).count(), 1, "{output:?}");
        assert!(output.find(restore).unwrap() < output.find("terminal panic fixture").unwrap());
        assert!(
            output.find("terminal panic fixture").unwrap() < output.find("AFTER_PANIC").unwrap()
        );
    }
}
