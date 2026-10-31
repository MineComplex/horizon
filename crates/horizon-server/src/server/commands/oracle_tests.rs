//! The command tree against the real classes: `LeafCommandsOracle.java`
//! (`mods/horizon/src/test/java/dev/horizonlab/leafserver/`) runs the 26.3 jar's `Commands`
//! dispatcher for a dedicated server, restricted to the ported commands; this compares parse
//! results and errors, usage, the commands packet per permission level, message rendering and the
//! unknown-command text. The command bodies need a `MinecraftServer` and are derived from sources.
//!
//! ```sh
//! java -cp <26.3 jar:libraries> LeafCommandsOracle.java commands.jsonl
//! HORIZON_VANILLA_JAR=<26.3 jar> HORIZON_COMMANDS_ORACLE=commands.jsonl \
//!     cargo test -p horizon-server commands_match_java -- --ignored --nocapture
//! ```

use serde_json::Value;

use super::*;
use crate::azalea::buf::AzBuf;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::azalea::language::Language;
use crate::command::ArgumentValue;
use crate::server::vanilla_jar::VanillaJar;
use crate::util::number_text::float_to_string;

fn str_of<'a>(row: &'a Value, key: &str) -> &'a str {
    row[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {row}"))
}

fn allowed(level: u8) -> impl Fn(Key) -> bool {
    move |key| !matches!(key, Key::Permission(needed) if needed > level)
}

fn argument_text(value: &ArgumentValue) -> String {
    match value {
        ArgumentValue::Integer(v) => v.to_string(),
        ArgumentValue::Float(v) => float_to_string(*v),
        ArgumentValue::String(v) => v.clone(),
        ArgumentValue::GameProfile(_) => "<profile>".to_owned(),
        other => panic!("no text for {other:?}"),
    }
}

fn argument(pair: &Value) -> PrimitiveOrComponent {
    let kind = pair[0].as_str().unwrap();
    let text = pair[1].as_str().unwrap();
    match kind {
        "s" => PrimitiveOrComponent::String(text.to_owned()),
        "i" => PrimitiveOrComponent::Integer(text.parse().unwrap()),
        "l" => PrimitiveOrComponent::Long(text.parse().unwrap()),
        "f" => PrimitiveOrComponent::Float(text.parse().unwrap()),
        "c" => PrimitiveOrComponent::FormattedText(source::translatable(text, Vec::new())),
        other => panic!("argument type {other}"),
    }
}

#[test]
#[ignore = "needs HORIZON_COMMANDS_ORACLE (LeafCommandsOracle.java on the 26.3 jar) and HORIZON_VANILLA_JAR"]
fn commands_match_java() {
    let path = std::env::var_os("HORIZON_COMMANDS_ORACLE").expect("HORIZON_COMMANDS_ORACLE");
    let jar = std::env::var_os("HORIZON_VANILLA_JAR").expect("HORIZON_VANILLA_JAR");
    let language: Language = VanillaJar::open(jar.as_ref()).unwrap().load_language();
    assert!(language.len() > 1000, "the jar's en_us table");
    let text = std::fs::read_to_string(path).expect("the oracle's output");
    let tree = build_tree();
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let kind = str_of(&row, "kind").to_owned();
        *counts.entry(kind.clone()).or_default() += 1;
        match kind.as_str() {
            "parse" => {
                let level = row["level"].as_u64().unwrap() as u8;
                let input = str_of(&row, "input");
                let parse = tree.parse(input, allowed(level));
                match (parse.context_chain(), row.get("error")) {
                    (Err(e), Some(java)) => {
                        assert_eq!(e.key(), str_of(java, "key"), "{line}");
                        let args: Vec<String> =
                            e.args().iter().map(|a| a.text_in(&language)).collect();
                        let expected: Vec<&str> = java["args"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|a| a.as_str().unwrap())
                            .collect();
                        assert_eq!(args, expected, "{line}");
                        assert_eq!(
                            e.cursor_utf16() as u64,
                            java["cursor"].as_u64().unwrap(),
                            "{line}"
                        );
                        assert_eq!(e.context().as_deref(), java["context"].as_str(), "{line}");
                    }
                    (Ok(chain), None) => {
                        let java = row["ok"].as_object().unwrap();
                        let ours: std::collections::BTreeMap<String, String> = chain
                            .executable
                            .arguments()
                            .iter()
                            .map(|a| (a.name.to_string(), argument_text(&a.value)))
                            .collect();
                        let expected: std::collections::BTreeMap<String, String> = java
                            .iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                            .collect();
                        assert_eq!(ours, expected, "{line}");
                    }
                    (ours, java) => panic!("{line}: ours {:?}, java {java:?}", ours.map(|_| ())),
                }
            }
            "usage" => {
                let level = row["level"].as_u64().unwrap() as u8;
                let node = tree
                    .child(crate::command::NodeId::ROOT, str_of(&row, "command"))
                    .unwrap();
                let ours =
                    tree.all_usage(node, allowed(level), row["restricted"].as_bool().unwrap());
                let java: Vec<&str> = row["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l.as_str().unwrap())
                    .collect();
                assert_eq!(ours, java, "{line}");
            }
            "packet" => {
                let level = row["level"].as_u64().unwrap() as u8;
                let packet = PlayerCommands::new(&tree, allowed(level), false).to_packet(
                    |key| matches!(key, Key::Permission(n) if n > 0),
                    |_| SuggestionProvider::AskServer,
                );
                let mut bytes = Vec::new();
                packet.azalea_write(&mut bytes).unwrap();
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                assert_eq!(hex, str_of(&row, "hex"), "level {level}");
            }
            "render" => {
                let args = row["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(argument)
                    .collect();
                let component = source::translatable(str_of(&row, "key"), args);
                assert_eq!(
                    component.to_string_in(&language),
                    str_of(&row, "text"),
                    "{line}"
                );
            }
            "unknown" => {
                let input = str_of(&row, "input");
                let parse = tree.parse(input, allowed(4));
                let error = parse
                    .context_chain()
                    .expect_err("a line that does not parse");
                assert_eq!(
                    unknown_command_message(&error).to_string_in(&language),
                    str_of(&row, "text"),
                    "{line}"
                );
            }
            other => panic!("kind {other}"),
        }
    }
    println!("rows: {counts:?}");
    assert!(counts.len() == 5 && counts.values().all(|n| *n > 0));
}
