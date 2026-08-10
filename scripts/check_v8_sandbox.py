#!/usr/bin/env python3
"""Decide whether a rusty_v8 archive was built with the V8 sandbox.

`code-mode-runtime` depends on the `v8` crate with `v8_enable_sandbox`. The v8
build script derives the prebuilt's name from the enabled features, but
`RUSTY_V8_ARCHIVE` overrides that name without checking it, so pointing it at
the plain build links and runs with the sandbox absent and emits no signal at
all. This script is the signal.

It reads what `v8__V8__IsSandboxEnabled` returns. That wrapper is compiled from
the same `V8_ENABLE_SANDBOX` condition the feature controls, and it is the
function the Rust side actually calls, so its return value is the invariant
rather than a proxy for it.

Counting `v8::internal::Sandbox` symbols -- which is what this replaced -- only
correlates. Any archive carrying one extra object that defines some
`v8::internal::Sandbox` name passes a non-zero count while the wrapper still
returns false, so the count cannot separate a sandbox build from a plain build
plus a bystander. It is reported here as a secondary observation and never as
the verdict.

No toolchain is required: the ELF is parsed and the wrapper's instructions are
decoded here. That matters because the archive being judged is usually for a
different architecture than the runner, and because a missing disassembler must
not turn into a skipped check. Only functions that return a constant are
decodable; anything else stops the build rather than being guessed at.
"""

from __future__ import annotations

import gzip
import struct
import sys
from pathlib import Path

WRAPPER = "v8__V8__IsSandboxEnabled"
SANDBOX_SYMBOL_FRAGMENT = b"2v88internal7Sandbox"

EM_X86_64 = 0x3E
EM_AARCH64 = 0xB7
MACHINE_NAMES = {EM_X86_64: "x86-64", EM_AARCH64: "aarch64"}


class Undecodable(Exception):
    """The wrapper is not a plain constant-returning function."""


class CannotJudge(Exception):
    """The archive could not be examined at all.

    Kept apart from the "sandbox absent" verdict on purpose. A caller that wants
    the negative -- the producer, when it is deliberately building the plain
    profile -- would otherwise read every failure as a confirmed plain build and
    publish whatever it just built.
    """


EXIT_SANDBOX_PRESENT = 0
EXIT_SANDBOX_ABSENT = 1
EXIT_CANNOT_JUDGE = 2


def read_archive(path: Path) -> bytes:
    blob = path.read_bytes()
    if blob[:2] == b"\x1f\x8b":
        return gzip.decompress(blob)
    return blob


def ar_members(blob: bytes):
    """Yield (name, body) for every real member of a GNU ar archive."""
    if blob[:8] != b"!<arch>\n":
        raise CannotJudge(f"not an ar archive (magic {blob[:8]!r})")
    offset = 8
    longnames = b""
    while offset + 60 <= len(blob):
        header = blob[offset : offset + 60]
        name = header[0:16].decode("ascii", "replace").rstrip()
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError as exc:
            raise CannotJudge(f"malformed ar header at offset {offset}") from exc
        body = blob[offset + 60 : offset + 60 + size]
        if name == "//":
            longnames = body
        elif name.startswith("/") and name[1:].isdigit():
            start = int(name[1:])
            end = longnames.find(b"/", start)
            yield longnames[start:end].decode("ascii", "replace"), body
        elif name not in ("/", "/SYM64/"):
            yield name.rstrip("/"), body
        offset += 60 + size + (size & 1)


def reject_relocated_body(
    obj: bytes,
    sections: list[tuple[int, int, int, int, int, int]],
    section_index: int,
    st_value: int,
    st_size: int,
) -> None:
    """Refuse a wrapper whose bytes the linker will rewrite.

    The bytes in an object file are not always the bytes that end up in the
    binary: a relocation covering the immediate operand replaces it at link
    time. An object can therefore read `mov $1,%eax; ret` here and return 0 once
    linked, which makes this whole check answer a question about the wrong file.
    There is no reason for a constant-returning wrapper to carry relocations, so
    any that land inside it stop the build rather than being resolved.
    """
    for sh_type, sh_offset, sh_size, _sh_link, sh_info, sh_entsize in sections:
        if sh_type not in (4, 9) or sh_entsize == 0:  # SHT_RELA, SHT_REL
            continue
        if sh_info != section_index:
            continue
        for entry in range(0, sh_size, sh_entsize):
            (r_offset,) = struct.unpack_from("<Q", obj, sh_offset + entry)
            if st_value <= r_offset < st_value + st_size:
                raise CannotJudge(
                    f"{WRAPPER} carries a relocation at offset "
                    f"{r_offset - st_value} of its body. The linker rewrites "
                    "those bytes, so what this file shows is not what the "
                    "binary would return."
                )


