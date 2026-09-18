"""Select already provisioned Windows build tools without unrelated PATH entries.

This is an input-selection contract, not a sandbox or a hashed tool closure.
The selected tools still depend on their installed support files and Windows.
"""

import json
import os
from pathlib import Path


def build_environment(path, target, selected_tools):
    if path.stat().st_size > 65536:
        raise ValueError("Windows build input document exceeds limits")
    inputs = json.loads(path.read_text(encoding="utf-8"))
    expected = {
        "cc": "cl.exe",
        "cxx": "cl.exe",
        "link": "link.exe",
        "lib": "lib.exe",
        "dumpbin": "dumpbin.exe",
        "assembler": "ml64.exe" if target.startswith("x86_64-") else "armasm64.exe",
        "rc": "rc.exe",
        "mt": "mt.exe",
        "cmake": "cmake.exe",
        "bootstrap_make": "nmake.exe",
        "make": "make.exe",
        "shell": "bash.exe",
        "cygpath": "cygpath.exe",
        "automake": "automake-1.18",
    }
    if (
        target not in ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")
        or inputs.get("schemaVersion") != 1
        or inputs.get("target") != target
        or set(inputs.get("tools", {})) != {*expected, "pkg_config", "python"}
    ):
        raise ValueError("Windows build inputs do not match the target and tool set")
    tools = {}
    for name, value in inputs["tools"].items():
        tool = Path(value)
        if (
            not tool.is_absolute()
            or not tool.is_file()
            or ";" in str(tool)
            or (name in expected and tool.name.lower() != expected[name])
        ):
            raise ValueError(f"Invalid Windows build tool: {name}")
        tools[name] = tool
    for name, tool in selected_tools.items():
        if not tools[name].samefile(tool):
            raise ValueError(f"Windows build input disagrees with --{name}")
    # libffi invokes cl/link/lib and its architecture's assembler by name.
    # Keep MSVC ahead of Cygwin, which also installs a different link.exe.
    directories = list(dict.fromkeys(tools[name].parent for name in expected))
    directories += [tools[name].parent for name in ("pkg_config", "python")]
    directories = list(dict.fromkeys(directories))
    for name, filename in expected.items():
        found = next(
            (p / filename for p in directories if (p / filename).is_file()), None
        )
        if found is None or not found.samefile(tools[name]):
            raise ValueError(f"Windows build tool is shadowed: {name}")
    system = Path(inputs["systemRoot"])
    command = system / "System32/cmd.exe"
    if not system.is_absolute() or not command.is_file() or ";" in str(system):
        raise ValueError(
            "Windows build inputs require a valid system command directory"
        )
    directories.append(command.parent)
    environment = {
        "PATH": os.pathsep.join(map(str, directories)),
        "SystemRoot": str(system),
        "SYSTEMROOT": str(system),
        "WINDIR": str(system),
        "COMSPEC": str(command),
    }
    for name in ("INCLUDE", "LIB"):
        values = inputs.get(name)
        if not isinstance(values, list) or not 1 <= len(values) <= 64:
            raise ValueError(
                f"Windows build inputs require explicit {name} directories"
            )
        for value in values:
            directory = Path(value)
            if not directory.is_absolute() or not directory.is_dir() or ";" in value:
                raise ValueError(f"Invalid Windows {name} directory")
        environment[name] = os.pathsep.join(values)
    return environment, inputs
