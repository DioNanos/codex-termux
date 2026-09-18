import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

import { runPostinstallTermuxLauncher } from '../scripts/postinstall_termux_launcher.js';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

const launchers = [
  ['bin/codex.js', '#!/usr/bin/env node'],
  ['bin/codex-exec.js', '#!/usr/bin/env node'],
  ['bin/codex', '#!/bin/sh'],
  ['bin/codex-exec', '#!/bin/sh'],
];

function createFixture() {
  const root = mkdtempSync(path.join(tmpdir(), 'dwa-prefix-'));
  mkdirSync(path.join(root, 'package', 'bin'), { recursive: true });
  mkdirSync(path.join(root, 'prefix', 'bin'), { recursive: true });
  for (const [relative] of launchers) {
    writeFileSync(
      path.join(root, 'package', relative),
      '#!/usr/bin/env node\n// launcher body\n',
      'utf8',
    );
  }
  return {
    root,
    packageRoot: path.join(root, 'package'),
    prefix: path.join(root, 'prefix'),
    execPath: path.join(root, 'prefix', 'bin', 'node'),
    read(relative) {
      return readFileSync(path.join(root, 'package', relative), 'utf8');
    },
    before() {
      return new Map(launchers.map(([relative]) => [relative, this.read(relative)]));
    },
  };
}

function assertLaunchersUntouched(fixture, before, warnings, label) {
  for (const [relative] of launchers) {
    assert.equal(
      fixture.read(relative),
      before.get(relative),
      `${label}: ${relative} must be left unchanged`,
    );
  }
  assert.ok(
    warnings.length > 0 && warnings.every((w) => /^Warning: .+PREFIX/.test(w) || /^Warning: .+bin\//.test(w)),
    `${label}: a clear warning must be emitted, got ${JSON.stringify(warnings)}`,
  );
}

test('missing or empty PREFIX leaves launchers untouched with a warning', () => {
  for (const envValue of [undefined, '']) {
    const fixture = createFixture();
    const before = fixture.before();
    const warnings = [];
    try {
      assert.doesNotThrow(() =>
        runPostinstallTermuxLauncher({
          packageRoot: fixture.packageRoot,
          env: { PREFIX: envValue, TERMUX_VERSION: 'test' },
          platform: 'android',
          execPath: fixture.execPath,
          warn: (m) => warnings.push(m),
        }),
      'an unusable PREFIX must not fail the install');
      assertLaunchersUntouched(
        fixture,
        before,
        warnings,
        `PREFIX=${JSON.stringify(envValue)}`,
      );
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  }
});

test('PREFIX that is not a directory leaves launchers untouched with a warning', () => {
  const fixture = createFixture();
  const before = fixture.before();
  const warnings = [];
  const notADirectory = path.join(fixture.root, 'plain-file');
  writeFileSync(notADirectory, 'not a directory', 'utf8');
  try {
    assert.doesNotThrow(() =>
      runPostinstallTermuxLauncher({
        packageRoot: fixture.packageRoot,
        env: { PREFIX: notADirectory, TERMUX_VERSION: 'test' },
        platform: 'android',
        execPath: fixture.execPath,
        warn: (m) => warnings.push(m),
      }),
    );
    assertLaunchersUntouched(fixture, before, warnings, 'PREFIX=file');
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test('PREFIX without bin/ leaves launchers untouched with a warning', () => {
  const fixture = createFixture();
  const before = fixture.before();
  const warnings = [];
  const prefixWithoutBin = path.join(fixture.root, 'prefix-no-bin');
  mkdirSync(prefixWithoutBin, { recursive: true });
  try {
    assert.doesNotThrow(() =>
      runPostinstallTermuxLauncher({
        packageRoot: fixture.packageRoot,
        env: { PREFIX: prefixWithoutBin, TERMUX_VERSION: 'test' },
        platform: 'android',
        execPath: fixture.execPath,
        warn: (m) => warnings.push(m),
      }),
    );
    assertLaunchersUntouched(fixture, before, warnings, 'PREFIX without bin/');
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test('valid PREFIX rewrites shebangs to the prefix interpreters and is idempotent', () => {
  const fixture = createFixture();
  const warnings = [];
  const run = () =>
    runPostinstallTermuxLauncher({
      packageRoot: fixture.packageRoot,
      env: { PREFIX: fixture.prefix, TERMUX_VERSION: 'test' },
      platform: 'android',
      execPath: fixture.execPath,
      warn: (m) => warnings.push(m),
    });
  try {
    run();
    for (const [relative] of launchers) {
      const expectedInterpreter = relative.endsWith('.js')
        ? fixture.execPath
        : path.join(path.dirname(fixture.execPath), 'sh');
      assert.equal(
        fixture.read(relative).split('\n', 1)[0],
        `#!${expectedInterpreter}`,
        `${relative} must point at the prefix interpreter`,
      );
      assert.equal(
        fixture.read(relative).split('\n').slice(1).join('\n'),
        '// launcher body\n',
        `${relative} body must be preserved`,
      );
    }
    run();
    for (const [relative] of launchers) {
      const expectedInterpreter = relative.endsWith('.js')
        ? fixture.execPath
        : path.join(path.dirname(fixture.execPath), 'sh');
      assert.equal(
        fixture.read(relative).split('\n', 1)[0],
        `#!${expectedInterpreter}`,
        `${relative} rewrite must be idempotent`,
      );
    }
    assert.deepEqual(warnings, [], 'a valid PREFIX must not warn');
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test('postinstall script exits cleanly when run outside android', () => {
  const script = path.join(packageRoot, 'scripts', 'postinstall_termux_launcher.js');
  const stdout = execFileSync(
    process.execPath,
    [script],
    { env: { ...process.env, PREFIX: undefined } },
  );
  assert.equal(stdout.toString(), '', 'nothing to do outside android');
});

test('PREFIX whose bin is a regular file leaves launchers untouched with a warning', () => {
  const fixture = createFixture();
  const before = fixture.before();
  const warnings = [];
  rmSync(path.join(fixture.prefix, 'bin'), { recursive: true });
  writeFileSync(path.join(fixture.prefix, 'bin'), 'regular file', 'utf8');
  try {
    assert.doesNotThrow(() =>
      runPostinstallTermuxLauncher({
        packageRoot: fixture.packageRoot,
        env: { PREFIX: fixture.prefix, TERMUX_VERSION: 'test' },
        platform: 'android',
        execPath: fixture.execPath,
        warn: (m) => warnings.push(m),
      }),
      'a PREFIX whose bin is not a directory must not fail the install',
    );
    assertLaunchersUntouched(fixture, before, warnings, 'PREFIX bin=regular file');
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test('PREFIX that cannot be inspected (symlink loop) leaves launchers untouched with a warning', () => {
  const fixture = createFixture();
  const before = fixture.before();
  const warnings = [];
  const loop = path.join(fixture.root, 'prefix-loop');
  symlinkSync(loop, loop);
  try {
    assert.doesNotThrow(() =>
      runPostinstallTermuxLauncher({
        packageRoot: fixture.packageRoot,
        env: { PREFIX: loop, TERMUX_VERSION: 'test' },
        platform: 'android',
        execPath: fixture.execPath,
        warn: (m) => warnings.push(m),
      }),
      'an uninspectable PREFIX (ELOOP) must not fail the install',
    );
    assertLaunchersUntouched(fixture, before, warnings, 'PREFIX=symlink loop');
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});
