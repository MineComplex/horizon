//! `/version`'s lines (VersionCommand), which F3 + V prints on a client.
use horizon_client::text::Text;

use horizon_core::version::{BUILD_TIME, DATA_VERSION, STABLE, VANILLA_VERSION, VERSION_ID};

fn num(n: impl std::fmt::Display) -> Text {
    Text::literal(n.to_string())
}

impl crate::game::Sim<'_> {
    /// `VersionCommand.dumpVersion`'s lines: Horizon reports the version it
    /// implements.
    pub(in crate::game) fn version_lines(&self) -> Vec<Text> {
        let protocol = horizon_protocol::PROTOCOL_VERSION;
        let mut lines: Vec<Text> = [
            ("commands.version.id", vec![Text::literal(VERSION_ID)]),
            (
                "commands.version.name",
                vec![Text::literal(VANILLA_VERSION)],
            ),
            ("commands.version.data", vec![num(DATA_VERSION)]),
            ("commands.version.series", vec![Text::literal("main")]),
            (
                "commands.version.protocol",
                vec![num(protocol), Text::literal(format!("0x{protocol:x}"))],
            ),
            (
                "commands.version.build_time",
                vec![Text::literal(BUILD_TIME)],
            ),
            (
                "commands.version.pack.resource",
                vec![Text::literal("97.1")],
            ),
            ("commands.version.pack.data", vec![Text::literal("121.0")]),
        ]
        .into_iter()
        .map(|(key, args)| Text::translate(key, &args))
        .collect();
        lines.push(Text::translate(
            if STABLE {
                "commands.version.stable.yes"
            } else {
                "commands.version.stable.no"
            },
            &[],
        ));
        lines
    }
}
