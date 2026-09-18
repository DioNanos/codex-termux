import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const defaultPackageRoot = () =>
  path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '..',
  );

// Termux has no /usr/bin/env. npm creates global bin links to these files,
// but the kernel processes their shebang before Node can run the launcher.
// Rewrite only the installed copy to the interpreters in the active prefix.
export function runPostinstallTermuxLauncher({
  packageRoot = defaultPackageRoot(),
  env = process.env,
  platform = process.platform,
  execPath = process.execPath,
  warn = console.warn,
} = {}) {
  if (platform !== 'android') {
    return;
  }

  // The rewritten shebangs point at the interpreters beside the running
  // Node executable, which on Termux lives inside $PREFIX/bin. A PREFIX that
  // is missing, empty, not a directory, uninspectable (e.g. a symlink loop),
  // or without a bin/ directory describes an environment we do not
  // understand: leave the launchers untouched (and the install successful)
  // rather than rewrite shebangs against it.
  const prefix = env.PREFIX;
  if (typeof prefix !== 'string' || prefix === '') {
    warn('Warning: Termux PREFIX is not set; launcher shebangs were left unchanged.');
    return;
  }
  let prefixStat;
  try {
    prefixStat = statSync(prefix, { throwIfNoEntry: false });
  } catch (error) {
    warn(`Warning: Termux PREFIX could not be inspected (${error.code ?? 'unknown error'}): ${prefix}; launcher shebangs were left unchanged.`);
    return;
  }
  if (!prefixStat?.isDirectory()) {
    warn(`Warning: Termux PREFIX is not a directory: ${prefix}; launcher shebangs were left unchanged.`);
    return;
  }
  const prefixBin = path.join(prefix, 'bin');
  let prefixBinStat;
  try {
    prefixBinStat = statSync(prefixBin, { throwIfNoEntry: false });
  } catch (error) {
    warn(`Warning: Termux PREFIX bin/ could not be inspected (${error.code ?? 'unknown error'}): ${prefixBin}; launcher shebangs were left unchanged.`);
    return;
  }
  if (!prefixBinStat?.isDirectory()) {
    warn(`Warning: Termux PREFIX has no bin/ directory: ${prefixBin}; launcher shebangs were left unchanged.`);
    return;
  }

  if (!path.isAbsolute(execPath)) {
    throw new Error(`Node executable path is not absolute: ${execPath}`);
  }

  const nodeInterpreter = execPath;
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
}

const invokedAsScript =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;
if (invokedAsScript) {
  runPostinstallTermuxLauncher();
}
