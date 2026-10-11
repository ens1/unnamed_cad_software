#!/usr/bin/env python3
"""Bundle dependency notices from the locked Cargo dependency graph."""
import json
import pathlib
import subprocess
import shutil

root = pathlib.Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root))
output = root / "third-party-licenses"
output.mkdir(exist_ok=True)
lines = ["# Third-party notices", "", "TCAD's original code is WTFPL. Dependencies retain their own licenses. This catalog covers the locked Cargo graph, including dependencies used only on other platforms and build tooling. License files supplied by each locked dependency are copied below. Git dependencies also record their exact source revision.", "", "| Dependency | Version | Declared license | Notices |", "|---|---|---|---|"]
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if not package.get("source"):
        continue
    directory = pathlib.Path(package["manifest_path"]).parent
    name = f'{package["name"]}-{package["version"]}'
    destination = output / name
    destination.mkdir(exist_ok=True)
    candidates = [p for p in directory.iterdir() if p.is_file() and any(p.name.upper().startswith(prefix) for prefix in ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"))]
    license_file = package.get("license_file")
    if license_file:
        candidate = directory / license_file
        if candidate.is_file() and candidate not in candidates:
            candidates.append(candidate)
    if package["source"].startswith("git+"):
        # Git workspace members can inherit the repository's root license;
        # unlike a published crate, the complete checkout is available here.
        for ancestor in (directory, *directory.parents):
            manifest = ancestor / "Cargo.toml"
            if manifest.is_file() and "[workspace]" in manifest.read_text():
                for candidate in ancestor.iterdir():
                    if candidate.is_file() and any(candidate.name.upper().startswith(prefix) for prefix in ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT")):
                        if not any(existing.name == candidate.name for existing in candidates):
                            candidates.append(candidate)
                break
        (destination / "PACKAGE.txt").write_text(f'{name}\nDeclared SPDX license: {package.get("license") or "See package"}\nUpstream: {package.get("repository") or package.get("homepage") or "See source"}\nLocked source: {package["source"]}\nDependency modifications retain the upstream license.\n')
    for candidate in candidates:
        shutil.copyfile(candidate, destination / candidate.name)
    if not candidates and not package["source"].startswith("git+"):
        (destination / "PACKAGE.txt").write_text(f'{name}\nDeclared SPDX license: {package.get("license") or "See package"}\nSource: {package.get("repository") or package.get("homepage") or package["source"]}\nThe published crate did not include a top-level license text.\n')
    links = ", ".join(f'[{p.name}](third-party-licenses/{name}/{p.name})' for p in sorted(destination.iterdir()))
    lines.append(f'| {package["name"]} | {package["version"]} | {package.get("license") or "See notices"} | {links} |')
(root / "THIRD_PARTY_NOTICES.md").write_text("\n".join(lines) + "\n")
print(f"Recorded notices for {len(metadata['packages'])} Cargo packages")
