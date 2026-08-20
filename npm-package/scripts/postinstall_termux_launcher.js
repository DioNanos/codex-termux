import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Termux has no /usr/bin/env. npm creates global bin links to these files,
// but the kernel processes their shebang before Node can run the launcher.
// Rewrite only the installed copy to the interpreters in the active prefix.
if (process.platform !== 'android') {
  process.exit(0);
}

if (!path.isAbsolute(process.execPath)) {
  throw new Error(`Node executable path is not absolute: ${process.execPath}`);
}

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const nodeInterpreter = process.execPath;
const shellInterpreter = path.join(path.dirname(nodeInterpreter), 'sh');
const launcherPaths = [
  ['bin/codex.js', nodeInterpreter],
  ['bin/codex-exec.js', nodeInterpreter],
  ['bin/codex', shellInterpreter],
  ['bin/codex-exec', shellInterpreter],
];

for (const [relativePath, interpreter] of launcherPaths) {
  const launcherPath = path.join(packageRoot, relativePath);
  if (!existsSync(launcherPath)) {
    continue;
  }

  const source = readFileSync(launcherPath, 'utf8');
  const firstLineEnd = source.indexOf('\n');
  const firstLine = firstLineEnd === -1 ? source : source.slice(0, firstLineEnd);
  if (!firstLine.startsWith('#!')) {
    throw new Error(`Launcher has no shebang: ${launcherPath}`);
  }

  const replacement = `#!${interpreter}`;
  if (firstLine !== replacement) {
    writeFileSync(
      launcherPath,
      `${replacement}${source.slice(firstLine.length)}`,
      'utf8',
    );
  }
}
