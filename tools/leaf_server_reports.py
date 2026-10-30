#!/usr/bin/env python3
"""Write vanilla's data generator reports for horizon-server, once per server directory.

horizon-server runs without a JVM and the repository holds no Mojang data, so the block state table
(and later other registries) comes from the server owner's own 26.3 server jar: this tool runs
vanilla's data generator (`net.minecraft.data.Main --reports`) on it and copies `reports/` to
`<server dir>/cache/vanilla-26.3/reports/` (`blocks.json`, `registries.json`, ...), which the server
reads at startup (crates/horizon-server/ARCHITECTURE.md, "Data"). It then runs
`mods/horizon/src/test/java/dev/horizonlab/leafserver/LeafBlockStatesExporter.java`
(Java's single-file launcher, after `Bootstrap.bootStrap()`) on the
same jar, which writes each block state's cached flags and shapes to
`<server dir>/cache/vanilla-26.3/block_states.bin` (crates/horizon-server/src/block/flags.rs).

  python3 tools/leaf_server_reports.py --jar server-26.3.jar --server-dir path/to/server
  python3 tools/leaf_server_reports.py --jar extracted/server.jar --libraries libraries/ --server-dir .

The jar is either Mojang's bundler jar (with `META-INF/versions.list`), run with
`-DbundlerMainClass`, or an extracted server jar whose libraries are every `.jar` under
`--libraries`.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile


def classpath(jar, libraries):
    jars = [jar] + sorted(str(p) for p in Path(libraries).rglob("*.jar"))
    return os.pathsep.join(jars)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--jar", required=True, help="the vanilla 26.3 server jar (bundler or extracted)")
    parser.add_argument("--libraries", help="for an extracted jar: a folder holding its library jars")
    parser.add_argument("--server-dir", default=".", help="the horizon-server directory (default: .)")
    parser.add_argument("--java", default=shutil.which("java") or "java", help="a Java 25 `java`")
    args = parser.parse_args()

    jar = str(Path(args.jar).resolve())
    with zipfile.ZipFile(jar) as archive:
        bundler = "META-INF/versions.list" in archive.namelist()
    if bundler:
        command = [args.java, "-DbundlerMainClass=net.minecraft.data.Main", "-jar", jar]
    elif args.libraries:
        command = [args.java, "-cp", classpath(jar, args.libraries), "net.minecraft.data.Main"]
    else:
        sys.exit("an extracted server jar needs --libraries")

    cache = Path(args.server_dir).resolve() / "cache" / "vanilla-26.3"
    target = cache / "reports"
    with tempfile.TemporaryDirectory(prefix="horizon-server-reports-") as work:
        # The generator writes `out/` and, for the bundler, `libraries/` and `versions/` in its
        # working directory: a scratch folder keeps them out of the server directory.
        result = subprocess.run(command + ["--reports", "--output", "out"], cwd=work)
        if result.returncode != 0:
            sys.exit(f"the data generator failed (exit {result.returncode})")
        reports = Path(work) / "out" / "reports"
        if not (reports / "blocks.json").is_file():
            sys.exit("the data generator wrote no reports/blocks.json")
        if target.exists():
            shutil.rmtree(target)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(reports, target)
        print(f"wrote {target}")

        # The bundler has extracted the server jar and its libraries into the working directory.
        if bundler:
            server = sorted(Path(work, "versions").rglob("*.jar"))
            if not server:
                sys.exit("the bundler extracted no server jar under versions/")
            cp = classpath(str(server[0]), Path(work, "libraries"))
        else:
            cp = classpath(jar, args.libraries)
        exporter = (Path(__file__).resolve().parents[1] / "mods/horizon/src/test/java/dev/horizonlab"
                    / "leafserver/LeafBlockStatesExporter.java")
        states = Path(work) / "block_states.bin"
        result = subprocess.run([args.java, "-cp", cp, str(exporter), str(states)], cwd=work)
        if result.returncode != 0 or not states.is_file():
            sys.exit(f"the block state exporter failed (exit {result.returncode})")
        shutil.copyfile(states, cache / "block_states.bin")
    print(f"wrote {cache / 'block_states.bin'}")


if __name__ == "__main__":
    main()
