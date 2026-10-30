//! `horizon-server`: Leaf's startup (`org.bukkit.craftbukkit.Main` -> `net.minecraft.server.Main`).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    horizon_server::api_impl::main::Main::main(&args);
}