def wrapper_body(obj: bytes) -> tuple[int, bytes] | None:
    """Return (e_machine, instruction bytes) for the linkable WRAPPER definition.

    Only GLOBAL and WEAK definitions count. A LOCAL symbol of the same name is
    invisible to the linker, so an archive holding a LOCAL "returns true" next
    to a GLOBAL "returns false" links the false one -- reading whichever came
    first would certify a sandbox that is not in the final binary.
    """
    if obj[:4] != b"\x7fELF" or obj[4] != 2:
        return None
    if obj[5] != 1:
        # EI_DATA: every offset below is decoded little-endian. A big-endian
        # object would parse into plausible nonsense rather than fail.
        raise CannotJudge("big-endian ELF; this check decodes little-endian only")
    machine = struct.unpack_from("<H", obj, 18)[0]
    (e_shoff,) = struct.unpack_from("<Q", obj, 40)
    e_shentsize, e_shnum, _e_shstrndx = struct.unpack_from("<HHH", obj, 58)
    sections = []
    for index in range(e_shnum):
        base = e_shoff + index * e_shentsize
        _name, sh_type, _flags, _addr, sh_offset, sh_size, sh_link = struct.unpack_from(
            "<IIQQQQI", obj, base
        )
        (sh_info,) = struct.unpack_from("<I", obj, base + 44)
        (sh_entsize,) = struct.unpack_from("<Q", obj, base + 56)
        sections.append((sh_type, sh_offset, sh_size, sh_link, sh_info, sh_entsize))

    found: list[bytes] = []
    for sh_type, sh_offset, sh_size, sh_link, _sh_info, sh_entsize in sections:
        if sh_type != 2 or sh_entsize == 0:  # SHT_SYMTAB
            continue
        _t, str_offset, str_size, _l, _i, _e = sections[sh_link]
        strings = obj[str_offset : str_offset + str_size]
        for entry in range(0, sh_size, sh_entsize):
            base = sh_offset + entry
            st_name, st_info, _st_other, st_shndx = struct.unpack_from(
                "<IBBH", obj, base
            )
            st_value, st_size = struct.unpack_from("<QQ", obj, base + 8)
            if st_shndx == 0 or st_shndx >= len(sections):
                continue
            binding = st_info >> 4
            if binding not in (1, 2):  # STB_GLOBAL, STB_WEAK
                continue
            end = strings.find(b"\x00", st_name)
            if strings[st_name:end].decode("ascii", "replace") != WRAPPER:
                continue
            _t, target_offset, _s, _l, _i, _e = sections[st_shndx]
            start = target_offset + st_value
            body = obj[start : start + st_size]
            if len(body) != st_size:
                raise CannotJudge(
                    f"{WRAPPER} claims {st_size} bytes but the section holds "
                    f"{len(body)}; the object is truncated"
                )
            reject_relocated_body(obj, sections, st_shndx, st_value, st_size)
            found.append(body)

    if not found:
        return None
    if len({bytes(body) for body in found}) > 1:
        raise CannotJudge(
            f"one object defines {WRAPPER} more than once with different bodies; "
            "which one the linker takes is not something to guess at"
        )
    return machine, found[0]


def returned_constant_aarch64(body: bytes) -> int:
    """Decode a constant-returning aarch64 function; raise if it is anything else."""
    if len(body) % 4:
        raise Undecodable(f"body is {len(body)} bytes, not a whole number of A64 words")
    value: int | None = None
    for offset in range(0, len(body), 4):
        (word,) = struct.unpack_from("<I", body, offset)
        if word in (0xD503245F, 0xD503201F, 0xD503233F):  # BTI c / NOP / PACIASP-hint
            continue
        if word == 0x2A1F03E0:  # mov w0, wzr
            value = 0
            continue
        if (word & 0xFFE0001F) == 0x52800000:  # movz w0, #imm16
            value = (word >> 5) & 0xFFFF
            continue
        if word == 0xD65F03C0:  # ret
            if value is None:
                raise Undecodable("returns before setting w0")
            return value
        raise Undecodable(f"unrecognised A64 word 0x{word:08x} at byte {offset}")
    raise Undecodable("no ret found")


def returned_constant_x86_64(body: bytes) -> int:
    """Decode a constant-returning x86-64 function; raise if it is anything else."""
    value: int | None = None
    index = 0
    while index < len(body):
        if body[index : index + 4] == b"\xf3\x0f\x1e\xfa":  # endbr64
            index += 4
            continue
        if body[index : index + 3] == b"\x48\x89\xe5":  # mov %rsp,%rbp
            index += 3
            continue
        opcode = body[index]
        if opcode in (0x55, 0x5D, 0x90):  # push %rbp / pop %rbp / nop
            index += 1
            continue
        if body[index : index + 2] == b"\x31\xc0":  # xor %eax,%eax
            value = 0
            index += 2
            continue
        if opcode == 0xB0:  # mov $imm8,%al
            if index + 2 > len(body):
                raise Undecodable("body ends inside a mov $imm8,%al")
            value = body[index + 1]
            index += 2
            continue
        if opcode == 0xB8:  # mov $imm32,%eax
            if index + 5 > len(body):
                raise Undecodable("body ends inside a mov $imm32,%eax")
            (value,) = struct.unpack_from("<I", body, index + 1)
            index += 5
            continue
        if opcode == 0xC3:  # ret
            if value is None:
                raise Undecodable("returns before setting eax")
            return value
        raise Undecodable(f"unrecognised opcode 0x{opcode:02x} at byte {index}")
    raise Undecodable("no ret found")


