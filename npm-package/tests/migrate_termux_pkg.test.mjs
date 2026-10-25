import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

import {
  MIGRATION_PACKAGE, automatedReason, cleanEnv, launchBundled, launchOfficial,
  officialTarget, runMigration, useOfficial,
} from '../bin/migrate-to-termux-pkg.js';

const STATE = '.termux-migration';
const statePathOf = (home) => path.join(home, '.codex', STATE);

function writeExecutable(file, body) {
  writeFileSync(file, body, 'utf8');
  chmodSync(file, 0o755);
}

function officialSh(mark) {
  return `#!/bin/sh
${mark}
if [ "$1" = "--version" ]; then
  if [ -n "$FAKE_VERSION_FAIL" ]; then echo "boom" >&2; exit 3; fi
  echo "codex-cli 0.164.0"
  exit 0
fi
echo "official $*" >> "$FAKE_STATE"
env > "$FAKE_ENV_OUT"
if [ -n "$FAKE_OFFICIAL_SIGNAL" ]; then kill -"$FAKE_OFFICIAL_SIGNAL" $$; fi
exit \${FAKE_OFFICIAL_EXIT:-0}
`;
}

function createFixture({
  installFails = false,
  installHangs = false,
  brokenOfficial = false,
  pkgNoop = false,
  dpkgClaimsCodex = false,
  dpkgStatus = null,
  markerRaw = null,
} = {}) {
  const root = mkdtempSync(path.join(tmpdir(), 'termux-migrate-'));
  const prefix = path.join(root, 'usr');
  const prefixBin = path.join(prefix, 'bin');
  const home = path.join(root, 'home');
  const packageDir = path.join(root, 'npm-package');
  const state = path.join(root, 'calls.log');
  const mark = '# official-termux-codex';

  for (const dir of [
    prefixBin, home, packageDir,
    path.join(packageDir, 'bin'),
    path.join(prefix, 'lib', 'node_modules', 'npm', 'bin'),
  ]) mkdirSync(dir, { recursive: true });
  writeFileSync(state, '', 'utf8');
  writeExecutable(path.join(packageDir, 'bin', 'codex.bin'), '#!/bin/sh\nexit 0\n');
  writeFileSync(path.join(packageDir, 'bin', 'codex.js'), '// npm launcher\n', 'utf8');

  writeExecutable(
    path.join(prefixBin, 'pkg'),
    `#!/bin/sh
echo "pkg $*" >> "$FAKE_STATE"
if [ -n "$FAKE_INSTALL_HANGS" ]; then sleep 0.6; exit 0; fi
if [ -n "$FAKE_INSTALL_FAILS" ]; then echo "E: failed" >&2; exit 100; fi
if [ -n "$FAKE_PKG_NOOP" ]; then exit 0; fi
if [ -n "$FAKE_BROKEN_OFFICIAL" ]; then
  printf '#!/bin/sh\\nexit 7\\n' > "${prefixBin}/codex"
  chmod +x "${prefixBin}/codex"
  exit 0
fi
cat > "${prefixBin}/codex" <<'OFFICIAL'
${officialSh(mark)}
OFFICIAL
chmod +x "${prefixBin}/codex"
exit 0
`,
  );

  writeExecutable(
    path.join(prefixBin, 'dpkg'),
    `#!/bin/sh
echo "dpkg $*" >> "$FAKE_STATE"
if [ "$1" = "-W" ]; then
  case "$FAKE_DPKG_STATUS" in
    "") exit 1 ;;
    unreadable) echo "garbage"; exit 0 ;;
    *) echo "$FAKE_DPKG_STATUS"; exit 0 ;;
  esac
fi
if [ "$1" = "-s" ]; then
  case "$FAKE_DPKG_STATUS" in
    "") exit 1 ;;
    unreadable) echo "Package: codex"; exit 0 ;;
    *) printf 'Package: codex\nStatus: %s\n' "$FAKE_DPKG_STATUS"; exit 0 ;;
  esac
fi
if [ "$1" = "-S" ]; then
  if [ -n "$FAKE_DPKG_CLAIMS_CODEX" ]; then echo "codex: $2"; exit 0; fi
  if [ -f "$2" ] && grep -q "official-termux-codex" "$2"; then echo "codex: $2"; exit 0; fi
  echo "dpkg-query: no path found matching pattern $2" >&2; exit 1
fi
exit 0
`,
  );

  writeExecutable(
    path.join(prefix, 'lib', 'node_modules', 'npm', 'bin', 'npm-cli.js'),
    `import { appendFileSync } from 'node:fs';
appendFileSync(process.env.FAKE_STATE, 'npm ' + process.argv.slice(2).join(' ') + '\\n');
process.exit(0);
`,
  );

  mkdirSync(path.join(home, '.codex'), { recursive: true });
  if (markerRaw !== null) writeFileSync(statePathOf(home), `${markerRaw}\n`, 'utf8');

  const env = {
    PATH: `${prefixBin}:/usr/bin:/bin`,
    TMPDIR: root,
    PREFIX: prefix,
    HOME: home,
    FAKE_STATE: state,
    FAKE_ENV_OUT: path.join(root, 'official-env.txt'),
    FAKE_INSTALL_FAILS: installFails ? '1' : '',
    FAKE_INSTALL_HANGS: installHangs ? '1' : '',
    FAKE_BROKEN_OFFICIAL: brokenOfficial ? '1' : '',
    FAKE_PKG_NOOP: pkgNoop ? '1' : '',
    FAKE_DPKG_CLAIMS_CODEX: dpkgClaimsCodex ? '1' : '',
    FAKE_DPKG_STATUS: dpkgStatus || '',
    LD_LIBRARY_PATH: path.join(packageDir, 'bin'),
    CODEX_MANAGED_BY_NPM: '1',
    CODEX_SELF_EXE: path.join(packageDir, 'bin', 'codex.bin'),
  };

  return {
    root, prefix, prefixBin, home, packageDir, state, env,
    calls: () => readFileSync(state, 'utf8').trim().split('\n').filter(Boolean),
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  };
}

