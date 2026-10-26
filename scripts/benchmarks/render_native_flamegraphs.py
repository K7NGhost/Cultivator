"""Render readable flamegraphs while retaining full symbols in raw profiles."""
import argparse
from collections import Counter
from pathlib import Path
import re
from profile_windows import flamegraph


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for source, name in (("before-cpu", "e01-before"), ("after-cpu", "e01-after"),
                         ("app-idle-symbols", "obsolete-file-view-scan")):
        stacks = Counter()
        for line in (args.directory / (source + ".folded")).read_text(encoding="utf-8").splitlines():
            stack, count = line.rsplit(" ", 1)
            frames = []
            for frame in stack.split(";"):
                frame = re.sub(r"enum2\$<([^<>]+)>", r"\1", frame)
                frames.append(frame.split("<", 1)[0])
            stacks[tuple(frames)] += int(count)
        path = args.output / (name + ".svg")
        flamegraph(stacks, path, name + " (generic arguments omitted; native stack observations)")
        print(path)


if __name__ == "__main__":
    main()
