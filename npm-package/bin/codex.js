#!/usr/bin/env node

import { spawn, spawnSync } from "child_process";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const binaryPath = join(__dirname, "codex");
const nativeBinaryPath = join(__dirname, "codex.bin");
const TERMUX_PREFIX = process.env.PREFIX || "/data/data/com.termux/files/usr";

function sanitizeLdLibraryPath(binDir) {
  const blocked = new Set([
    `${TERMUX_PREFIX}/lib`,
    `${TERMUX_PREFIX}/libexec`,
    "/data/data/com.termux/files/usr/lib",
    "/data/data/com.termux/files/usr/libexec",
  ]);

  const extraPaths = (process.env.LD_LIBRARY_PATH || "")
    .split(":")
    .filter((entry) => entry && !blocked.has(entry));

  return [binDir, ...extraPaths].join(":");
}

const env = { ...process.env, CODEX_MANAGED_BY_NPM: "1" };
const binDir = __dirname;
// Hidden arg0 aliases must target the native ELF directly. Pointing them at
// the shell wrapper would lose the special argv[0] when it execs codex.bin.
env.CODEX_SELF_EXE = nativeBinaryPath;
env.LD_LIBRARY_PATH = sanitizeLdLibraryPath(binDir);

let cachedSubcommands;

function detectSubcommands() {
  if (cachedSubcommands !== undefined) {
    return cachedSubcommands;
  }

  const helpResult = spawnSync(binaryPath, ["--help"], {
    encoding: "utf8",
    env,
  });

  if (helpResult.error || helpResult.status !== 0) {
    cachedSubcommands = null;
    return cachedSubcommands;
  }

  const output = `${helpResult.stdout || ""}\n${helpResult.stderr || ""}`;
  const commands = new Set();
  let inCommandsSection = false;

  for (const line of output.split(/\r?\n/)) {
    if (!inCommandsSection) {
      if (/^\s*Commands:\s*$/.test(line)) {
        inCommandsSection = true;
      }
      continue;
    }

    if (/^\s*(Arguments|Options):\s*$/.test(line)) {
      break;
    }

    const commandMatch = line.match(/^\s{2,}([a-z0-9][a-z0-9-]*)\s{2,}/i);
    if (!commandMatch) {
      continue;
    }

    commands.add(commandMatch[1]);

    const aliasesMatch = line.match(/\[aliases?: ([^\]]+)\]/);
    if (aliasesMatch?.[1]) {
      for (const alias of aliasesMatch[1].split(",")) {
        const cleanAlias = alias.trim();
        if (cleanAlias) {
          commands.add(cleanAlias);
        }
      }
    }
  }

  cachedSubcommands = commands.size > 0 ? commands : null;
  return cachedSubcommands;
}

const args = process.argv.slice(2);
const first = args[0];
const isOption = first?.startsWith("-");
const knownSubcommands = first && !isOption ? detectSubcommands() : null;
const isKnownSubcommand = Boolean(first && knownSubcommands?.has(first));

const finalArgs =
  args.length === 0
    ? []
    : isOption || isKnownSubcommand || knownSubcommands === null
      ? args
      : ["exec", ...args];

const child = spawn(binaryPath, finalArgs, {
  stdio: "inherit",
  env,
});

child.on("error", (error) => {
  console.error(`Failed to launch bundled Codex binary: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
