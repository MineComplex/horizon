//! Minecraft's argument types against `testdata/arguments.txt`: what the real 26.3 classes parse,
//! fail with and suggest for each input, local coordinates, `Mth.wrapDegrees` and the scoreboard
//! operations. The file is the output of a single-file Java program run on the server jar (not
//! committed): `P` parse results, `S` suggestions, `L` local positions (raw double bits), `W`
//! wrapped angles (raw float bits), `O` operations.

use super::*;
use crate::azalea::chat::translatable_component::PrimitiveOrComponent;
use crate::command::{CommandOrigin, SuggestionsBuilder, local_position, wrap_degrees};
use crate::core::geometry::Vec3;
use crate::core::position::BlockPos;
use crate::util::number_text::{double_to_string, float_to_string};

const ORACLE: &str = include_str!("testdata/arguments.txt");

fn type_named(name: &str) -> ArgumentType {
    match name {
        "block_pos" => ArgumentType::BlockPos,
        "column_pos" => ArgumentType::ColumnPos,
        "vec3" => ArgumentType::vec3(),
        "vec3_nocenter" => ArgumentType::Vec3 {
            center_correct: false,
        },
        "vec2" => ArgumentType::vec2(),
        "vec2_nocenter" => ArgumentType::Vec2 {
            center_correct: false,
        },
        "rotation" => ArgumentType::Rotation,
        "angle" => ArgumentType::Angle,
        "swizzle" => ArgumentType::Swizzle,
        "int_range" => ArgumentType::IntRange,
        "float_range" => ArgumentType::FloatRange,
        "time" => ArgumentType::time(),
        "time5" => ArgumentType::Time { min: 5 },
        "uuid" => ArgumentType::Uuid,
        "operation" => ArgumentType::Operation,
        "team_color" => ArgumentType::TeamColor,
        _ => panic!("unknown type {name}"),
    }
}

/// The oracle's text for a value.
fn describe(value: &ArgumentValue) -> String {
    let opt = |v: Option<String>| v.unwrap_or_else(|| "_".into());
    match value {
        ArgumentValue::Coordinates(Coordinates::World { x, y, z }) => {
            let wc = |c: &WorldCoordinate| {
                format!(
                    "{}{}",
                    if c.relative { "~" } else { "" },
                    double_to_string(c.value)
                )
            };
            format!("world {} {} {}", wc(x), wc(y), wc(z))
        }
        ArgumentValue::Coordinates(Coordinates::Local { left, up, forwards }) => format!(
            "local {} {} {}",
            double_to_string(*left),
            double_to_string(*up),
            double_to_string(*forwards)
        ),
        ArgumentValue::IntRange(r) => format!(
            "range {} {}",
            opt(r.min.map(|v| v.to_string())),
            opt(r.max.map(|v| v.to_string()))
        ),
        ArgumentValue::FloatRange(r) => format!(
            "range {} {}",
            opt(r.min.map(double_to_string)),
            opt(r.max.map(double_to_string))
        ),
        ArgumentValue::Angle(a) => format!("angle {} {}", float_to_string(a.angle), a.relative),
        ArgumentValue::Swizzle(Swizzle(axes)) => {
            let names: Vec<&str> = ["x", "y", "z"]
                .into_iter()
                .zip(axes)
                .filter_map(|(n, &on)| on.then_some(n))
                .collect();
            format!("swizzle [{}]", names.join(", "))
        }
        ArgumentValue::Integer(v) => format!("Integer {v}"),
        ArgumentValue::Uuid(v) => format!("UUID {v}"),
        ArgumentValue::Operation(_) => "operation".into(),
        ArgumentValue::TeamColor(c) => format!("color {}", c.name()),
        other => panic!("unexpected value {other:?}"),
    }
}

fn describe_error(error: &CommandError) -> String {
    let args: Vec<String> = error
        .args()
        .iter()
        .map(|arg| match arg {
            PrimitiveOrComponent::Integer(v) => v.to_string(),
            PrimitiveOrComponent::String(v) => v.clone(),
            other => panic!("unexpected argument {other:?}"),
        })
        .collect();
    let cursor = match error.input() {
        Some(_) => error.cursor_utf16() as i64,
        None => -1,
    };
    format!("err {cursor} {} [{}]", error.key(), args.join(", "))
}

