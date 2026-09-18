"""Copy declared library payloads while keeping a real native-search directory."""

from pathlib import Path
import shutil
import sys


def copy_payloads(locator, pairs):
    if len(pairs) % 2:
        raise ValueError("library copies require source/destination pairs")
    Path(locator).mkdir(parents=True, exist_ok=True)
    for source, destination in zip(pairs[::2], pairs[1::2], strict=True):
        output = Path(destination)
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, output)


if __name__ == "__main__":
    copy_payloads(sys.argv[1], sys.argv[2:])
