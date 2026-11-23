"""Fetch a pinned Microsoft VC headers VSIX into this checkout, without an installer."""
import hashlib
from pathlib import Path
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / ".local" / "msvc-headers-14.51"
SHA256 = "e6d9cc5f0750cbc40a6b2468991c57ed0044fa2e242295a6d51bc02d72502b05"
URL = ("https://download.visualstudio.microsoft.com/download/pr/a62262cf-b9f6-4770-9454-7372ad8fcec0/"
       + SHA256 + "/Microsoft.VC.14.51.CRT.Headers.base.vsix")
# The SHA-256 is independently pinned in Firefox's build/vs/vs2026.yaml.
TARGET.mkdir(parents=True, exist_ok=True)
data = urllib.request.urlopen(URL, timeout=60).read(10 * 1024 * 1024)
actual = hashlib.sha256(data).hexdigest()
if actual != SHA256:
    raise RuntimeError(f"MSVC headers SHA-256 mismatch: expected {SHA256}, got {actual}")
archive_path = TARGET / "headers.vsix"
archive_path.write_bytes(data)
try:
    with zipfile.ZipFile(archive_path) as archive:
        extracted = 0
        for item in archive.infolist():
            if item.is_dir():
                continue
            extracted += item.file_size
            if extracted > 64 * 1024 * 1024:
                raise RuntimeError("MSVC header archive expands beyond the allowed size")
            target = (TARGET / item.filename).resolve()
            if not target.is_relative_to(TARGET.resolve()):
                raise RuntimeError("invalid VSIX path")
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(item) as source, target.open("wb") as output:
                while chunk := source.read(1024 * 1024):
                    output.write(chunk)
finally:
    archive_path.unlink(missing_ok=True)
print("Pinned MSVC headers extracted locally; no installer was run.")
