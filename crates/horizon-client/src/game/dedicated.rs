//! The dedicated server (net.minecraft.server.Main and DedicatedServer):
//! `horizon-client --dedicated [--nogui] [--port N] [--world NAME]
//! [--universe DIR] [--initSettings]`. It reads server.properties and
//! eula.txt from the working directory, opens `<universe>/<level-name>` and
//! runs the same game as single player, without a local player: the world,
//! its simulation and the play-state hub that LAN hosting uses, configured
//! from server.properties, with the operator, whitelist and ban lists and a
//! console on standard input.

thread_local! {
    /// The lines logged while a control request runs a console command.
    static CAPTURE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}
