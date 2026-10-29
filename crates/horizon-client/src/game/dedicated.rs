//! The dedicated server (net.minecraft.server.Main and DedicatedServer):
//! `horizon-client --dedicated [--nogui] [--port N] [--world NAME]
//! [--universe DIR] [--initSettings]`. It reads server.properties and
//! eula.txt from the working directory, opens `<universe>/<level-name>` and
//! runs the same game as single player, without a local player: the world,
//! its simulation and the play-state hub that LAN hosting uses, configured
//! from server.properties, with the operator, whitelist and ban lists and a
//! console on standard input.
use std::io::Write;

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