DECODERS = {EM_X86_64: returned_constant_x86_64, EM_AARCH64: returned_constant_aarch64}


def main() -> int:
    if len(sys.argv) != 2:
        raise CannotJudge("usage: check_v8_sandbox.py <librusty_v8...a[.gz]>")

    path = Path(sys.argv[1])
    if not path.is_file():
        raise CannotJudge(f"{path} does not exist")
    blob = read_archive(path)

    sandbox_symbol_hits = 0
    # Collect every member that defines it, rather than stopping at the first.
    # Which member an archive's definition comes from is the linker's decision,
    # and reading one while it links another is how a check certifies something
    # that is not in the binary.
    definitions: list[tuple[str, int, bytes]] = []
    elf_members = 0
    for member, obj in ar_members(blob):
        sandbox_symbol_hits += obj.count(SANDBOX_SYMBOL_FRAGMENT)
        if obj[:4] == b"\x7fELF":
            elf_members += 1
        candidate = wrapper_body(obj)
        if candidate is not None:
            machine, body = candidate
            definitions.append((member, machine, body))

    if elf_members == 0:
        raise CannotJudge(
            f"{path} holds no ELF members. A Windows .lib holds COFF objects, "
            "which this check does not decode -- teach it that format rather "
            "than letting the archive through unjudged."
        )
    if not definitions:
        raise CannotJudge(
            f"{path} defines no linkable {WRAPPER}. Either it is not a rusty_v8 "
            "archive, or the binding was renamed, or the only definition is LOCAL "
            "and the linker cannot see it; this check must be revisited rather "
            "than skipped, because skipping it is how the plain build shipped."
        )
    distinct = {(machine, bytes(body)) for _member, machine, body in definitions}
    if len(distinct) > 1:
        members = ", ".join(member for member, _m, _b in definitions)
        raise CannotJudge(
            f"{path} carries {len(definitions)} definitions of {WRAPPER} that do "
            f"not agree ({members}). The linker takes one of them and this check "
            "would be reading another."
        )

    member, machine, body = definitions[0]
    architecture = MACHINE_NAMES.get(machine, f"e_machine 0x{machine:x}")
    decoder = DECODERS.get(machine)
    if decoder is None:
        raise CannotJudge(
            f"{path} targets {architecture}, which this check cannot decode. Teach it "
            "that architecture rather than letting an unverified archive through."
        )

    print(f"archive: {path}")
    print(f"architecture: {architecture}")
    print(f"member defining {WRAPPER}: {member}")
    print(f"wrapper body: {body.hex(' ')}")
    # Reported, never decisive: see the module docstring.
    print(f"v8::internal::Sandbox name occurrences: {sandbox_symbol_hits}")

    try:
        value = decoder(body)
    except Undecodable as exc:
        raise CannotJudge(
            f"{WRAPPER} is not a constant-returning function ({exc}). This check "
            "only decides that shape; a build whose wrapper changed shape has to "
            "be looked at rather than assumed good."
        ) from exc

    # The wrapper returns `bool`, and both ABIs put that in the low byte of the
    # return register with only 0 and 1 defined. Treating "any non-zero" as true
    # is what the caller does not do: a function returning 256 leaves the low
    # byte clear, so C and Rust read false while a whole-register check reads
    # true. Decide on the byte the caller reads, and refuse anything that is not
    # a valid bool rather than pick an interpretation for it.
    print(f"{WRAPPER} returns: {value} (bool ABI reads low byte: {value & 0xFF})")
    if value not in (0, 1):
        raise CannotJudge(
            f"{WRAPPER} returns {value}, which is not a bool a compiler emits. Its "
            f"low byte is {value & 0xFF}, so the caller would read "
            f"{'true' if value & 0xFF else 'false'} while a whole-register reading "
            "says the opposite -- exactly the disagreement that must not be "
            "resolved by picking one."
        )
    if value == 0:
        print(
            "VERDICT: this V8 has no sandbox. Code mode would run without it, and "
            "nothing at runtime would say so.",
            file=sys.stderr,
        )
        return EXIT_SANDBOX_ABSENT
    print("VERDICT: the sandbox is compiled in.")
    return EXIT_SANDBOX_PRESENT


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CannotJudge as exc:
        print(f"cannot judge this archive: {exc}", file=sys.stderr)
        sys.exit(EXIT_CANNOT_JUDGE)
    except Exception as exc:  # noqa: BLE001
        # Everything unplanned lands here on purpose. A struct.error from a
        # truncated object, an OverflowError, a gzip read failure -- each one
        # exits 1 by default, and the producer's plain branch reads exit 1 as
        # "confirmed plain" and publishes an archive nobody judged. Say so
        # instead.
        print(
            f"cannot judge this archive: unexpected {type(exc).__name__}: {exc}",
            file=sys.stderr,
        )
        sys.exit(EXIT_CANNOT_JUDGE)
