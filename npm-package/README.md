# Codex Termux

[![npm termux](https://img.shields.io/npm/v/@mmmbuto/codex-cli-termux?style=flat-square&logo=npm)](https://www.npmjs.com/package/@mmmbuto/codex-cli-termux)
[![latest release](https://img.shields.io/github/v/release/DioNanos/codex-termux?style=flat-square)](https://github.com/DioNanos/codex-termux/releases/latest)

> [!IMPORTANT]
> **End of life.** Codex is finally an official Termux package.
> `0.163.0-termux.3` is the final release of `@mmmbuto/codex-cli-termux`:
> from now on, Codex for Termux stays on the official package.

## Thank you

To everyone who installed this package, ran it on real phones and
reported issues: thank you. You kept Codex alive on Android until it
had an official home.

## The switch is automatic

Once you are on the final release, the first interactive launch installs the
official `codex` package, verifies it and starts it. The old npm package stays
installed and inert: it is not removed for you.

Your `~/.codex` (config, login, sessions) is reused as-is. If anything fails, it
prints the manual steps and uses the bundled binary for that run.

## Manual switch

Already on the official package? Remove the old npm one, then put the command
back:

```bash
npm uninstall -g @mmmbuto/codex-cli-termux
pkg install --reinstall codex
hash -r
codex --version
```

The reinstall is not optional: npm can remove the `codex` command together with
the package. The old `codex-exec` command belongs to the npm package and
disappears with it.

Not migrated yet? Install the official package first:

```bash
pkg install codex
```

If `pkg install codex` reports the package as already installed but `codex`
still runs the old launcher, use `pkg install --reinstall codex`.

From then on, update with `pkg upgrade codex`.

## Want the extras?

The official package is plain Codex. For `/loop`, `/goal`, `/vivling`,
`/vl` and `/remote-control`, install [codex-vl](https://github.com/DioNanos/codex-vl):

```bash
npm install -g @mmmbuto/codex-vl
```

It runs as `codex-vl`, side by side with the official `codex`: no PATH
conflict. Termux, Linux and macOS.

## Links

- Official package: [termux-packages/packages/codex](https://github.com/termux/termux-packages/tree/master/packages/codex)
- Extras: [DioNanos/codex-vl](https://github.com/DioNanos/codex-vl)
- Past releases: [GitHub](https://github.com/DioNanos/codex-termux/releases) · [npm](https://www.npmjs.com/package/@mmmbuto/codex-cli-termux)

## License

Apache 2.0, inherited from OpenAI Codex (original work: OpenAI).
See [LICENSE](./LICENSE).
