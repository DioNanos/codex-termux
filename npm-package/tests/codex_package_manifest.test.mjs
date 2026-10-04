import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

// The daemon's L1 check (app-server-daemon/src/managed_install.rs
// ensure_selected_variant_is_fork) reads codex-package.json beside the npm
// package root and refuses to start unless it carries variant codex-termux.
// A tarball without that file fails every remote-control start on Android,
// so the manifest must ship in the package and stay pinned to the layout.
test('packaged manifest carries the codex-termux variant and layout', () => {
  const manifest = JSON.parse(
    readFileSync(path.join(packageRoot, 'codex-package.json'), 'utf8'),
  );
  assert.equal(manifest.layoutVersion, 1);
  assert.equal(manifest.variant, 'codex-termux');
  assert.equal(manifest.target, 'aarch64-linux-android');
  assert.equal(manifest.entrypoint, 'bin/codex.bin');
  assert.equal(Object.hasOwn(manifest, 'version'), false);
});

test('package.json ships codex-package.json in files', () => {
  const pkg = JSON.parse(
    readFileSync(path.join(packageRoot, 'package.json'), 'utf8'),
  );
  assert.ok(
    pkg.files.includes('codex-package.json'),
    'npm pack omits files that are not listed: codex-package.json must stay in files',
  );
});
