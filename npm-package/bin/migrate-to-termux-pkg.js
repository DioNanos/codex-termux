// One-time migration from the npm package to the official Termux package.
//
// The npm package is never removed here: it stays installed and inert, and the
// final cleanup is a short list of commands the user runs when they choose to.
//
// Nothing is installed unless the caller is a bare `codex` on a terminal: any
// argument means an automated or scripted use, and parsing subcommands is a
// losing game — "no arguments" cannot be misread.
import { spawnSync } from 'node:child_process';
import {
  chmodSync, existsSync, mkdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MIGRATION_PACKAGE = '@mmmbuto/codex-cli-termux';
export const STATE_FILE = '.termux-migration';

const OFFICIAL_PACKAGE = 'codex';
const DEFAULT_COMMAND_TIMEOUT_MS = 180000;
const MANAGED_BY_ENV = [
  'CODEX_MANAGED_BY_VITE_PLUS',
  'CODEX_MANAGED_BY_PNPM',
  'CODEX_MANAGED_BY_NPM',
  'CODEX_MANAGED_BY_BUN',
];

function run(command, args, env, timeoutMs = DEFAULT_COMMAND_TIMEOUT_MS) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    env,
    timeout: timeoutMs,
    shell: false,
  });
  return {
    ok: !result.error && result.status === 0,
    status: result.status,
    signal: result.signal || null,
    timedOut: Boolean(result.error && result.error.code === 'ETIMEDOUT'),
    stdout: result.stdout || '',
    stderr: result.stderr || '',
  };
}

// The official binary must not inherit what the npm launcher prepared for the
// bundled ELF: its own library directory, the executable identity, or the
// package-manager hints that would make Codex believe it is an npm install.
export function cleanEnv(env, { packageBinDir } = {}) {
  const out = { ...env };
  for (const key of MANAGED_BY_ENV) delete out[key];
  delete out.CODEX_SELF_EXE;
  if (packageBinDir && typeof out.LD_LIBRARY_PATH === 'string') {
    const resolvedTarget = path.resolve(packageBinDir);
    const kept = out.LD_LIBRARY_PATH.split(':')
      .filter(Boolean)
      .filter((entry) => path.resolve(entry) !== resolvedTarget);
    if (kept.length) out.LD_LIBRARY_PATH = kept.join(':');
    else delete out.LD_LIBRARY_PATH;
  }
  return out;
}

// Reasons that keep the migration away. Any argument is enough: `exec` and its
// alias, a global option before it, or a bare prompt all mean scripted use.
export function automatedReason(args = [], env = {}) {
  if (Array.isArray(args) && args.length > 0) return 'no-args';
  if (Object.keys(env).some((key) => key.startsWith('NEXUSCREW_'))) return 'nexuscrew';
  return '';
}

// Resolves the command that would actually run. A symlink back into this
// package is the npm launcher: following it would re-enter this same code.
export function officialTarget({ prefix, packageBinDir }) {
  const official = path.join(prefix, 'bin', OFFICIAL_PACKAGE);
  let real;
  try {
    real = realpathSync(official);
  } catch (_) {
    return { ok: false, reason: 'missing' };
  }
  const ours = path.resolve(packageBinDir);
  const oursRoot = path.dirname(ours);
  if (real === ours || real.startsWith(`${ours}${path.sep}`)
    || real.startsWith(`${oursRoot}${path.sep}`)) {
    return { ok: false, reason: 'points-to-ours' };
  }
  try {
    const stat = statSync(real);
    if (!stat.isFile() || !(stat.mode & 0o111)) return { ok: false, reason: 'not-executable' };
  } catch (_) {
    return { ok: false, reason: 'missing' };
  }
  return { ok: true, path: real };
}

// `dpkg` is taken from the prefix, never from PATH: the package attestation is
// the whole point of this check.
function dpkgOwns(file, env, prefix, timeoutMs) {
  const dpkg = path.join(prefix, 'bin', 'dpkg');
  if (!existsSync(dpkg) || !existsSync(file)) return false;
  const result = run(dpkg, ['-S', file], {
    ...env,
    PATH: [path.join(prefix, 'bin'), '/system/bin', '/usr/bin', '/bin'].join(':'),
  }, timeoutMs);
  if (!result.ok) return false;
  return /(^|\n)codex:/.test(result.stdout);
}

