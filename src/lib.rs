//! A [`std::process::Command`] look-alike with [`Command::spawn_detached`],
//! which starts a child in its own session so it is independent of the parent
//! process: it survives the parent exiting and does not receive the parent's
//! terminal signals such as `SIGINT`/`SIGHUP`.
//!
//! # Example
//!
//! ```no_run
//! use fatherless::Command;
//!
//! let child = Command::new("sleep")
//!     .arg("60")
//!     .env("FOO", "bar")
//!     .spawn_detached()
//!     .expect("failed to spawn");
//! println!("started detached process {}", child.id());
//! // The child keeps running even after this program exits.
//! ```
//!
//! # Platform support
//!
//! Unix only (it relies on `setsid(2)` and [`std::os::unix::process::CommandExt`]).

use std::ffi::OsStr;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{self, Stdio};
use std::thread;

/// A process builder, modelled after [`std::process::Command`].
///
/// The builder methods mirror those of the standard library and return
/// `&mut Command` so calls can be chained. Finish with
/// [`spawn_detached`](Command::spawn_detached).
///
/// Unless configured otherwise, a detached child's stdin, stdout and stderr
/// are all connected to `/dev/null`, since the parent's terminal is not
/// shared with the child. Use [`stdin`](Command::stdin),
/// [`stdout`](Command::stdout) and [`stderr`](Command::stderr) to redirect
/// them, e.g. to a log file.
#[derive(Debug)]
pub struct Command {
    inner: process::Command,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

/// Handle to a detached child. Dropping it does not affect the child.
///
/// It only carries the process id: there is no `wait` or `kill`, because the
/// child is reaped automatically in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachedChild {
    pid: u32,
}

impl DetachedChild {
    /// The OS process id of the child (also its session and group id).
    pub fn id(&self) -> u32 {
        self.pid
    }
}

impl Command {
    /// Constructs a new `Command` for launching `program`, with the same
    /// lookup rules as [`std::process::Command::new`].
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command {
            inner: process::Command::new(program),
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Adds an argument to pass to the program.
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.inner.arg(arg);
        self
    }

    /// Adds multiple arguments to pass to the program.
    pub fn args<I, S>(&mut self, args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.inner.args(args);
        self
    }

    /// Sets an environment variable for the child.
    pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, val: V) -> &mut Command {
        self.inner.env(key, val);
        self
    }

    /// Sets multiple environment variables for the child.
    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Command
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.inner.envs(vars);
        self
    }

    /// Removes an environment variable from the child's environment.
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Command {
        self.inner.env_remove(key);
        self
    }

    /// Clears the entire environment inherited by the child.
    pub fn env_clear(&mut self) -> &mut Command {
        self.inner.env_clear();
        self
    }

    /// Sets the working directory of the child.
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Command {
        self.inner.current_dir(dir);
        self
    }

    /// Configures the child's stdin (default: `/dev/null`).
    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stdin = Some(cfg.into());
        self
    }

    /// Configures the child's stdout (default: `/dev/null`).
    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stdout = Some(cfg.into());
        self
    }

    /// Configures the child's stderr (default: `/dev/null`).
    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stderr = Some(cfg.into());
        self
    }

    /// Spawns the command as a detached process.
    ///
    /// The child becomes the leader of a new session (`setsid`), so it has no
    /// controlling terminal and is not part of the parent's process group.
    /// It is reaped in the background, so it never lingers as a zombie, and it
    /// keeps running if the parent exits.
    ///
    /// # Errors
    ///
    /// Returns an error if the program cannot be started (for example it does
    /// not exist or is not executable) or if `setsid` fails in the child.
    ///
    /// # Notes
    ///
    /// Reaping uses one lightweight background thread per child, which lives
    /// until the child exits. If the parent exits first, the child is
    /// re-parented to init and reaped there.
    pub fn spawn_detached(&mut self) -> io::Result<DetachedChild> {
        self.inner.stdin(self.stdin.take().unwrap_or_else(Stdio::null));
        self.inner.stdout(self.stdout.take().unwrap_or_else(Stdio::null));
        self.inner.stderr(self.stderr.take().unwrap_or_else(Stdio::null));

        // SAFETY: `setsid` is async-signal-safe and allocates nothing, so it
        // is sound to call between fork and exec.
        unsafe {
            self.inner.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let mut child = self.inner.spawn()?;
        let pid = child.id();
        // Reap the child when it exits; the handle we return is just the pid.
        thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(DetachedChild { pid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_has_own_session() {
        let dir = std::env::temp_dir().join(format!("fatherless-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out");
        let child = Command::new("sh")
            .arg("-c")
            .arg("ps -o sid= -p $$ > \"$0\"")
            .arg(&out)
            .spawn_detached()
            .unwrap();
        for _ in 0..100 {
            if std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let sid: u32 = std::fs::read_to_string(&out).unwrap().trim().parse().unwrap();
        assert_eq!(sid, child.id());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_program_errors() {
        assert!(Command::new("/nonexistent/program").spawn_detached().is_err());
    }
}