function migrate(fx, overrides = {}) {
  const stderr = [];
  const result = runMigration({
    env: { ...fx.env, ...(overrides.env || {}) },
    prefix: fx.prefix,
    home: fx.home,
    interactive: overrides.interactive ?? true,
    args: overrides.args || [],
    now: () => new Date('2026-10-25T12:00:00Z'),
    stderr: (line) => stderr.push(line),
    packageBinDir: path.join(fx.packageDir, 'bin'),
    ...(overrides.extra || {}),
  });
  return { result, stderr };
}

test('a successful migration installs, qualifies the real command and hands over', () => {
  const fx = createFixture();
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'migrated');
    assert.ok(fx.calls().some((c) => c === 'pkg install -y codex'));
    assert.equal(JSON.parse(readFileSync(statePathOf(fx.home), 'utf8')).state, 'done');
    assert.match(stderr.join('\n'), /still installed but inert/);
    assert.match(stderr.join('\n'), /codex-exec/);
    assert.ok(!fx.calls().some((c) => c.startsWith('npm ')), 'npm must never run');
  } finally {
    fx.cleanup();
  }
});

test('a partial installation keeps the bundled binary and reports the real state', () => {
  const fx = createFixture({ brokenOfficial: true });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'official-not-dpkg');
    assert.equal(existsSync(statePathOf(fx.home)), false, 'a clean failure must allow a retry');
    assert.match(stderr.join('\n'), /State of `codex`/);
    assert.ok(!fx.calls().some((c) => c.startsWith('npm ')));
  } finally {
    fx.cleanup();
  }
});