/// `name |input| rest` after the line's tag.
fn split_case(line: &str) -> (&str, &str, &str) {
    let (name, rest) = line.split_once(" |").unwrap();
    let (input, expected) = rest.split_once("| ").unwrap();
    (name, input, expected)
}

#[test]
fn parsing_matches_java() {
    let mut checked = 0;
    for line in ORACLE.lines().filter_map(|l| l.strip_prefix("P ")) {
        let (name, input, expected) = split_case(line);
        let mut reader = StringReader::new(input);
        let actual = match type_named(name).parse(&mut reader) {
            Ok(value) => format!("ok {} {}", reader.cursor(), describe(&value)),
            Err(error) => describe_error(&error),
        };
        assert_eq!(actual, expected, "{name} {input:?}");
        checked += 1;
    }
    assert!(checked > 400, "{checked}");
}

#[test]
fn suggestions_match_java() {
    for line in ORACLE.lines().filter_map(|l| l.strip_prefix("S ")) {
        let (name, input, expected) = split_case(line);
        let mut builder = SuggestionsBuilder::new(input, 0);
        type_named(name).list_suggestions(&mut builder);
        let built = builder.build();
        let mut actual = format!("{},{}", built.range.start, built.range.end);
        for s in &built.list {
            actual += &format!(" [{},{} {}]", s.range.start, s.range.end, s.text);
        }
        assert_eq!(actual, expected, "{name} {input:?}");
    }
}

#[test]
fn local_coordinates_match_java() {
    let mut checked = 0;
    for line in ORACLE.lines().filter_map(|l| l.strip_prefix("L ")) {
        let f: Vec<&str> = line.split(' ').collect();
        let d = |i: usize| f[i].parse::<f64>().unwrap();
        let rotation = [d(3) as f32, d(4) as f32];
        let out = local_position(Vec3::new(10.5, 64.0, -3.25), rotation, d(0), d(1), d(2));
        let bits = |v: f64| format!("{:x}", v.to_bits());
        assert_eq!(
            [bits(out.x), bits(out.y), bits(out.z)],
            [f[5], f[6], f[7]],
            "{line}"
        );
        checked += 1;
    }
    assert_eq!(checked, 24);
}

#[test]
fn wrap_degrees_matches_java() {
    for line in ORACLE.lines().filter_map(|l| l.strip_prefix("W ")) {
        let (input, expected) = line.split_once(' ').unwrap();
        let input = f32::from_bits(u32::from_str_radix(input, 16).unwrap());
        assert_eq!(
            format!("{:x}", wrap_degrees(input).to_bits()),
            expected,
            "{input}"
        );
    }
}

#[test]
fn operations_match_java() {
    let mut checked = 0;
    for line in ORACLE.lines().filter_map(|l| l.strip_prefix("O ")) {
        let mut f = line.splitn(4, ' ');
        let (symbol, a, b, expected) = (
            f.next().unwrap(),
            f.next().unwrap(),
            f.next().unwrap(),
            f.next().unwrap(),
        );
        let op = ScoreOperation::ALL
            .into_iter()
            .find(|op| op.symbol() == symbol)
            .unwrap();
        let (mut a, mut b) = (a.parse().unwrap(), b.parse().unwrap());
        let actual = match op.apply(&mut a, &mut b) {
            Ok(()) => format!("{a} {b}"),
            Err(error) => describe_error(&error),
        };
        assert_eq!(actual, expected, "{line}");
        checked += 1;
    }
    assert_eq!(checked, 90);
}

fn parse(ty: ArgumentType, input: &str) -> ArgumentValue {
    ty.parse(&mut StringReader::new(input)).unwrap()
}

fn coordinates(ty: ArgumentType, input: &str) -> Coordinates {
    match parse(ty, input) {
        ArgumentValue::Coordinates(c) => c,
        other => panic!("{other:?}"),
    }
}