function officialVersionOk(file, env, timeoutMs) {
  const result = run(file, ['--version'], env, timeoutMs);
  return result.ok && /\d+\.\d+\.\d+/.test(result.stdout);
}

// One qualification, used at every handover: the path is outside this package,
// the file is executable, dpkg owns it, and it answers a version in a clean
// environment. A previous success is not evidence about the current file.
function qualify({ prefix, packageBinDir, env, timeoutMs }) {
  const target = officialTarget({ prefix, packageBinDir });
  if (!target.ok) return { ok: false, reason: `official-${target.reason}` };
  if (!dpkgOwns(target.path, env, prefix, timeoutMs)) {
    return { ok: false, reason: 'official-not-dpkg' };
  }
  if (!officialVersionOk(target.path, env, timeoutMs)) {
    return { ok: false, reason: 'official-version' };
  }
  return { ok: true, path: target.path };
}

// dpkg knows about the package even when its command is gone: that is the
// case where a plain install is a no-op and only a reinstall helps.
function packageInstalled(prefix, env, timeoutMs) {
  const dpkg = path.join(prefix, 'bin', 'dpkg');
  if (!existsSync(dpkg)) return false;
  const pinned = {
    ...env,
    PATH: [path.join(prefix, 'bin'), '/system/bin', '/usr/bin', '/bin'].join(':'),
  };
  const status = run(dpkg, ['-W', '-f=${Status}', OFFICIAL_PACKAGE], pinned, timeoutMs);
  if (status.ok && /install ok installed/.test(status.stdout)) return true;
  // `dpkg -s` exits 0 for a package left in `config-files` too: the exit code
  // is not the answer, the Status field is.
  const show = run(dpkg, ['-s', OFFICIAL_PACKAGE], pinned, timeoutMs);
  return show.ok && /^Status:\s*install ok installed\s*$/m.test(show.stdout);
}

function describeCommand(prefix, packageBinDir, env, timeoutMs) {
  const official = path.join(prefix, 'bin', OFFICIAL_PACKAGE);
  if (!existsSync(official)) {
    return packageInstalled(prefix, env, timeoutMs)
      ? '`codex` is missing, but the package is installed'
      : '`codex` is not available';
  }
  const target = officialTarget({ prefix, packageBinDir });
  if (target.ok) return `\`codex\` now points to ${target.path}`;
  if (target.reason === 'points-to-ours') return '`codex` still points to the old npm launcher';
  if (target.reason === 'not-executable') return '`codex` exists but is not executable';
  return '`codex` is present but could not be resolved';
}

// A repair of an already-registered package is `--reinstall`; a first install
// is not. Telling them apart is the difference between a fix and a no-op.
function repairCommand(prefix, env, timeoutMs) {
  const official = path.join(prefix, 'bin', OFFICIAL_PACKAGE);
  if (dpkgOwns(official, env, prefix, timeoutMs)) return 'pkg install --reinstall codex';
  if (!existsSync(official) && packageInstalled(prefix, env, timeoutMs)) {
    return 'pkg install --reinstall codex';
  }
  return 'pkg install codex';
}

function successMessage(statePath) {
  return [
    'Codex for Termux: you are now on the official Termux package.',
    'The npm package is still installed but inert. To remove it, in this order',
    '(npm can take the `codex` command with it, hence the reinstall):',
    `  npm uninstall -g ${MIGRATION_PACKAGE}`,
    '  pkg install --reinstall codex',
    '  hash -r',
    '  codex --version',
    'The old `codex-exec` command belongs to the npm package and stays until then.',
    `This message is shown once; the state is kept in ${statePath}.`,
  ];
}

function uncertainMessage(statePath) {
  return [
    'Codex for Termux: the installation did not report completion; it may still be running.',
    'Wait for it, then check `pkg list-installed codex`.',
    `If it is not installed, delete ${statePath} and run \`pkg install codex\` again.`,
    'For this run the bundled binary is used.',
  ];
}

function recoveryMessage(statePath) {
  return [
    'Codex for Termux: a previous installation did not finish.',
    'Check the package state with `pkg list-installed codex` (or `dpkg -l codex`).',
    `If it is installed and \`codex --version\` works from an official path, delete ${statePath} and keep using it.`,
    'If it is installed but `codex` still runs the old npm launcher, use `pkg install --reinstall codex`.',
    `Otherwise delete ${statePath} and run \`pkg install codex\`.`,
  ];
}

