"""Collects the license files of every Rust crate linked into dpdf_voice.dll.

Usage: python scripts/collect-licenses.py <out dir>

Writes <out dir>/<crate>-<version>/<license files> plus <out dir>/CRATES.md.
Only normal (non-build, non-dev) dependencies of dpdf-voice-vst are included,
since only those end up in the shipped DLL. Fails if a crate has no license
file, so nothing ships without its notice.
"""
import json
import os
import shutil
import subprocess
import sys

TARGET = "x86_64-pc-windows-msvc"
PATTERNS = ("license", "licence", "copying", "notice", "copyright")


def linked_crates(root):
    out = subprocess.run(
        ["cargo", "tree", "-p", "dpdf-voice-vst", "-e", "normal", "--target", TARGET,
         "--prefix", "none", "--format", "{p}"],
        cwd=root, check=True, capture_output=True, text=True).stdout
    crates = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            crates.add((parts[0], parts[1][1:]))
    return crates


def license_files(pkg_dir):
    found = []
    for base, dirs, files in os.walk(pkg_dir):
        dirs[:] = [d for d in dirs if d not in ("target", ".git", "tests", "examples", "benches")]
        for f in files:
            if f.lower().startswith(PATTERNS):
                found.append(os.path.join(base, f))
    return sorted(found)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out_dir = sys.argv[1]
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET],
        cwd=root, check=True, capture_output=True, text=True).stdout)
    packages = {(p["name"], p["version"]): p for p in meta["packages"]}
    workspace = set(meta["workspace_members"])

    rows, missing = [], []
    for name, version in sorted(linked_crates(root)):
        pkg = packages[(name, version)]
        if pkg["id"] in workspace:
            continue  # dpdf-voice itself: LICENSE-MIT / LICENSE-APACHE at the top level
        pkg_dir = os.path.dirname(pkg["manifest_path"])
        files = license_files(pkg_dir)
        if not files:
            missing.append(f"{name} {version}")
            continue
        dest = os.path.join(out_dir, f"{name}-{version}")
        os.makedirs(dest, exist_ok=True)
        for f in files:
            rel = os.path.relpath(f, pkg_dir).replace(os.sep, "-")
            shutil.copyfile(f, os.path.join(dest, rel))
        rows.append((name, version, pkg.get("license") or "see files", len(files)))

    if missing:
        sys.exit("no license file found for: " + ", ".join(missing))
    with open(os.path.join(out_dir, "CRATES.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# Rust crates linked into dpdf_voice.dll\n\n")
        f.write("Each crate's license files are in the folder `<crate>-<version>/`.\n\n")
        f.write("| Crate | Version | License |\n|---|---|---|\n")
        for name, version, lic, _ in rows:
            f.write(f"| {name} | {version} | {lic} |\n")
    print(f"{len(rows)} crates, license files in {out_dir}")


if __name__ == "__main__":
    main()
