# Fatherless

A `std::process::Command` alternative for spawning processes that are fully
independent of their parent.

A child started with `fatherless` runs in its own session. It keeps running
after your program exits, has no controlling terminal, and does not receive
terminal signals (`SIGINT`, `SIGHUP`) sent to the parent.

## Features

- **Std-like API**: modelled after `std::process::Command`, so it feels familiar.
- **Process detachment**: the child becomes a session leader via `setsid(2)`.
- **No zombies**: the child is reaped in the background.
- **Safe defaults**: stdin, stdout and stderr go to `/dev/null` unless set.

## Installation

```toml
[dependencies]
fatherless = "0.1"
```

Or from a local checkout:

```toml
[dependencies]
fatherless = { path = "../fatherless" }
```

## Usage

```rust
use fatherless::Command;

fn main() -> std::io::Result<()> {
    let child = Command::new("sleep")
        .arg("60")
        .spawn_detached()?;

    println!("started detached process {}", child.id());
    Ok(())
}
```

Redirecting output to a log file:

```rust
use std::fs::File;
use fatherless::Command;

let log = File::create("/tmp/server.log")?;
let child = Command::new("my-server")
    .args(["--port", "8080"])
    .env("RUST_LOG", "info")
    .current_dir("/srv/app")
    .stdout(log.try_clone()?)
    .stderr(log)
    .spawn_detached()?;
```

## API overview

| Item | Description |
| --- | --- |
| `Command::new(program)` | Create a builder for `program` |
| `.arg` / `.args` | Add arguments |
| `.env` / `.envs` / `.env_remove` / `.env_clear` | Configure the environment |
| `.current_dir` | Set the working directory |
| `.stdin` / `.stdout` / `.stderr` | Configure standard streams (default `/dev/null`) |
| `.spawn_detached()` | Start the process; returns `io::Result<DetachedChild>` |
| `DetachedChild::id()` | The child's PID (also its session and group id) |

`DetachedChild` holds only the PID. Dropping it does not affect the child, and
there is no `wait`/`kill`, since reaping is handled for you.

## Platform support

Unix only (Linux, macOS, BSDs), as it relies on `setsid(2)`.

## Development

```sh
cargo build
cargo test
cargo doc --open
```

## License

See [LICENSE.md](LICENSE.md).