function markerMessage() {
  return [
    'Codex for Termux: the migration state could not be written, so nothing was installed.',
    'Check the free space and the permissions of your home directory, then try again.',
  ];
}

function failureMessage(reason, { prefix, packageBinDir, env, timeoutMs }) {
  return [
    `Codex for Termux: the switch to the official Termux package did not complete (${reason}).`,
    'For this run the bundled binary is used.',
    `State of \`codex\`: ${describeCommand(prefix, packageBinDir, env, timeoutMs)}.`,
    `To repair it by hand: ${repairCommand(prefix, env, timeoutMs)}`,
  ];
}

function removeState(statePath) {
  try {
    rmSync(statePath, { force: true });
  } catch (_) { /* a stale marker only blocks a retry, never the client */ }
}

function readState(statePath) {
  if (!existsSync(statePath)) return { kind: 'absent' };
  let raw;
  try {
    raw = readFileSync(statePath, 'utf8').trim();
  } catch (_) {
    return { kind: 'unreadable' };
  }
  try {
    const parsed = JSON.parse(raw);
    // Only the states this code writes are meaningful: anything else is a
    // record from another version or a damaged one, and it must ask for help
    // instead of pretending to be a live run.
    const known = ['in-progress', 'uncertain', 'done'];
    if (!parsed || !known.includes(parsed.state)) return { kind: 'unreadable' };
    return { kind: parsed.state, pid: parsed.pid, at: parsed.at };
  } catch (_) {
    return { kind: 'unreadable' };
  }
}

function processAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return Boolean(error && error.code === 'EPERM');
  }
}

// Returns { action: 'already' | 'skipped' | 'migrated' | 'failed', reason?,
// officialPath? }.
export function runMigration(options = {}) {
  const env = options.env || process.env;
  const prefix = options.prefix || env.PREFIX || '/data/data/com.termux/files/usr';
  const home = options.home || env.HOME || os.homedir();
  const write = options.stderr || ((line) => process.stderr.write(`${line}\n`));
  const packageBinDir = options.packageBinDir || path.dirname(fileURLToPath(import.meta.url));
  const args = Array.isArray(options.args) ? options.args : [];
  const timeoutMs = Number(options.commandTimeoutMs) > 0
    ? Number(options.commandTimeoutMs) : DEFAULT_COMMAND_TIMEOUT_MS;
  const interactive = options.interactive
    ?? Boolean(process.stdin.isTTY && process.stdout.isTTY);
  const statePath = path.join(home, '.codex', STATE_FILE);

  if (env.CODEX_TERMUX_SKIP_MIGRATION === '1') {
    return { action: 'skipped', reason: 'disabled' };
  }
  if (!path.isAbsolute(prefix)) {
    return { action: 'skipped', reason: 'bad-prefix' };
  }
  const automated = automatedReason(args, env);
  if (automated) {
    return { action: 'skipped', reason: automated };
  }
  if (!interactive) {
    return { action: 'skipped', reason: 'no-tty' };
  }

  const childEnv = cleanEnv(env, { packageBinDir });
  const state = readState(statePath);

  if (state.kind === 'unreadable') {
    for (const line of recoveryMessage(statePath)) write(line);
    return { action: 'failed', reason: 'recovery-required' };
  }
  if (state.kind === 'in-progress') {
    // An abandoned claim cannot be told apart from a slow live one without
    // proof of quiescence, so it stays blocked — but it says so, instead of
    // looking like a healthy concurrent run forever.
    if (processAlive(state.pid)) return { action: 'skipped', reason: 'concurrent' };
    for (const line of recoveryMessage(statePath)) write(line);
    return { action: 'failed', reason: 'recovery-required' };
  }
  if (state.kind === 'uncertain') {
    for (const line of recoveryMessage(statePath)) write(line);
    return { action: 'failed', reason: 'recovery-required' };
  }
  if (state.kind === 'done') {
    // The marker records that a switch happened, not that the command is still
    // what it was: qualify again before handing over, without touching pkg.
    const current = qualify({
      prefix, packageBinDir, env: childEnv, timeoutMs,
    });
    if (!current.ok) {
      for (const line of failureMessage(current.reason, {
        prefix, packageBinDir, env: childEnv, timeoutMs,
      })) write(line);
      return { action: 'failed', reason: current.reason };
    }
    return { action: 'already', officialPath: current.path };
  }

  // The marker is taken before anything else and it is fail-closed: without it
  // there is no way to know that an install is not already running.
  try {
    mkdirSync(path.dirname(statePath), { recursive: true });
    writeFileSync(statePath, JSON.stringify({
      state: 'in-progress', pid: process.pid, at: new Date().toISOString(),
    }), { flag: 'wx' });
  } catch (error) {
    if (error && error.code === 'EEXIST') return { action: 'skipped', reason: 'concurrent' };
    for (const line of markerMessage()) write(line);
    return { action: 'failed', reason: 'marker' };
  }

  const pkg = ['pkg', 'apt']
    .map((name) => path.join(prefix, 'bin', name))
    .find((candidate) => existsSync(candidate));
  if (!pkg) {
    removeState(statePath);
    for (const line of failureMessage('no-pkg', {
      prefix, packageBinDir, env: childEnv, timeoutMs,
    })) write(line);
    return { action: 'failed', reason: 'no-pkg' };
  }

  write('Codex for Termux: installing the official Termux package (pkg install codex)...');
  const install = run(pkg, ['install', '-y', OFFICIAL_PACKAGE], childEnv, timeoutMs);
  if (!install.ok) {
    if (install.timedOut || install.signal) {
      // Unknown, not failed: the package manager may still be working. The
      // marker keeps the next start from launching a second installer.
      try {
        writeFileSync(statePath, JSON.stringify({
          state: 'uncertain', pid: process.pid, at: new Date().toISOString(),
        }), 'utf8');
      } catch (_) { /* the message below still tells the truth */ }
      for (const line of uncertainMessage(statePath)) write(line);
      return { action: 'failed', reason: 'install-uncertain' };
    }
    removeState(statePath);
    for (const line of failureMessage('install', {
      prefix, packageBinDir, env: childEnv, timeoutMs,
    })) write(line);
    return { action: 'failed', reason: 'install' };
  }

  const current = qualify({
    prefix, packageBinDir, env: childEnv, timeoutMs,
  });
  if (!current.ok) {
    removeState(statePath);
    for (const line of failureMessage(current.reason, {
      prefix, packageBinDir, env: childEnv, timeoutMs,
    })) write(line);
    return { action: 'failed', reason: current.reason };
  }

  try {
    writeFileSync(statePath, JSON.stringify({
      state: 'done', pid: process.pid, at: new Date().toISOString(),
    }), 'utf8');
  } catch (_) {
    // The switch itself succeeded; only the record of it is missing.
  }
  for (const line of successMessage(statePath)) write(line);
  return { action: 'migrated', officialPath: current.path };
}

