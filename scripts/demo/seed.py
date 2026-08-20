#!/usr/bin/env python3
import os
import pathlib
import shutil

HOME = pathlib.Path(os.environ["DUODIFF_DEMO_HOME"])
WORK = HOME / "work"
LEFT = WORK / "left"
RIGHT = WORK / "right"

SAME_BYTES = "Hello, this file is exactly the same on both sides.\n"

# Explicit mtimes so the demo shows every tree state deterministically. Writing
# the two sides milliseconds apart would otherwise leave `identical.txt` with
# differing timestamps, which Fast mode reports as `≈` rather than `=`.
SAME_TIME = 1_700_000_000
NEWER_TIME = 1_700_086_400

FILES_LEFT = {
    "identical.txt": SAME_BYTES,
    # Same bytes, same size, rewritten timestamp — Fast mode cannot tell
    # whether the contents match, so this row is `≈` (content unverified).
    "timestamp_only.txt": SAME_BYTES,
    "diff_left_newer.txt": "This file differs.\nLeft side has extra lines.\n",
    "left_only.txt": "This file exists only on the left side.\n",
    # Two hunks separated by an equal line; first line is a same-length
    # replacement so intraline highlighting is visible in the diff view.
    "merge_demo.txt": (
        "mode = fast\n"
        "enabled = yes\n"
        "host = alpha\n"
    ),
    "nested_dir/nested.txt": "Nested file.\nLeft version.\n",
}

FILES_RIGHT = {
    "identical.txt": SAME_BYTES,
    "timestamp_only.txt": SAME_BYTES,
    "diff_left_newer.txt": "This file differs.\n",
    "diff_right_newer.txt": "This file differs.\nRight side is newer.\n",
    "right_only.txt": "This file exists only on the right side.\n",
    "merge_demo.txt": (
        "mode = slow\n"
        "enabled = yes\n"
        "host = beta\n"
    ),
    "nested_dir/nested.txt": "Nested file.\nRight version.\n",
}

def main():
    if WORK.exists():
        shutil.rmtree(WORK)
    LEFT.mkdir(parents=True, exist_ok=True)
    RIGHT.mkdir(parents=True, exist_ok=True)

    for name, content in FILES_LEFT.items():
        p = LEFT / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)

    for name, content in FILES_RIGHT.items():
        p = RIGHT / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)

    for side in (LEFT, RIGHT):
        os.utime(side / "identical.txt", (SAME_TIME, SAME_TIME))
    os.utime(LEFT / "timestamp_only.txt", (SAME_TIME, SAME_TIME))
    os.utime(RIGHT / "timestamp_only.txt", (NEWER_TIME, NEWER_TIME))

    print(f"seeded comparison trees under {WORK}")

if __name__ == "__main__":
    main()