#[test]
fn coordinates_resolve_against_the_source() {
    let origin = CommandOrigin {
        position: Vec3::new(10.7, 64.0, -3.2),
        anchor: Vec3::new(10.7, 65.62, -3.2),
        rotation: [30.0, -90.0],
    };
    let pos = coordinates(ArgumentType::BlockPos, "~1 ~-0.5 5");
    assert_eq!(pos.position(&origin), Vec3::new(11.7, 63.5, 5.0));
    assert_eq!(pos.block_pos(&origin), BlockPos::new(11, 63, 5));
    assert_eq!(pos.relative_axes(), [true, true, false]);
    // Centre correction: whole absolute x and z only.
    let vec = coordinates(ArgumentType::vec3(), "1 2 ~");
    assert_eq!(vec.position(&origin), Vec3::new(1.5, 2.0, -3.2));
    let column = coordinates(ArgumentType::ColumnPos, "~ -7");
    assert_eq!(column.column_pos(&origin), [10, -7]);
    let vec2 = coordinates(ArgumentType::vec2(), "~1 3");
    assert_eq!(vec2.vec2(&origin), [11.7, 3.5]);
    // `yaw pitch`, kept as `[x_rot, y_rot]`.
    let rotation = coordinates(ArgumentType::Rotation, "~10 45");
    assert_eq!(rotation.rotation(&origin), [45.0, -80.0]);
    // Local coordinates start from the anchor, and have no rotation of their own.
    let local = coordinates(ArgumentType::BlockPos, "^ ^ ^");
    assert_eq!(local.position(&origin), origin.anchor);
    assert_eq!(local.rotation(&origin), [0.0, 0.0]);
    assert_eq!(local.relative_axes(), [true; 3]);
    let ArgumentValue::Angle(angle) = parse(ArgumentType::Angle, "~100") else {
        panic!()
    };
    assert_eq!(angle.resolve(170.0), -90.0);
}

#[test]
fn ranges_match() {
    let ArgumentValue::IntRange(ints) = parse(ArgumentType::IntRange, "-3..2") else {
        panic!()
    };
    assert!(ints.matches(-3) && ints.matches(2) && !ints.matches(3) && !ints.matches(-4));
    // Squared bounds: 9 and 4, so a negative minimum makes this range empty.
    assert!(!ints.matches_sqr(4) && !ints.matches_sqr(9));
    let ArgumentValue::FloatRange(doubles) = parse(ArgumentType::FloatRange, "..2.5") else {
        panic!()
    };
    assert!(doubles.matches(-1e9) && doubles.matches(2.5) && !doubles.matches(2.6));
    assert!(doubles.matches(f64::NAN));
    assert!(doubles.matches_sqr(6.25) && !doubles.matches_sqr(6.3));
}

#[test]
fn team_colors_and_swizzles() {
    assert_eq!(
        TeamColor::ALL.map(TeamColor::id),
        std::array::from_fn(|i| i as i32)
    );
    assert_eq!(TeamColor::Gold.rgb(), 0xffaa00);
    assert_eq!(
        TeamColor::by_name("light_purple"),
        Some(TeamColor::LightPurple)
    );
    let ArgumentValue::Swizzle(Swizzle(axes)) = parse(ArgumentType::Swizzle, "zx") else {
        panic!()
    };
    assert_eq!(
        Vec3::new(1.5, -2.5, -0.5).align(axes),
        Vec3::new(1.0, -2.5, -1.0)
    );
}

#[test]
fn context_getters() {
    use crate::command::{CommandTree, argument, literal};
    let mut tree = CommandTree::<u8>::new();
    tree.register(
        literal("x").then(
            argument("pos", ArgumentType::vec3()).then(
                argument("time", ArgumentType::time()).then(
                    argument("op", ArgumentType::Operation).then(
                        argument("color", ArgumentType::TeamColor)
                            .then(argument("id", ArgumentType::Uuid).executes(1)),
                    ),
                ),
            ),
        ),
    );
    let input = "x ^ ^1 ^ 1.5s %= aqua 1-2-3-4-5";
    let parse = tree.parse(input, |_| true);
    assert!(parse.errors.is_empty() && parse.cursor == input.len());
    let ctx = &parse.context;
    let local = Coordinates::Local {
        left: 0.0,
        up: 1.0,
        forwards: 0.0,
    };
    assert_eq!(ctx.coordinates("pos"), Some(local));
    assert_eq!(ctx.time("time"), Some(30));
    assert_eq!(ctx.operation("op"), Some(ScoreOperation::Modulo));
    assert_eq!(ctx.team_color("color"), Some(TeamColor::Aqua));
    let id = ctx.uuid("id").map(|u| u.to_string());
    assert_eq!(id.as_deref(), Some("00000001-0002-0003-0004-000000000005"));
    assert_eq!(ctx.angle("pos"), None);
}
