#!/usr/bin/env python3
"""Split the game binary's `Game` (crates/horizon-client/src/game.rs + game/) into its simulation and
its window, mechanically and repeatably (docs/crates.md):

1. `Frontend`: the fields only a window uses (FRONT below) move into `Game::front`; every access the
   compiler then rejects gets `front.`. The acting player's input (`keys`, `left`, `right`, `clicks`,
   `inv_clicks`) stays in `Sim`: `as_guest` swaps a guest's in (a sneaking guest holds Shift).
2. `Sim`: every other field moves into `Game::sim`; `Game` derefs to `Sim`. Where two borrows of
   `Game` must stay disjoint, the access names `self.sim` explicitly.
3. `impl Sim`: methods that touch neither the window nor a window-only method, and are not named as
   `Game::f`, move into `impl Sim` blocks beside the blocks they came from (same cfg attributes).

The fork keeps merging upstream, which edits these files freely: a merge reverts the commit this
script made, merges, then runs it again. It drives `cargo check` and rewrites only what the compiler
reports, so it is safe to rerun on new code.

    python3 tools/split_game.py            # all steps
    python3 tools/split_game.py --step 3   # one step
"""
import argparse
import collections
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "crates/horizon-client/src")
GAME_RS = os.path.join(SRC, "game.rs")

FRONT = set("""
window renderer physical_keys cursor cursor_icon captured modifiers fullscreen mouse smooth_camera
toggles pad fps fps_frames fps_at world_frames last_input limiter frame_clock frame_ticks dragging fog
screen_fx weather_parameters panorama_rotation lod lod_source_attempted lod_observe lod_covered_at
entity_vertices dynamic_lights luminance see_through_vertices entity_spans glowing_vertices pack_ids
pending_uploads view_frustum entity_culler pack_shadow_casters native_shaders view avatar
border_renderer border_state text_display_vertices world_renderer_ready frame_stats frame_graph
debug_charts debug_keys heights sway scope_scale audio music ambience sound_paused cart_sounds
riding_cart_sounds subtitles gui health_hud tool_highlight saving_indicator boss_overlay tab_list
waypoint_styles waypoints clipboard text_selection gamemode_switcher map_patches bundle_mouse
title creative_screen clicks inv_clicks spectator_gui control
""".split())

BORROW_CODES = ("E0502", "E0499", "E0503", "E0506", "E0505", "E0500", "E0501", "E0597", "E0716")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def write(path, text):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def game_files():
    out = [GAME_RS]
    for dirpath, _, names in os.walk(os.path.join(SRC, "game")):
        out += [os.path.join(dirpath, n) for n in names if n.endswith(".rs")]
    return sorted(out)


def check():
    """`cargo check` of the game binary (and its tests), as compiler messages."""
    out = subprocess.run(
        ["cargo", "check", "-p", "horizon-client", "--bins", "--tests", "--features", "mods",
         "--message-format", "json"],
        cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True).stdout
    messages = []
    for line in out.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-message" and msg["message"]["level"] == "error":
            messages.append(msg["message"])
    return messages


def struct_span(text, header):
    start = text.index(header)
    end = text.index("\n}\n", start)
    return start, end


def struct_items(lines):
    """A struct body's fields: (lines with their docs and attributes, name)."""
    items, buf, depth = [], [], 0
    for line in lines:
        buf.append(line)
        s = line.strip()
        if s.startswith(("//", "#")):
            continue
        depth += sum(s.count(c) for c in "{([<") - sum(s.count(c) for c in "})]>") + s.count("->")
        if depth <= 0 and s.endswith(","):
            first = next(x for x in buf if not x.strip().startswith(("//", "#")))
            name = re.search(r"^\s*(?:pub(?:\([\w:]+\))? )?(\w+):", first).group(1)
            items.append((buf, name))
            buf, depth = [], 0
    return items, buf


