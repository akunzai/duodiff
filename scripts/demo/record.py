#!/usr/bin/env python3
import codecs
import fcntl
import json
import os
import pty
import select
import signal
import struct
import termios
import time

HOME = os.environ["DUODIFF_DEMO_HOME"]
BIN = os.environ["DUODIFF_DEMO_BIN"]
STEPS_FILE = os.environ["DUODIFF_DEMO_STEPS"]
OUT = os.environ["DUODIFF_DEMO_CAST"]
WORK = os.path.abspath(os.path.join(HOME, "work"))
LEFT = os.path.abspath(os.path.join(WORK, "left"))
RIGHT = os.path.abspath(os.path.join(WORK, "right"))
COLS = int(os.environ.get("DUODIFF_DEMO_COLS", "100"))
ROWS = int(os.environ.get("DUODIFF_DEMO_ROWS", "30"))

KEYS = {
    "enter": "\r",
    "esc": "\x1b",
    "tab": "\t",
    "up": "\x1b[A",
    "down": "\x1b[B",
    "right": "\x1b[C",
    "left": "\x1b[D",
}

THEME = {
    "fg": "#c0caf5",
    "bg": "#1a1b26",
    "palette": ":".join([
        "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7",
        "#7dcfff", "#a9b1d6", "#414868", "#f7768e", "#9ece6a", "#e0af68",
        "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5"
    ])
}

def keybytes(name):
    return KEYS.get(name, name).encode()

def main():
    steps = json.loads(open(STEPS_FILE).read())
    env = dict(os.environ)
    # Crossterm honours NO_COLOR (https://no-color.org/) and omits SGR palette codes.
    env.pop("NO_COLOR", None)
    env["FORCE_COLOR"] = "1"
    env["XDG_CONFIG_HOME"] = os.path.join(HOME, "xdg")
    env["XDG_CACHE_HOME"] = os.path.join(HOME, "xdg", "cache")
    env["TERM"] = "xterm-256color"
    env["COLORTERM"] = "truecolor"
    env["COLUMNS"] = str(COLS)
    env["LINES"] = str(ROWS)

    pid, master = pty.fork()
    if pid == 0:
        os.chdir(WORK)
        os.execvpe(BIN, [BIN, "--no-update-check", LEFT, RIGHT], env)
        os._exit(127)

    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    events = []
    start = time.time()
    dec = codecs.getincrementaldecoder("utf-8")("replace")

      # Initial settle time
    def drain(timeout):
        end = time.time() + timeout
        while True:
            remaining = end - time.time()
            if remaining <= 0:
                return True
            r, _, _ = select.select([master], [], [], remaining)
            if master in r:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                text = dec.decode(data)
                if text:
                    events.append([round(time.time() - start, 4), "o", text])

    alive = drain(1.5)
    for step in steps:
        if not alive:
            break
        kind, val = step
        if kind == "wait":
            alive = drain(float(val))
        elif kind == "key":
            os.write(master, keybytes(val))
            # Merge keys need a little longer for diff refresh + status toast.
            pause = 0.55 if val in {"]", "["} else 0.35
            alive = drain(pause)
    drain(0.8)

    try:
        os.write(master, b"q")
        drain(0.5)
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass
    try:
        os.waitpid(pid, 0)
    except OSError:
        pass

    header = {
        "version": 2,
        "width": COLS,
        "height": ROWS,
        "timestamp": int(start),
        "env": {"TERM": "xterm-256color", "SHELL": "/bin/zsh"},
        "theme": THEME
    }
    with open(OUT, "w") as f:
        f.write(json.dumps(header) + "\n")
        for e in events:
            f.write(json.dumps(e) + "\n")
    print(f"wrote {OUT}: {len(events)} events")

if __name__ == "__main__":
    main()
