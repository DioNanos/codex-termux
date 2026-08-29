# codex-app-server-daemon

> `codex-app-server-daemon` is experimental and its lifecycle contract may
> change while the remote-management flow is still being developed.

`codex-app-server-daemon` backs the machine-readable `codex app-server`
lifecycle commands used by remote clients such as the desktop and mobile apps.
It is intended for Codex instances launched over SSH, including fresh developer
machines that should expose app-server with `remote_control` enabled.

## Platform support

The current daemon implementation is Unix-only. It uses pidfile-backed
daemonization plus Unix process and file-locking primitives, and does not yet
support Windows lifecycle management.

## Commands

```sh
codex app-server daemon start
codex app-server daemon restart
codex app-server daemon enable-remote-control
codex app-server daemon disable-remote-control
codex app-server daemon stop
codex app-server daemon version
codex app-server daemon bootstrap --remote-control
```

On success, every command writes exactly one JSON object to stdout. Consumers
should parse that JSON rather than relying on human-readable text. Lifecycle
responses report the resolved backend, socket path, local CLI version, and
running app-server version when applicable.

## Bootstrap flow

For a new remote machine:

```sh
npm install -g @mmmbuto/codex-cli-termux@latest
codex app-server daemon bootstrap --remote-control
```

On Android, `bootstrap` uses the native `codex.bin` path supplied by the package
launcher through `CODEX_SELF_EXE` as the managed executable. It records the daemon settings under
`CODEX_HOME/app-server-daemon/`, starts app-server as a pidfile-backed detached
process, and keeps automatic updater fetches disabled for this Termux fork.

## Installation and update cases

The daemon assumes Codex Termux is installed through the fork npm package. On
Android it launches the native package path supplied by `CODEX_SELF_EXE`; other Unix
targets retain the upstream standalone path under `CODEX_HOME`.

| Situation | What starts | Does this daemon fetch new binaries? | Does a running app-server eventually move to a newer binary on its own? |
| --- | --- | --- | --- |
| The fork npm package has run, but only `start` is used | On Android, `start` uses the native `codex.bin` path from `CODEX_SELF_EXE` | No | No. The managed path is used when starting or restarting, but no updater is installed. |
| The fork npm package has run, then `bootstrap` is used | The pidfile backend uses the native managed path supplied by the launcher | No. Bootstrap stops any stale updater loop and leaves `autoUpdateEnabled` false. | No. Update with `npm install -g @mmmbuto/codex-cli-termux@latest`, then restart the daemon. |
| Some other tool updates the managed binary path | The next fresh start or restart uses the updated file at that path | No | No. Restart app-server after updating the managed path. |

### Termux npm installs

For installs created by the fork npm package:

- lifecycle commands use the native package binary supplied through
  `CODEX_SELF_EXE`
- `bootstrap` is supported
- `bootstrap` does not fetch installers or spawn an updater loop
- updates are explicit through `@mmmbuto/codex-cli-termux@latest`

### Out-of-band updates

This daemon does not watch arbitrary executable files for replacement. If some
other tool updates the managed binary path:

- without `bootstrap`, a currently running app-server remains on the old
  executable image until an explicit `restart`
- with `bootstrap`, a currently running app-server still remains on the old
  executable image until an explicit `restart`

## Lifecycle semantics

`start` is idempotent and returns after app-server is ready to answer the normal
JSON-RPC initialize handshake on the Unix control socket.

`restart` stops any managed daemon and starts it again.

`enable-remote-control` and `disable-remote-control` persist the launch setting
for future starts. If a managed app-server is already running, they restart it
so the new setting takes effect immediately.

Top-level `codex remote-control` bootstraps with `--remote-control` when the
managed daemon is not running. Otherwise it enables remote control and starts
the daemon normally.

`stop` sends a graceful termination request first, then sends a second
termination signal after the grace window if the process is still alive.

All mutating lifecycle commands are serialized per `CODEX_HOME`, so a concurrent
`start`, `restart`, `enable-remote-control`, `disable-remote-control`, `stop`,
or `bootstrap` does not race another in-flight lifecycle operation.

## State

The daemon stores its local state under `CODEX_HOME/app-server-daemon/`:

- `settings.json` for persisted launch settings
- `app-server.pid` for the app-server process record
- `app-server-updater.pid` for stopping stale updater loops from older builds
- `daemon.lock` for daemon-wide lifecycle serialization
