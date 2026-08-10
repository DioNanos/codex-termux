#!/usr/bin/env python3
"""Teach a rusty_v8 source checkout how to run bindgen for Android.

`build.rs` configures the bindgen invocation for macOS, Linux and iOS. There is
no Android branch, so the V8 headers are parsed against the host's C library and
every declaration `<cstdint>` imports with `using_if_exists` stays unresolved.

The environment cannot carry the fix. Cargo exports `TARGET` for the whole
build, so even the per-target `BINDGEN_EXTRA_CLANG_ARGS_<triple>` is picked up
by the bindgen calls chromium runs for its own x86_64 host tools, which then
reject their own `-msse3` because the Android triple was forced on them.

Two edits are applied:

1. an Android branch that collects the arguments this target needs;
2. a change to the builder call so those arguments are passed **after** the GN
   ones. `build.rs` appends the GN arguments last, and for clang the last
   `--target` or `--sysroot` wins, so anything added in the branch alone can be
   overridden by whatever the GN build happens to pass.

The builder call also prints the final argument list. Generating the bindings is
the last thing a two-hour build does, so a run that still fails has to explain
itself rather than only repeating that it failed.

Every addition is conditional on environment variables, so a patched checkout
still builds unchanged for every other target.
"""

from __future__ import annotations

import sys
from pathlib import Path

# `target_os == "android"` already appears upstream for unrelated reasons, so it
# cannot tell a patched checkout from a pristine one -- and neither can a single
# name this patch introduces, because a checkout that merely mentions it in a
# comment would be declared patched and left untouched, silently producing an
# unpatched build. Idempotence is decided by the postconditions below: all three
# present means applied, none present means apply, and anything in between is a
# half-patched tree that must stop the build rather than guess.

# `let target_os = ...` alone appears five times; pair it with the branch that
# follows it in the bindgen function to name the one place meant here.
DECLARE_ANCHOR = (
    '  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();\n'
    '  if target_os == "macos" {\n'
)
DECLARE = (
    "  // Filled in by the android branch below and applied after the GN\n"
    "  // arguments, which build.rs passes last.\n"
    "  let mut android_last_args: Vec<String> = Vec::new();\n"
)

BRANCH_ANCHOR = '  } else if target_os == "ios" {'
BRANCH = '''  } else if target_os == "android" {
    // One definition, composed by the caller and already exercised by the
    // toolchain check, instead of rebuilding the list here where it could
    // drift from what was verified.
    if let Ok(extra) = env::var("V8_BINDGEN_CLANG_ARGS") {
      android_last_args
        .extend(extra.split_whitespace().map(|arg| arg.to_string()));
    }
'''

BUILDER_ANCHOR = """    .clang_args(clang_args)
    .clang_args(filtered_args)
"""
BUILDER = """    .clang_args({
      let mut all: Vec<String> =
        clang_args.iter().map(|arg| arg.to_string()).collect();
      all.extend(filtered_args.iter().map(|arg| arg.to_string()));
      all.extend(android_last_args.iter().cloned());
      eprintln!("bindgen clang args: {all:?}");
      all
    })
"""


def apply(source: str, anchor: str, replacement: str, what: str, path: Path) -> str:
    """Replace the single occurrence of `anchor`, refusing to guess otherwise."""
    found = source.count(anchor)
    if found != 1:
        raise SystemExit(
            f"expected exactly one anchor for {what} in {path}, found {found}: "
            "the build script changed shape and this patch must be revisited "
            "rather than applied blindly"
        )
    return source.replace(anchor, replacement, 1)


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: patch_rusty_v8_android_bindgen.py <path to build.rs>")

    path = Path(sys.argv[1])
    source = path.read_text(encoding="utf-8")

    postconditions = (
        ("the argument list", DECLARE),
        ("the android branch", BRANCH),
        ("the builder call", BUILDER),
    )
    present = [what for what, text in postconditions if text in source]
    if len(present) == len(postconditions):
        print(f"{path} already has the android bindgen branch; leaving it alone")
        return 0
    if present:
        missing = [what for what, text in postconditions if text not in source]
        raise SystemExit(
            f"{path} is half-patched: it already has {', '.join(present)} but not "
            f"{', '.join(missing)}. Applying the rest would stack edits on an "
            "unknown state; restore the checkout and run this once."
        )

    source = apply(
        source, DECLARE_ANCHOR, DECLARE + DECLARE_ANCHOR, "the argument list", path
    )
    source = apply(
        source, BRANCH_ANCHOR, BRANCH + BRANCH_ANCHOR, "the android branch", path
    )
    source = apply(source, BUILDER_ANCHOR, BUILDER, "the builder call", path)

    path.write_text(source, encoding="utf-8")
    print(f"added the android bindgen branch to {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
