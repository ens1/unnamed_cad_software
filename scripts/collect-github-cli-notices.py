#!/usr/bin/env python3
"""Collect redistribution notices from the pinned helper's Go build metadata.

This reads a binary; it does not execute it. `go mod download` verifies module
checksums and downloads source archives without running dependency code.
"""
import concurrent.futures
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "third-party-licenses/github-cli-2.83.2"
REVISION = "560ea94fc7d63ab1f56ca47713599be5e8966fd3"


def collect(module):
    name, version, expected_sum = module
    result = subprocess.run(
        ["go", "mod", "download", "-json", f"{name}@{version}"],
        capture_output=True, text=True, timeout=180, check=True,
    )
    metadata = json.loads(result.stdout)
    if metadata.get("Sum") != expected_sum:
        raise ValueError(f"Module checksum changed: {name}@{version}")
    source = pathlib.Path(metadata["Dir"])
    key = name.replace("/", "__") + "@" + version
    target = DESTINATION / "modules" / key
    notices = []
    for file in sorted(source.rglob("*")):
        if not file.is_file() or file.is_symlink():
            continue
        if not re.match(r"^(licen[cs]e|notice|copying|copyright|authors)([._-].*)?$", file.name, re.I):
            continue
        relative = file.relative_to(source)
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(file, destination)
        notices.append(str(destination.relative_to(DESTINATION)))
    if not notices:
        raise ValueError(f"No redistribution notice found for {name}@{version}")
    return {"module": name, "version": version, "sum": expected_sum, "notices": notices}


def main():
    binary = pathlib.Path(sys.argv[1]).resolve()
    info = subprocess.check_output(["go", "version", "-m", str(binary)], text=True)
    if f"vcs.revision={REVISION}" not in info:
        raise ValueError("The binary is not from the pinned GitHub CLI revision")
    modules = []
    for line in info.splitlines():
        fields = line.split()
        if fields and fields[0] == "=>":
            raise ValueError("Review module replacements before collecting notices")
        if fields and fields[0] == "dep":
            modules.append(tuple(fields[1:4]))
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as executor:
        inventory = list(executor.map(collect, modules))
    go_version = info.splitlines()[0].split()[-1]
    runtime_url = f"https://raw.githubusercontent.com/golang/go/{go_version}/LICENSE"
    with urllib.request.urlopen(runtime_url, timeout=30) as response:
        runtime_license = response.read(128 * 1024)
    (DESTINATION / "GO-LICENSE").write_bytes(runtime_license)
    manifest = {
        "helper": "GitHub CLI 2.83.2", "source_revision": REVISION,
        "go_runtime": go_version, "go_license_source": runtime_url,
        "go_license_sha256": hashlib.sha256(runtime_license).hexdigest(),
        "modules": inventory,
    }
    (DESTINATION / "modules.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Collected {len(inventory)} module notices and the {go_version} runtime license")


if __name__ == "__main__":
    main()
