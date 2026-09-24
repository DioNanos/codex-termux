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

Shared clients use the environment inherited when the daemon started. Opening a
new terminal or clearing variables there does not clear the running daemon's
environment; per-client environment isolation is not provided.
An invocation that sets `CODEX_EXEC_SERVER_URL` skips implicit daemon attachment
so its executor selection is preserved. If an implicitly discovered daemon cannot
initialize the connection, the TUI starts an embedded server instead. Explicit
`--remote` endpoints remain authoritative and report connection failures.

## Commands

```sh
codex app-server daemon start
codex app-server daemon restart
codex app-server daemon update
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

Eligible managed daemons check for updates after five minutes, then hourly by
default. Edit `CODEX_HOME/app-server-daemon/settings.json` to change this:

```json
{"remoteControlEnabled": false,
 "shutdownGraceSeconds": 60,
 "updater": {"autoUpdateEnabled": false, "updateIntervalMinutes": 120}}
```

Positive minute intervals have no configured cap. `daemon restart` applies the
enabled state; the next updater wait reads a new interval. The preference does
not affect an explicit `codex update` command or `daemon update`.

`daemon update` selects the latest stable release, even with automatic updates
disabled. It also returns pinned or local managed packages to production update
eligibility, preserving the automatic-update preference. Legacy installations
migrate to the dedicated root once the published installer and release support
migration. JSON reports `updated`, `noUpdate`, or `unsupported`, with installed
and running versions. A running daemon restarts, so active or queued work may be
interrupted; a stopped daemon stays stopped. Installer errors return nonzero.
The updater uses saved network settings; CLI `-c` overrides do not reach it.

For all managed app-server shutdowns, including explicit stop and restart and
updater-triggered restarts, `shutdownGraceSeconds` defaults to 60 and accepts
an integer from 0 through 300. Zero forces shutdown immediately after requesting
a graceful exit; the five-minute maximum bounds the wait even if a turn is still
running.

## Bootstrap flow

For a new remote machine:

```sh
npm install -g @mmmbuto/codex-cli-termux@latest
codex app-server daemon bootstrap --remote-control
```

On Windows, use a non-elevated PowerShell terminal whose host allows breakaway:

```powershell
irm https://chatgpt.com/codex/install.ps1 | iex
$codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HOME '.codex' }
& "$codexHome\packages\standalone\current\bin\codex.exe" app-server daemon bootstrap --remote-control
```

`bootstrap` can use any complete CLI package. If no daemon package is installed,
it copies the invoking package into `CODEX_HOME/packages/app-server-daemon` and
prints an installation message without asking for confirmation. Existing daemon
packages are reused, including legacy installations; a broken selection is not
silently replaced. A bare executable cannot supply a new installation.

It records the daemon settings under `CODEX_HOME/app-server-daemon/`, starts app-server as a
pidfile-backed detached process. It launches a detached updater loop when
automatic updates are enabled, the installer selected the stable `latest`
channel, and the managed binary supports the updater command.

On Android, `bootstrap` uses the native `codex.bin` path supplied by the package
launcher through `CODEX_SELF_EXE` as the managed executable, and keeps automatic
updater fetches disabled for this Termux fork.

## Installation and update cases

New daemons use `CODEX_HOME/packages/app-server-daemon/current/bin/codex`
(`codex.exe` on Windows). The package contains the executable and its helpers.
Daemon-only installer updates leave the user's CLI command and shell setup alone.

Previously launched legacy daemons retain `CODEX_HOME/packages/standalone/current`,
including its flat binary layout when present. Starts and scheduled updates keep
using that location. An explicit production update prepares and validates a
compatible dedicated package before stopping the legacy updater and daemon,
selecting the new package, and restarting only a previously running daemon.
The old CLI package files and selection remain unchanged.

| Situation | What starts | Does this daemon fetch new binaries? | Does a running app-server eventually move to a newer binary on its own? |
| --- | --- | --- | --- |
| The fork npm package has run, but only `start` is used | On Android, `start` uses the native `codex.bin` path from `CODEX_SELF_EXE` | No | No. The managed path is used when starting or restarting, but no updater is installed. |
| The fork npm package has run, then `bootstrap` is used | The pidfile backend uses the native managed path supplied by the launcher | No. Bootstrap stops any stale updater loop and leaves `autoUpdateEnabled` false. | No. Update with `npm install -g @mmmbuto/codex-cli-termux@latest`, then restart the daemon. |
| Some other tool updates the managed binary path | The next fresh start or restart uses the updated file at that path | No | No. Restart app-server after updating the managed path. |

### Managed packages

For dedicated and retained legacy daemon installations:

- lifecycle commands use the selected daemon package, regardless of the invoking
  CLI version; they do not implicitly replace an existing package

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

Top-level `codex remote-control start` enables and persists remote control for
the managed daemon, overriding a saved disabled value. It starts or bootstraps
the daemon as needed. Plain `codex remote-control` runs a separate foreground
server and does not change daemon settings; `codex remote-control stop` stops
the managed daemon without clearing its saved remote-control preference.
`daemon start` and `daemon restart` use that saved preference. `daemon bootstrap`
sets it according to `--remote-control` (disabled when omitted).

`stop` sends a graceful termination request first, then force-terminates the
process after the configured grace window if it is still alive.

All mutating lifecycle commands are serialized per `CODEX_HOME`, so a concurrent
`start`, `restart`, `enable-remote-control`, `disable-remote-control`, `stop`,
or `bootstrap` does not race another in-flight lifecycle operation.

## State

The daemon stores its local state under `CODEX_HOME/app-server-daemon/`:

- `settings.json` for remote-control launch settings and updater preferences
- `app-server.pid` for the app-server process record
- `app-server-updater.pid` for stopping stale updater loops from older builds
- `daemon.lock` for daemon-wide lifecycle serialization
