"""Fetch pinned Microsoft WDK validation tools into the checkout, never install."""
import base64
import hashlib
from pathlib import Path
import shutil
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / ".local" / "wdk-validation"
ARCHIVE = BASE / "wdk.nupkg"
CHECKSUM = BASE / "wdk.sha512"
URL = ("https://api.nuget.org/v3-flatcontainer/microsoft.windows.wdk.x64/"
       "10.0.26100.1/microsoft.windows.wdk.x64.10.0.26100.1.nupkg")
MAX_ARCHIVE = 256 * 1024 * 1024
MAX_EXTRACTED = 512 * 1024 * 1024


def download():
    BASE.mkdir(parents=True, exist_ok=True)
    temporary = BASE / "wdk.download"
    digest = hashlib.sha512()
    total = 0
    try:
        with urllib.request.urlopen(URL, timeout=60) as response, temporary.open("wb") as output:
            expected = response.headers.get("x-ms-meta-SHA512")
            if not expected:
                raise RuntimeError("Microsoft package did not provide a SHA-512 digest")
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                if total > MAX_ARCHIVE:
                    raise RuntimeError("WDK download exceeds size limit")
                digest.update(chunk)
                output.write(chunk)
        if base64.b64encode(digest.digest()).decode("ascii") != expected:
            raise RuntimeError("WDK download failed SHA-512 verification")
        temporary.replace(ARCHIVE)
        CHECKSUM.write_text(expected, encoding="ascii")
        with zipfile.ZipFile(ARCHIVE) as archive:
            names = [name for name in archive.namelist() if "infverif" in name.lower() or "inf2cat" in name.lower()]
        print(f"Verified {total} bytes; candidate tools:")
        print("\n".join(names) if names else "No INF validation tools found in this package.")
    finally:
        temporary.unlink(missing_ok=True)


def extract():
    if not ARCHIVE.is_file() or not CHECKSUM.is_file():
        raise RuntimeError("Run download first")
    digest = hashlib.sha512()
    with ARCHIVE.open("rb") as package:
        while chunk := package.read(1024 * 1024):
            digest.update(chunk)
    if base64.b64encode(digest.digest()).decode("ascii") != CHECKSUM.read_text(encoding="ascii"):
        raise RuntimeError("Downloaded WDK archive changed after verification")
    target = BASE / "tools"
    expanded = 0
    with zipfile.ZipFile(ARCHIVE) as archive:
        names = archive.namelist()
        candidates = [n for n in names if n.lower().endswith(("/infverif.exe", "/inf2cat.exe"))]
        if not candidates:
            raise RuntimeError("No INF validation tools found")
        # The INF2CAT directory is small. INFVERIF's tool directory also holds
        # hundreds of unrelated tools, so select only INFVERIF and its two
        # WDK validation DLLs rather than extracting that entire directory.
        inf2cat_dirs = {n.rsplit("/", 1)[0] + "/" for n in candidates if n.lower().endswith("/inf2cat.exe")}
        support = {n for n in names if n.lower().endswith(("/bin/x64/infverif.dll", "/bin/x64/infverifhlk.dll"))}
        selected = set(candidates) | support
        for info in archive.infolist():
            if info.is_dir() or not (info.filename in selected or any(info.filename.startswith(d) for d in inf2cat_dirs)):
                continue
            expanded += info.file_size
            if expanded > MAX_EXTRACTED:
                raise RuntimeError("WDK validation tools exceed extraction limit")
            path = (target / info.filename).resolve()
            if not path.is_relative_to(target.resolve()):
                raise RuntimeError("WDK archive path escapes the project")
            path.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(info) as source, path.open("wb") as output:
                shutil.copyfileobj(source, output)
        for exe in candidates:
            if exe.lower().endswith("/infverif.exe"):
                for dll in support:
                    shutil.copy2(target / dll, (target / exe).parent / Path(dll).name)
    print(f"Extracted {expanded} bytes of tool files under {target}")
    for candidate in candidates:
        print(target / candidate)
    ARCHIVE.unlink()
    CHECKSUM.unlink()
    print("Removed downloaded archive; no installer was run.")


if __name__ == "__main__":
    if sys.argv[1:] == ["download"]:
        download()
    elif sys.argv[1:] == ["extract"]:
        extract()
    else:
        raise SystemExit("Usage: fetch-wdk-validation.py download|extract")
