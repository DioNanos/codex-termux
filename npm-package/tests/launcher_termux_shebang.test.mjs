import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const read = (relativePath) =>
  readFileSync(path.join(packageRoot, relativePath), 'utf8');

test('Termux postinstall rewrites both npm entrypoint interpreters', () => {
  const postinstall = read('scripts/postinstall_termux_launcher.js');
  assert.match(postinstall, /Termux has no \/usr\/bin\/env/);
  assert.match(postinstall, /process\.platform !== 'android'/);
  assert.match(postinstall, /bin\/codex\.js/);
  assert.match(postinstall, /bin\/codex-exec\.js/);
  assert.match(postinstall, /bin\/codex', shellInterpreter/);
  assert.match(postinstall, /bin\/codex-exec', shellInterpreter/);
});

for (const launcher of ['bin/codex.js', 'bin/codex-exec.js']) {
  test(`${launcher} invokes codex.bin directly`, () => {
    const source = read(launcher);
    assert.match(source, /const binaryPath = join\(__dirname, 'codex\.bin'\)/);
    assert.match(source, /spawn(?:Sync)?\(binaryPath/);
    assert.match(source, /CODEX_MANAGED_BY_NPM/);
    assert.match(source, /CODEX_SELF_EXE/);
    assert.match(source, /sanitizeLdLibraryPath/);
  });
}