// The launcher hands over only to a command that was resolved, qualified and
// is known not to be this file again.
export function useOfficial(result) {
  return Boolean(result && (result.action === 'migrated' || result.action === 'already')
    && result.officialPath);
}

function exitCodeOf(result) {
  if (result.signal) {
    const number = os.constants.signals[result.signal];
    return typeof number === 'number' ? 128 + number : 128;
  }
  if (result.status === null || result.status === undefined) return 1;
  return result.status;
}

// Runs the resolved official command with the caller's arguments and returns
// its exit code, or 128+signal when it was killed by a signal.
export function launchOfficial({
  path: command,
  env = process.env,
  args = [],
  stdio = 'inherit',
  packageBinDir,
} = {}) {
  if (!command) return 1;
  const result = spawnSync(command, args, {
    env: cleanEnv(env, { packageBinDir }),
    stdio,
    shell: false,
  });
  if (result.error) return 1;
  return exitCodeOf(result);
}

// The bundled binary of this package, used when the official command is not
// usable. Its own identity and its own library directory.
export function launchBundled({
  fallbackPath,
  args = [],
  env = process.env,
  stdio = 'inherit',
} = {}) {
  if (!fallbackPath || !existsSync(fallbackPath)) return 1;
  try {
    chmodSync(fallbackPath, 0o755);
  } catch (_) { /* already executable in a normal install */ }
  const result = spawnSync(fallbackPath, args, {
    env: {
      ...env,
      CODEX_SELF_EXE: fallbackPath,
      LD_LIBRARY_PATH: path.dirname(fallbackPath),
    },
    stdio,
    shell: false,
  });
  if (result.error) return 1;
  return exitCodeOf(result);
}