def literal_entries(text, start):
    """The top-level `name: expr,` entries of a struct literal whose body starts at `start`."""
    entries, k, depth, begin = [], start, 0, start
    while True:
        ch = text[k]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            entries.append(text[begin:k + 1])
            begin = k + 1
        elif ch == '"':
            k += 1
            while text[k] != '"':
                k += 2 if text[k] == "\\" else 1
        k += 1
    return entries, text[begin:k], k


def entry_name(entry):
    return re.match(r"\s*(?://[^\n]*\n\s*)*(\w+)", entry).group(1)


def step_front():
    text = read(GAME_RS)
    if "\nstruct Frontend {" in text:
        print("front: already split")
        return
    lines = text.split("\n")
    s = next(i for i, l in enumerate(lines) if l.startswith("struct Game<'a> {"))
    e = s + 1
    while not lines[e].startswith("}"):
        e += 1
    items, tail = struct_items(lines[s + 1:e])
    missing = FRONT - {n for _, n in items}
    if missing:
        sys.exit(f"front: no such Game fields: {sorted(missing)}")
    keep = [l for b, n in items if n not in FRONT for l in b]
    front = [l for b, n in items if n in FRONT for l in b]
    lines = (lines[:s + 1] + keep + tail
             + ["    /// What only a window has: input, rendering, sounds and the HUD's widgets.", "    front: Frontend,"]
             + lines[e:e + 1] + ["",
                                 "/// The window's half of `Game`: input, rendering, sounds and HUD widgets.",
                                 "struct Frontend {"] + front + ["}"] + lines[e + 1:])
    text = "\n".join(lines)
    j = text.index("        Ok(Game {\n") + len("        Ok(Game {\n")
    entries, rest, k = literal_entries(text, j)
    fe = [x for x in entries if entry_name(x) in FRONT]
    ke = [x for x in entries if entry_name(x) not in FRONT]
    if {entry_name(x) for x in fe} != FRONT:
        sys.exit("front: the Game literal does not set every front field")
    init = "\n            front: Frontend {" + "".join(x.replace("\n", "\n    ") for x in fe) + "\n            },"
    write(GAME_RS, text[:j] + "".join(ke) + init + rest + text[k:])
    for round_ in range(10):
        fixes = collections.defaultdict(set)
        for m in check():
            mm = re.match(r"no field `(\w+)` on type `(?:&(?:mut )?)?(?:game::)?Game", m["message"])
            primary = [sp for sp in m["spans"] if sp["is_primary"]]
            if mm and primary:
                sp = primary[0]
                fixes[sp["file_name"]].add((sp["line_start"], sp["column_start"], mm.group(1)))
        if not fixes:
            break
        n = 0
        for f, found in fixes.items():
            path = os.path.join(ROOT, f)
            src = read(path).split("\n")
            for line, col, name in sorted(found, key=lambda x: (x[0], -x[1])):
                src[line - 1] = src[line - 1][:col - 1] + "front." + src[line - 1][col - 1:]
                n += 1
            write(path, "\n".join(src))
        print(f"front: round {round_ + 1}, {n} accesses")


def sim_fields():
    text = read(GAME_RS)
    s, e = struct_span(text, "struct Sim<'a> {\n")
    return set(re.findall(r"^    (\w+):", text[s:e], re.M))


def explicit_sim(messages, names, window=(7, 3), calls=False):
    """On the lines of borrow errors, name `self.sim` for the simulation's fields and methods."""
    pat = re.compile(r"\b(self|game|g)(\s*)\.(" + "|".join(sorted(names, key=len, reverse=True))
                     + (r")\b" if calls else r")\b(?!\s*\()"))
    lines = collections.defaultdict(set)
    for m in messages:
        if m.get("code") and m["code"]["code"] in BORROW_CODES:
            for sp in m["spans"]:
                lines[sp["file_name"]].update(range(sp["line_start"], sp["line_end"] + 1))
    n = 0
    for f, found in lines.items():
        path = os.path.join(ROOT, f)
        src = read(path).split("\n")
        for line in sorted(found):
            a, b = max(0, line - window[0]), min(len(src), line + window[1])
            seg = "\n".join(src[a:b])
            new = pat.sub(lambda mm: f"{mm.group(1)}{mm.group(2)}.sim.{mm.group(3)}", seg)
            if new != seg:
                src[a:b] = new.split("\n")
                n += 1
        write(path, "\n".join(src))
    return n