test('a timeout leaves an uncertain marker and blocks the next attempt', () => {
  const fx = createFixture({ installHangs: true });
  try {
    const first = migrate(fx, { extra: { commandTimeoutMs: 200 } });
    assert.equal(first.result.action, 'failed');
    assert.equal(first.result.reason, 'install-uncertain');
    const second = migrate(fx, { extra: { commandTimeoutMs: 200 } });
    assert.equal(second.result.action, 'failed');
    assert.equal(second.result.reason, 'recovery-required');
    const installs = fx.calls().filter((c) => c.startsWith('pkg install')).length;
    assert.equal(installs, 1, 'a second installer was started');
  } finally {
    fx.cleanup();
  }
});

test('an unwritable marker stops the migration before any installation', () => {
  const fx = createFixture();
  try {
    const codexHome = path.join(fx.home, '.codex');
    chmodSync(codexHome, 0o500);
    try {
      const { result } = migrate(fx);
      assert.equal(result.action, 'failed');
      assert.equal(result.reason, 'marker');
      assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')), 'pkg ran without a marker');
    } finally {
      chmodSync(codexHome, 0o700);
    }
  } finally {
    fx.cleanup();
  }
});

test('a concurrent run does not start a second installation', () => {
  const fx = createFixture({ markerRaw: JSON.stringify({ state: 'in-progress', pid: process.pid, at: new Date().toISOString() }) });
  try {
    const { result } = migrate(fx);
    assert.equal(result.action, 'skipped');
    assert.equal(result.reason, 'concurrent');
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('codex exec never migrates and writes nothing', () => {
  const fx = createFixture();
  try {
    const { result } = migrate(fx, { args: ['exec', 'hello'] });
    assert.equal(result.action, 'skipped');
    assert.equal(result.reason, 'no-args');
    assert.equal(fx.calls().length, 0);
    assert.equal(existsSync(statePathOf(fx.home)), false);
  } finally {
    fx.cleanup();
  }
});

test('a managed cell never migrates and writes nothing', () => {
  const fx = createFixture();
  try {
    const { result } = migrate(fx, { env: { NEXUSCREW_MCP_SESSION: 'synthetic-cell' } });
    assert.equal(result.action, 'skipped');
    assert.equal(result.reason, 'nexuscrew');
    assert.equal(fx.calls().length, 0);
    assert.equal(existsSync(statePathOf(fx.home)), false);
  } finally {
    fx.cleanup();
  }
});

test('tmux alone is not a cell', () => {
  assert.equal(automatedReason([], { TMUX: '/tmp/tmux,1,0' }), '');
  assert.equal(automatedReason([], { NEXUSCREW_MCP_SESSION: 'x' }), 'nexuscrew');
  assert.equal(automatedReason(['exec'], {}), 'no-args');
});

test('a link put back by npm is not followed into the old launcher', () => {
  const fx = createFixture();
  try {
    symlinkSync(
      path.join(fx.packageDir, 'bin', 'codex.js'),
      path.join(fx.prefixBin, 'codex'),
    );
    const target = officialTarget({
      prefix: fx.prefix,
      packageBinDir: path.join(fx.packageDir, 'bin'),
    });
    assert.equal(target.ok, false);
    assert.equal(target.reason, 'points-to-ours');
  } finally {
    fx.cleanup();
  }
});

test('an already migrated install is recognised from the marker', () => {
  const fx = createFixture({ markerRaw: JSON.stringify({ state: 'done', pid: 1, at: new Date().toISOString() }) });
  writeExecutable(path.join(fx.prefixBin, 'codex'), officialSh('# official-termux-codex'));
  try {
    const { result } = migrate(fx);
    assert.equal(result.action, 'already');
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')), 'pkg ran again');
  } finally {
    fx.cleanup();
  }
});

test('the official binary propagates a signal as 128+N', () => {
  const fx = createFixture();
  try {
    writeExecutable(path.join(fx.prefixBin, 'codex'), officialSh('# official-termux-codex'));
    const code = launchOfficial({
      path: path.join(fx.prefixBin, 'codex'),
      env: { ...fx.env, FAKE_OFFICIAL_SIGNAL: 'ABRT' },
      args: ['whatever'],
      stdio: 'ignore',
    });
    assert.equal(code, 134);
  } finally {
    fx.cleanup();
  }
});

test('the official binary receives the caller arguments and a clean environment', () => {
  const fx = createFixture();
  try {
    writeExecutable(path.join(fx.prefixBin, 'codex'), officialSh('# official-termux-codex'));
    const code = launchOfficial({
      path: path.join(fx.prefixBin, 'codex'),
      env: { ...fx.env, FAKE_OFFICIAL_EXIT: '5' },
      args: ['exec', 'hello world'],
      stdio: 'ignore',
    });
    assert.equal(code, 5);
    assert.ok(fx.calls().includes('official exec hello world'));
    const seen = readFileSync(fx.env.FAKE_ENV_OUT, 'utf8');
    assert.doesNotMatch(seen, /^CODEX_MANAGED_BY_/m);
    assert.doesNotMatch(seen, /^CODEX_SELF_EXE=/m);
  } finally {
    fx.cleanup();
  }
});

test('cleanEnv drops the managed hints and keeps the user library path', () => {
  const out = cleanEnv(
    {
      PATH: '/bin',
      LD_LIBRARY_PATH: '/pkg/bin:/vendor/lib',
      CODEX_MANAGED_BY_NPM: '1',
      CODEX_SELF_EXE: '/pkg/bin/codex.bin',
      HOME: '/home/x',
    },
    { packageBinDir: '/pkg/bin' },
  );
  assert.equal(out.LD_LIBRARY_PATH, '/vendor/lib');
  assert.equal(out.CODEX_MANAGED_BY_NPM, undefined);
  assert.equal(out.CODEX_SELF_EXE, undefined);
  assert.equal(out.HOME, '/home/x');
});

test('the bundled binary runs when the official is unusable', () => {
  const fx = createFixture();
  try {
    const code = launchBundled({
      fallbackPath: path.join(fx.packageDir, 'bin', 'codex.bin'),
      args: [],
      env: fx.env,
      stdio: 'ignore',
    });
    assert.equal(code, 0);
  } finally {
    fx.cleanup();
  }
});

test('useOfficial accepts only a verified handover', () => {
  assert.equal(useOfficial({ action: 'migrated', officialPath: '/usr/bin/codex' }), true);
  assert.equal(useOfficial({ action: 'already', officialPath: '/usr/bin/codex' }), true);
  assert.equal(useOfficial({ action: 'migrated' }), false);
  assert.equal(useOfficial({ action: 'failed', reason: 'verify-version' }), false);
  assert.equal(useOfficial({ action: 'skipped', reason: 'no-tty' }), false);
});

test('no branch ever runs npm', () => {
  const scenarios = [
    {},
    { installFails: true },
    { brokenOfficial: true },
    { installHangs: true },
    { markerRaw: JSON.stringify({ state: 'in-progress', pid: process.pid, at: new Date().toISOString() }) },
    { markerRaw: JSON.stringify({ state: 'done', pid: 1, at: new Date().toISOString() }) },
  ];
  for (const scenario of scenarios) {
    const fx = createFixture(scenario);
    try {
      migrate(fx, { extra: { commandTimeoutMs: 200 } });
      assert.ok(
        !fx.calls().some((c) => c.startsWith('npm ')),
        `npm ran in ${JSON.stringify(scenario)}: ${JSON.stringify(fx.calls())}`,
      );
    } finally {
      fx.cleanup();
    }
  }
});

test('the migration writes only the marker under home', () => {
  const fx = createFixture();
  try {
    migrate(fx);
    const top = execFileSync('ls', ['-A', fx.home], { encoding: 'utf8' }).trim().split('\n');
    assert.deepEqual(top, ['.codex']);
    const inside = execFileSync('ls', ['-A', path.join(fx.home, '.codex')], { encoding: 'utf8' })
      .trim().split('\n');
    assert.deepEqual(inside, [STATE]);
  } finally {
    fx.cleanup();
  }
});

// ── Ogni passaggio riverifica il comando, lo stato e la riparazione ────

function seedOfficial(fx, kind) {
  const file = path.join(fx.prefixBin, 'codex');
  const bodies = {
    healthy: officialSh('# official-termux-codex'),
    'not-executable': officialSh('# official-termux-codex'),
    'version-fails': '#!/bin/sh\n# official-termux-codex\nif [ "$1" = "--version" ]; then echo boom >&2; exit 3; fi\nexit 0\n',
    'no-dpkg': '#!/bin/sh\nexit 0\n',
  };
  if (kind === 'ours-link') {
    symlinkSync(path.join(fx.packageDir, 'bin', 'codex.js'), file);
    return file;
  }
  writeExecutable(file, bodies[kind]);
  if (kind === 'not-executable') chmodSync(file, 0o644);
  return file;
}

const DONE = JSON.stringify({ state: 'done', pid: 1, at: '2026-10-25T12:00:00Z' });

for (const [kind, reason] of [
  ['not-executable', 'official-not-executable'],
  ['version-fails', 'official-version'],
  ['no-dpkg', 'official-not-dpkg'],
  ['ours-link', 'official-points-to-ours'],
]) {
  test(`a done marker with an ${kind} command falls back to the bundled binary`, () => {
    const fx = createFixture({ markerRaw: DONE, dpkgClaimsCodex: kind === 'version-fails' });
    try {
      seedOfficial(fx, kind);
      const { result, stderr } = migrate(fx);
      assert.equal(result.action, 'failed');
      assert.equal(result.reason, reason);
      assert.equal(useOfficial(result), false);
      assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')), 'pkg ran in a done branch');
      assert.match(stderr.join('\n'), /State of `codex`/);
    } finally {
      fx.cleanup();
    }
  });
}

test('a done marker with a healthy official command is handed over', () => {
  const fx = createFixture({ markerRaw: DONE });
  try {
    seedOfficial(fx, 'healthy');
    const { result } = migrate(fx);
    assert.equal(result.action, 'already');
    assert.equal(useOfficial(result), true);
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('a live in-progress marker is a concurrent run', () => {
  const fx = createFixture({
    markerRaw: JSON.stringify({ state: 'in-progress', pid: process.pid, at: '2026-10-25T12:00:00Z' }),
  });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'skipped');
    assert.equal(result.reason, 'concurrent');
    assert.equal(stderr.length, 0);
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('an abandoned in-progress marker asks for manual recovery', () => {
  const fx = createFixture({
    markerRaw: JSON.stringify({ state: 'in-progress', pid: 999999, at: '2000-01-01T00:00:00Z' }),
  });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'recovery-required');
    assert.match(stderr.join('\n'), /did not finish/);
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('an unreadable marker asks for manual recovery', () => {
  const fx = createFixture({ markerRaw: '{not json' });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'recovery-required');
    assert.match(stderr.join('\n'), /did not finish/);
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('any argument keeps the migration away', () => {
  for (const args of [['--profile', 'x', 'exec', 'p'], ['e', 'p'], ['--help'], ['resume'], ['-c', 'x=y']]) {
    const fx = createFixture();
    try {
      const { result } = migrate(fx, { args });
      assert.equal(result.action, 'skipped', JSON.stringify(args));
      assert.equal(result.reason, 'no-args', JSON.stringify(args));
      assert.equal(fx.calls().length, 0, `commands for ${JSON.stringify(args)}`);
    } finally {
      fx.cleanup();
    }
  }
});

test('a link restored by npm asks for a reinstall, not a plain install', () => {
  const fx = createFixture({ pkgNoop: true, dpkgClaimsCodex: true });
  try {
    seedOfficial(fx, 'ours-link');
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'official-points-to-ours');
    const text = stderr.join('\n');
    assert.match(text, /pkg install --reinstall codex/);
    assert.doesNotMatch(text, /To switch by hand: pkg install codex/);
  } finally {
    fx.cleanup();
  }
});

test('a first install without the package asks for a plain install', () => {
  const fx = createFixture();
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'migrated');
    const second = createFixture({ installFails: true });
    try {
      const failed = migrate(second);
      assert.equal(failed.result.action, 'failed');
      assert.match(failed.stderr.join('\n'), /pkg install codex/);
      assert.doesNotMatch(failed.stderr.join('\n'), /--reinstall/);
    } finally {
      second.cleanup();
    }
  } finally {
    fx.cleanup();
  }
});

// ── Stato riconoscibile e pacchetto registrato ─────────────────────────

test('an unknown marker state asks for manual recovery', () => {
  const fx = createFixture({ markerRaw: JSON.stringify({ state: 'bogus', pid: process.pid }) });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'recovery-required');
    assert.match(stderr.join('\n'), /did not finish/);
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('a marker without a state asks for manual recovery', () => {
  const fx = createFixture({ markerRaw: JSON.stringify({ pid: process.pid }) });
  try {
    const { result } = migrate(fx);
    assert.equal(result.action, 'failed');
    assert.equal(result.reason, 'recovery-required');
    assert.ok(!fx.calls().some((c) => c.startsWith('pkg ')));
  } finally {
    fx.cleanup();
  }
});

test('a missing command with the package installed asks for a reinstall', () => {
  const fx = createFixture({ dpkgStatus: 'install ok installed', installFails: true });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    const text = stderr.join('\n');
    assert.match(text, /pkg install --reinstall codex/);
    assert.match(text, /missing, but the package is installed|not available/);
  } finally {
    fx.cleanup();
  }
});

test('a missing command without the package asks for a plain install', () => {
  const fx = createFixture({ installFails: true });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    const text = stderr.join('\n');
    assert.match(text, /To repair it by hand: pkg install codex/);
    assert.doesNotMatch(text, /--reinstall/);
  } finally {
    fx.cleanup();
  }
});

// ── Lo stato di dpkg si legge dal campo Status ────────────────────────

test('a package left in config-files is not installed', () => {
  const fx = createFixture({ dpkgStatus: 'deinstall ok config-files', installFails: true });
  try {
    const { result, stderr } = migrate(fx);
    assert.equal(result.action, 'failed');
    const text = stderr.join('\n');
    assert.match(text, /To repair it by hand: pkg install codex/);
    assert.doesNotMatch(text, /--reinstall/);
  } finally {
    fx.cleanup();
  }
});

test('a package that is not installed asks for a plain install', () => {
  const fx = createFixture({ installFails: true });
  try {
    const { stderr } = migrate(fx);
    const text = stderr.join('\n');
    assert.match(text, /To repair it by hand: pkg install codex/);
    assert.doesNotMatch(text, /--reinstall/);
  } finally {
    fx.cleanup();
  }
});

test('an unreadable dpkg answer is not an installed package', () => {
  const fx = createFixture({ dpkgStatus: 'unreadable', installFails: true });
  try {
    const { stderr } = migrate(fx);
    const text = stderr.join('\n');
    assert.doesNotMatch(text, /--reinstall/);
  } finally {
    fx.cleanup();
  }
});

test('an installed package asks for a reinstall', () => {
  const fx = createFixture({ dpkgStatus: 'install ok installed', installFails: true });
  try {
    const { stderr } = migrate(fx);
    assert.match(stderr.join('\n'), /To repair it by hand: pkg install --reinstall codex/);
  } finally {
    fx.cleanup();
  }
});
