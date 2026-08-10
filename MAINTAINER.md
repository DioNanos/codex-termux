# Maintainer

Codex Termux is maintained by **Davide A. Guglielmi** (GitHub:
[DioNanos](https://github.com/DioNanos)) as the porting / distribution
maintainer for Android ARM64 (Termux).

This is **not** an independent fork — Codex Termux tracks
[OpenAI Codex](https://github.com/openai/codex) closely and carries only the
Android/Termux compatibility delta needed to package and run it on Termux.

## Scope of maintenance

In scope:

- the Android ARM64 / Termux compatibility patches (browser auth via
  `termux-open-url`, packaged wrappers, `LD_LIBRARY_PATH` sanitization,
  `RUNPATH=$ORIGIN`, PTY/lock-handling fixes, and the Termux no-audio policy
  after upstream removed the realtime voice surface)
- the `@mmmbuto/codex-cli-termux` npm package and the matching GitHub
  release assets
- the release flow: validate the full tree, build and audit a sanitized GitHub
  Actions candidate, publish the unchanged npm artifact, promote the tested
  public commit to clean `main`, and cut GitHub Releases from `main`

Out of scope here:

- changes that belong upstream — please file those on
  [openai/codex](https://github.com/openai/codex) directly
- broad product features unrelated to Termux compatibility

If a feature is generic and not Termux-specific, the right place is upstream.

## Reporting

| Channel                                  | Where                                                             |
| ---------------------------------------- | ----------------------------------------------------------------- |
| Termux/Android bug reports, PRs          | [DioNanos/codex-termux](https://github.com/DioNanos/codex-termux) |
| Generic Codex bugs (not Termux-specific) | [openai/codex](https://github.com/openai/codex)                   |
| Security disclosures (Termux fork)       | [`SECURITY.md`](./SECURITY.md) — `security@mmmbuto.com`           |
| General contact                          | `dev@mmmbuto.com`                                                 |

When reporting a Termux bug, please include: device, Android version, Termux
build (Classic or F-Droid), Node.js version, and `codex --version`.

## Identity

- Profile: [github.com/DioNanos](https://github.com/DioNanos)
- Project hub: [mmmbuto.com](https://mmmbuto.com)
- Maintainer page and dev journal: [dev.mmmbuto.com](https://dev.mmmbuto.com)

## License

Codex Termux is distributed under the Apache License 2.0 inherited from
[OpenAI Codex](https://github.com/openai/codex). The Termux compatibility
patches are released under the same license. Original work: OpenAI. Termux
port: minimal Android compatibility patches.
See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE).

---

_Per aspera ad astra._
