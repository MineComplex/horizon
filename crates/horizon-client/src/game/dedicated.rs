//! The dedicated server (net.minecraft.server.Main and DedicatedServer):
//! `horizon-client --dedicated [--nogui] [--port N] [--world NAME]
//! [--universe DIR] [--initSettings]`. It reads server.properties and
//! eula.txt from the working directory, opens `<universe>/<level-name>` and
//! runs the same game as single player, without a local player: the world,
//! its simulation and the play-state hub that LAN hosting uses, configured
//! from server.properties, with the operator, whitelist and ban lists and a
//! console on standard input.
use super::Game;
use std::io::Write;

mod status;




/// Log4j's console pattern: `[HH:mm:ss] [thread/LEVEL]: message`.
pub(super) fn log(level: &str, message: &str) {
    log_from("Server thread", level, message);
}

/// A line from another of vanilla's threads (the login's User Authenticator).
fn log_from(thread: &str, level: &str, message: &str) {
    let file_level = match level {
        "WARN" => ::log::Level::Warn,
        "ERROR" => ::log::Level::Error,
        _ => ::log::Level::Info,
    };
    horizon_client::logging::write_as(thread, file_level, message);
    CAPTURE.with_borrow_mut(|lines| {
        if let Some(lines) = lines {
            lines.push(message.to_owned());
        }
    });
    // Log4j's %d{HH:mm:ss}: the local time, as in logs/latest.log.
    // The launcher can close its stdout pipe while the server keeps running.
    let _ = writeln!(
        std::io::stdout().lock(),
        "[{}] [{thread}/{level}]: {message}",
        chrono::Local::now().format("%H:%M:%S")
    );
}

thread_local! {
    /// The lines logged while a control request runs a console command.
    static CAPTURE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}






impl Game<'_> {}
impl crate::game::Sim<'_> {
}




impl Game<'_> {

}
impl crate::game::Sim<'_> {
}


#[cfg(test)]
mod tests {
    use super::{capture_log, log, log_from};
    use std::{
        fs,
        io::{self, BufRead, BufReader, Read, Write},
        process::{Command, Stdio},
        time::{SystemTime, UNIX_EPOCH},
    };

    const LOGIN: &str = "Player[/127.0.0.1:0] logged in with entity id 2 at (0.5, 115.0, 0.5)";
    const WARNING: &str = "Authentication warning";
    const READY: &str = "horizon-console-log-ready\n";
    const CHILD_HOME: &str = "HORIZON_TEST_CONSOLE_LOG_HOME";

    fn assert_log_lines(text: &str) {
        let expected = [
            format!("] [Server thread/INFO]: {LOGIN}"),
            format!("] [User Authenticator #1/WARN]: {WARNING}"),
        ];
        assert_eq!(text.lines().count(), expected.len());
        for (line, suffix) in text.lines().zip(expected) {
            assert!(line.starts_with('['));
            assert!(chrono::NaiveTime::parse_from_str(&line[1..9], "%H:%M:%S").is_ok());
            assert_eq!(&line[9..], suffix);
        }
    }

    #[test]
    fn console_logging_survives_closed_stdout() {
        if let Some(home) = std::env::var_os(CHILD_HOME) {
            horizon_client::logging::init(std::path::Path::new(&home), "Server thread");
            // Let the parent close the real stdout pipe only after libtest's startup output.
            io::stdout().write_all(READY.as_bytes()).unwrap();
            io::stdout().flush().unwrap();
            io::stdin().read_exact(&mut [0]).unwrap();
            std::thread::Builder::new()
                .name("Server thread".into())
                .spawn(|| {
                    let captured = capture_log(|| {
                        log("INFO", LOGIN);
                        log_from("User Authenticator #1", "WARN", WARNING);
                    });
                    assert_eq!(captured, [LOGIN, WARNING]);
                    horizon_client::logging::flush();
                })
                .unwrap()
                .join()
                .unwrap();
            // Libtest's final status also writes to stdout; only the game logger is under test.
            std::process::exit(0);
        }

        for closed_stdout in [false, true] {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let home = std::env::temp_dir().join(format!(
                "horizon-console-log-test-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir(&home).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "game::dedicated::tests::console_logging_survives_closed_stdout",
                    "--nocapture",
                ])
                .env(CHILD_HOME, &home)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdout = BufReader::new(child.stdout.take().unwrap());
            loop {
                let mut line = String::new();
                assert!(
                    stdout.read_line(&mut line).unwrap() > 0,
                    "child was not ready"
                );
                if line.ends_with(READY) {
                    break;
                }
            }
            if closed_stdout {
                drop(stdout); // No reader remains: the next console write gets EPIPE.
            } else {
                child.stdout = Some(stdout.into_inner());
            }
            child.stdin.take().unwrap().write_all(&[1]).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "closed_stdout={closed_stdout}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if !closed_stdout {
                assert_log_lines(std::str::from_utf8(&output.stdout).unwrap());
            }
            assert_log_lines(&fs::read_to_string(home.join("logs/latest.log")).unwrap());
            fs::remove_dir_all(home).unwrap();
        }
    }
}