def step_sim():
    text = read(GAME_RS)
    if "\nstruct Sim<'a> {" not in text:
        s = text.index("struct Game<'a> {\n")
        e = text.index("\n}\n", s) + 3
        body = text[s + len("struct Game<'a> {\n"):e - 3]
        fm = "    /// What only a window has: input, rendering, sounds and the HUD's widgets.\n    front: Frontend,"
        if fm not in body:
            sys.exit("sim: run the front step first")
        body = body.replace(fm, "").rstrip() + "\n"
        new = ("struct Game<'a> {\n    /// The simulation: the world, its players and entities, rules and the server side.\n"
               "    /// `Game` derefs to it.\n    sim: Sim<'a>,\n" + fm + "\n}\n"
               "/// Everything of `Game` but the window (`Frontend`): what a server runs.\nstruct Sim<'a> {\n" + body + "}\n"
               "impl<'a> std::ops::Deref for Game<'a> {\n    type Target = Sim<'a>;\n"
               "    fn deref(&self) -> &Sim<'a> {\n        &self.sim\n    }\n}\n"
               "impl<'a> std::ops::DerefMut for Game<'a> {\n"
               "    fn deref_mut(&mut self) -> &mut Sim<'a> {\n        &mut self.sim\n    }\n}\n")
        text = text[:s] + new + text[e:]
        j = text.index("        Ok(Game {\n") + len("        Ok(Game {\n")
        fi = text.index("            front: Frontend {", j)
        end = text.index("\n        })", fi)
        text = (text[:j] + "            sim: Sim {\n" + text[j:fi].replace("\n", "\n    ").rstrip(" ")
                + "            },\n" + text[fi:end] + text[end:])
        write(GAME_RS, text)
    names = sim_fields()
    for round_ in range(12):
        n = explicit_sim(check(), names)
        print(f"sim: round {round_ + 1}, {n} lines")
        if not n:
            break


HDR = re.compile(r"^impl(?:<'a>)? (?:super::|crate::game::)?Game<'(?:_|a)> \{$")
FN = re.compile(r"^    (?:pub(?:\([\w:]+\))? )?(?:const )?(?:async )?(?:unsafe )?fn (\w+)")


def parse_blocks(lines):
    blocks, i = [], 0
    while i < len(lines):
        if HDR.match(lines[i]):
            j, methods, pending = i + 1, [], i + 1
            while not lines[j].startswith("}"):
                m = FN.match(lines[j])
                if m:
                    a = j
                    while a > pending and re.match(r"^    (///|#\[|//)", lines[a - 1]):
                        a -= 1
                    k = j
                    if not lines[j].rstrip().endswith("{}"):
                        while lines[k] != "    }":
                            k += 1
                    methods.append({"name": m.group(1), "a": a, "b": k + 1, "body": "\n".join(lines[j:k + 1])})
                    pending = j = k + 1
                    continue
                j += 1
            attrs, a = [], i
            while a > 0 and lines[a - 1].startswith("#["):
                attrs.insert(0, lines[a - 1])
                a -= 1
            blocks.append({"start": i, "end": j, "methods": methods, "attrs": attrs})
            i = j
        i += 1
    return blocks


