"""Fetch only the x64 KMDF headers/libraries into this checkout; no installer."""
import base64
import hashlib
import json
from pathlib import Path
import shutil
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / ".local" / "wdk-26100"
VERSION = "10.0.26100.1"
KIT = "10.0.26100.0"
MAX_ARCHIVE = 600 * 1024 * 1024
MAX_EXTRACTED = 1024 * 1024 * 1024
PACKAGES = ("microsoft.windows.wdk.x64", "microsoft.windows.sdk.cpp")
PREFIXES = tuple(x.lower() for x in (
    f"c/include/{KIT}/km/", f"c/include/{KIT}/shared/", f"c/include/{KIT}/ucrt/",
    "c/include/wdf/kmdf/1.33/", f"c/lib/{KIT}/km/x64/", "c/lib/wdf/kmdf/x64/1.33/",
))


def fetch(package):
    url = f"https://api.nuget.org/v3-flatcontainer/{package}/{VERSION}/{package}.{VERSION}.nupkg"
    temporary = TARGET / (package + ".download")
    sha = hashlib.sha512()
    received = 0
    try:
        with urllib.request.urlopen(url, timeout=60) as response, temporary.open("wb") as output:
            expected = response.headers.get("x-ms-meta-SHA512")
            if not expected:
                raise RuntimeError(f"no NuGet SHA-512 header for {package}")
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                received += len(chunk)
                if received > MAX_ARCHIVE:
                    raise RuntimeError("WDK archive exceeds limit")
                sha.update(chunk)
                output.write(chunk)
        if base64.b64encode(sha.digest()).decode("ascii") != expected:
            raise RuntimeError(f"NuGet SHA-512 mismatch for {package}")
        extracted = 0
        with zipfile.ZipFile(temporary) as archive:
            for item in archive.infolist():
                name = item.filename.lower()
                if item.is_dir() or not (name.startswith(PREFIXES) or name == "license.txt"):
                    continue
                extracted += item.file_size
                if extracted > MAX_EXTRACTED:
                    raise RuntimeError("WDK extracted data exceeds limit")
                dest = (TARGET / item.filename).resolve()
                if not dest.is_relative_to(TARGET.resolve()):
                    raise RuntimeError("invalid WDK archive path")
                if name == "license.txt":
                    dest = TARGET / "licenses" / (package + ".txt")
                dest.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(item) as source, dest.open("wb") as output:
                    shutil.copyfileobj(source, output)
        return {"package": package, "version": VERSION, "archive_bytes": received, "extracted_bytes": extracted, "sha512": expected}
    finally:
        temporary.unlink(missing_ok=True)


if __name__ == "__main__":
    TARGET.mkdir(parents=True, exist_ok=True)
    metadata = []
    for package in PACKAGES:
        print(f"Downloading {package}", flush=True)
        metadata.append(fetch(package))
    (TARGET / "manifest.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    print("WDK files extracted locally; no drivers installed.")
