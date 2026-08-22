import { defineVideo } from "tcut";
import { execSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const rootDir = process.cwd();
const castDir = mkdtempSync(join(tmpdir(), "duodiff-demo-"));
const demoHome = join(castDir, "demo-home");
const workDir = join(demoHome, "work");
const leftDir = join(workDir, "left");
const rightDir = join(workDir, "right");

const SAME_BYTES = "Hello, this file is exactly the same on both sides.\n";
const SAME_TIME = 1_700_000_000;
const NEWER_TIME = 1_700_086_400;

const FILES_LEFT: Record<string, string> = {
  "identical.txt": SAME_BYTES,
  "timestamp_only.txt": SAME_BYTES,
  "diff_left_newer.txt": "This file differs.\nLeft side has extra lines.\n",
  "left_only.txt": "This file exists only on the left side.\n",
  "merge_demo.txt": "mode = fast\nenabled = yes\nhost = alpha\n",
  "nested_dir/nested.txt": "Nested file.\nLeft version.\n",
};

const FILES_RIGHT: Record<string, string> = {
  "identical.txt": SAME_BYTES,
  "timestamp_only.txt": SAME_BYTES,
  "diff_left_newer.txt": "This file differs.\n",
  "diff_right_newer.txt": "This file differs.\nRight side is newer.\n",
  "right_only.txt": "This file exists only on the right side.\n",
  "merge_demo.txt": "mode = slow\nenabled = yes\nhost = beta\n",
  "nested_dir/nested.txt": "Nested file.\nRight version.\n",
};

function seedDemo() {
  mkdirSync(leftDir, { recursive: true });
  mkdirSync(rightDir, { recursive: true });

  for (const [name, content] of Object.entries(FILES_LEFT)) {
    const p = join(leftDir, name);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, content);
  }

  for (const [name, content] of Object.entries(FILES_RIGHT)) {
    const p = join(rightDir, name);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, content);
  }

  for (const side of [leftDir, rightDir]) {
    utimesSync(join(side, "identical.txt"), SAME_TIME, SAME_TIME);
  }
  utimesSync(join(leftDir, "timestamp_only.txt"), SAME_TIME, SAME_TIME);
  utimesSync(join(rightDir, "timestamp_only.txt"), NEWER_TIME, NEWER_TIME);

  const configDir = join(demoHome, "xdg", "duodiff");
  mkdirSync(configDir, { recursive: true });
  writeFileSync(
    join(configDir, "config.toml"),
    'theme = "dark"\nscan_mode = "precise"\ncheck_updates = false\n',
  );
}

process.on("exit", () => {
  try {
    rmSync(castDir, { recursive: true, force: true });
  } catch {
    // Cleanup must not replace a successful render with an exit error.
  }
});

// Build duodiff before recording starts so build logs never enter the PTY.
execSync("cargo build --release", { cwd: rootDir, stdio: "ignore" });
seedDemo();

export default defineVideo(
  {
    output: "website/demo.gif",
    cast: join(castDir, "demo.cast"),
    theme: "dracula",
    cols: 100,
    rows: 30,
    fps: 18,
    typingSpeed: 48,
    maxPause: "1.5s",
    shadow: true,
    title: "duodiff — directory diff TUI",
    requires: ["cargo"],
  },
  async (t) => {
    await t.hide(async () => {
      await t.run(
        `export DUODIFF_DEMO_HOME=${JSON.stringify(demoHome)} && export DUODIFF_DEMO_BIN=${JSON.stringify(join(rootDir, "target/release/duodiff"))} && export PATH=${JSON.stringify(`${rootDir}/target/release:$PATH`)} && export XDG_CONFIG_HOME=${JSON.stringify(join(demoHome, "xdg"))} && export XDG_CACHE_HOME=${JSON.stringify(join(demoHome, "xdg", "cache"))} && export HOME=${JSON.stringify(demoHome)} && export USERPROFILE=${JSON.stringify(demoHome)} && unset NO_COLOR && cd ${JSON.stringify(workDir)}`,
      );
      await t.clear();
      await t.type("duodiff --no-update-check left right\n");
      await t.wait(/duodiff/i, { scope: "screen" });
      await t.sleep("500ms");
    });

    await t.snapshot("website/tree-view.png");
    await t.sleep("1.2s");

    await t.type("2");
    await t.sleep("0.5s");
    await t.type("1");
    await t.sleep("0.6s");
    await t.type("/");
    await t.sleep("0.4s");
    await t.type("merge");
    await t.sleep("0.4s");
    await t.type("\n");
    await t.sleep("0.6s");
    await t.type("\n");
    await t.sleep("2.5s");
    await t.type("]");
    await t.sleep("2.8s");
    await t.type("N");
    await t.sleep("1.0s");
    await t.type("]");
    await t.sleep("2.8s");
    await t.escape();
    await t.sleep("1.0s");
    await t.type("q");
    await t.sleep("0.5s");
  },
);