def classify(extra):
    methods = collections.defaultdict(list)
    for f in game_files():
        for b in parse_blocks(read(f).split("\n")):
            for m in b["methods"]:
                methods[m["name"]].append(m)
    game = set(extra)
    for f in game_files():
        game |= set(re.findall(r"\b(?:Game|Self)::(\w+)", read(f)))
    for name, ms in methods.items():
        for m in ms:
            body = m["body"]
            if (not re.search(r"\bself\b", body.split("{", 1)[0]) or re.search(r"\b(?:self|game|g|this)\s*\.\s*front\b", body)
                    or re.search(r"[(,]\s*(&mut \*)?self\s*[,)]", body) or re.search(r"\bGame\b", body)
                    or re.search(r"\bSelf\b", body.split("\n", 1)[1] if "\n" in body else "")):
                game.add(name)
    calls = {n: set(re.findall(r"\bself\s*\.\s*(\w+)\s*(?:::<[^>]*>)?\s*\(", " ".join(m["body"] for m in ms)))
             for n, ms in methods.items()}
    changed = True
    while changed:
        changed = False
        for n, c in calls.items():
            if n not in game and c & game:
                game.add(n)
                changed = True
    return methods, game


def move_methods(game):
    moved = 0
    for f in game_files():
        lines = read(f).split("\n")
        blocks = parse_blocks(lines)
        out, last = [], 0
        for b in blocks:
            sim = [m for m in b["methods"] if m["name"] not in game]
            if not sim:
                continue
            taken = set()
            for m in sim:
                taken |= set(range(m["a"], m["b"]))
            out += lines[last:b["start"]]
            out += [l for idx, l in enumerate(lines[b["start"]:b["end"] + 1], start=b["start"]) if idx not in taken]
            out += b["attrs"] + ["impl crate::game::Sim<'_> {"]
            for m in sim:
                out += [re.sub(r"\bself(\s*)\.sim\.", r"self\1.", l) for l in lines[m["a"]:m["b"]]]
            out.append("}")
            last = b["end"] + 1
            moved += len(sim)
        out += lines[last:]
        write(f, "\n".join(out))
    return moved


def failing_sim_methods(messages):
    """Methods in `impl Sim` blocks the compiler rejects: they need `Game`."""
    names, cache = set(), {}
    for m in messages:
        primary = [sp for sp in m["spans"] if sp["is_primary"]]
        if not primary:
            continue
        sp = primary[0]
        path = os.path.join(ROOT, sp["file_name"])
        src = cache.setdefault(path, read(path).split("\n"))
        i, name = sp["line_start"] - 1, None
        while i >= 0 and not src[i].startswith(("impl", "}")):
            mm = FN.match(src[i])
            if mm:
                name = mm.group(1)
                break
            i -= 1
        j = i
        while j >= 0 and not src[j].startswith("impl"):
            j -= 1
        if name and j >= 0 and "Sim" in src[j]:
            names.add(name)
    return names


def step_methods():
    snapshot = {f: read(f) for f in game_files()}
    extra = set()
    for round_ in range(12):
        _, game = classify(extra)
        moved = move_methods(game)
        messages = check()
        failing = failing_sim_methods(messages) - extra
        print(f"methods: round {round_ + 1}, {moved} in impl Sim, {len(failing)} to keep on Game")
        if not failing:
            break
        extra |= failing
        for f, text in snapshot.items():
            write(f, text)
    names = sim_fields() | (set(classify(extra)[0]) - classify(extra)[1])
    for round_ in range(6):
        n = explicit_sim(check(), names, window=(0, 1), calls=True)
        print(f"methods: borrow round {round_ + 1}, {n} lines")
        if not n:
            break


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--step", type=int, choices=[1, 2, 3])
    args = parser.parse_args()
    steps = {1: step_front, 2: step_sim, 3: step_methods}
    for n in [args.step] if args.step else [1, 2, 3]:
        steps[n]()
    subprocess.run(["cargo", "fmt", "-p", "horizon-client"], cwd=ROOT)
    left = check()
    for m in left[:20]:
        sp = next((s for s in m["spans"] if s["is_primary"]), None)
        print(f"error: {m['message']}" + (f" ({sp['file_name']}:{sp['line_start']})" if sp else ""))
    sys.exit(1 if left else 0)


if __name__ == "__main__":
    main()
