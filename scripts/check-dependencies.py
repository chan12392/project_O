"""npm audit + OSV's RustSec/GHSA data for every locked Rust package (Python 3.11+)."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib
from urllib.request import Request, urlopen
from urllib.parse import quote


GLIB_PATH = "vendor/glib-0.18.5"
GLIB_TREE = "2f7ef2019fd0cb8cb2b95f3a3a4d08b63c4ea546362eae332643a6a6de0e67bf"
GLIB_FIXED = {"RUSTSEC-2024-0429", "GHSA-wrw7-89jp-8q8g"}
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
LOCAL = {"o-cli", "o-core", "o-guard", "project-o", "glib-variant-regression"}
LOCKS = ("Cargo.lock", "tests/glib-variant/Cargo.lock")


def read_toml(path):
    return tomllib.loads(path.read_text(encoding="utf-8"))


def verify_glib(root):
    """A version-based advisory may be marked backported only after this succeeds."""
    if read_toml(root / "Cargo.toml").get("patch", {}).get("crates-io", {}).get("glib") != {"path": GLIB_PATH}:
        raise ValueError("Cargo.toml must resolve glib to the reviewed local patch")
    fixture = read_toml(root / "tests/glib-variant/Cargo.toml")
    if fixture["dependencies"]["glib"] != {"path": "../../" + GLIB_PATH}:
        raise ValueError("the Linux regression must use the same glib source")
    entries = []
    source = root / GLIB_PATH
    if source.is_symlink() or not source.is_dir():
        raise ValueError("missing or linked glib source")
    for path in sorted(source.rglob("*"), key=lambda p: p.relative_to(source).as_posix()):
        if path.is_symlink():
            raise ValueError(f"linked glib source: {path}")
        if path.is_file():
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            entries.append(f"{path.relative_to(source).as_posix()}\0{digest}\n")
    if hashlib.sha256("".join(entries).encode()).hexdigest() != GLIB_TREE:
        raise ValueError("glib source differs from the reviewed archive + upstream two-line patch")


def locked_packages(root):
    packages = set()
    for name in LOCKS:
        lock = read_toml(root / name)
        if lock.get("version") != 4 or not lock.get("package"):
            raise ValueError(f"unsupported or empty lockfile: {name}")
        glib = [p for p in lock["package"] if p["name"] == "glib"]
        if len(glib) != 1 or glib[0]["version"] != "0.18.5" or "source" in glib[0]:
            raise ValueError(f"{name}: glib must be the local 0.18.5 backport")
        for package in lock["package"]:
            source = package.get("source")
            if source == REGISTRY or (package["name"] == "glib" and source is None):
                packages.add((package["name"], package["version"]))
            elif source is not None or package["name"] not in LOCAL:
                raise ValueError(f"unreviewed dependency source: {package['name']} ({source})")
    return sorted(packages)


def verify_resolution(root, run=subprocess.run):
    result = run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root,
                 capture_output=True, text=True, timeout=120,
                 creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    if result.returncode != 0:
        raise ValueError("cargo metadata --locked failed: lockfile/manifest resolution must succeed")
    glib = [p for p in json.loads(result.stdout)["packages"] if p["name"] == "glib"]
    if len(glib) != 1 or glib[0]["version"] != "0.18.5" or glib[0]["source"] is not None \
            or Path(glib[0]["manifest_path"]).resolve() != (root / GLIB_PATH / "Cargo.toml").resolve():
        raise ValueError("Cargo resolved glib to a different source/version")


def request_json(url, body=None):
    request = Request(url, data=None if body is None else json.dumps(body).encode(),
                      headers={"Content-Type": "application/json", "User-Agent": "project_O dependency check"})
    with urlopen(request, timeout=60) as response:
        return json.load(response)


def query_osv(packages, request=request_json):
    findings = set()
    for start in range(0, len(packages), 500):
        pending = [(p, {}) for p in packages[start:start + 500]]
        seen = set()
        while pending:
            queries = [{"package": {"ecosystem": "crates.io", "name": p[0]}, "version": p[1], **page}
                       for p, page in pending]
            answer = request("https://api.osv.dev/v1/querybatch", {"queries": queries})
            results = answer.get("results")
            if not isinstance(results, list) or len(results) != len(pending):
                raise ValueError("OSV returned an incomplete batch")
            following = []
            for (package, _), result in zip(pending, results):
                if not isinstance(result, dict) or not isinstance(result.get("vulns", []), list):
                    raise ValueError("OSV returned a malformed package result")
                for vuln in result.get("vulns", []):
                    if not isinstance(vuln, dict) or not isinstance(vuln.get("id"), str) or not vuln["id"]:
                        raise ValueError("OSV returned a malformed advisory")
                    findings.add((*package, vuln["id"]))
                token = result.get("next_page_token", "")
                if not isinstance(token, str) or (token and (package, token) in seen):
                    raise ValueError("OSV returned an invalid/repeated page token")
                if token:
                    seen.add((package, token))
                    following.append((package, {"page_token": token}))
            pending = following
    return sorted(findings)


def classify(finding, request=request_json):
    name, version, advisory = finding
    if (name, version) == ("glib", "0.18.5") and advisory in GLIB_FIXED:
        return "BACKPORTED"
    # RustSec distinguishes lack of maintenance from a vulnerability. Unsound advisories fail.
    if advisory.startswith("RUSTSEC-"):
        record = request(f"https://api.osv.dev/v1/vulns/{quote(advisory, safe='')}")
        if record.get("id") != advisory or not isinstance(record.get("affected"), list):
            raise ValueError("OSV returned a malformed advisory record")
        affected = [a for a in record["affected"] if a.get("package", {}).get("name") == name
                    and a["package"].get("ecosystem") == "crates.io"]
        if affected and all(a.get("database_specific", {}).get("informational") == "unmaintained" for a in affected):
            return "UNMAINTAINED"
    return "VULNERABLE"


def npm_audit(folder, run=subprocess.run):
    command = ["cmd.exe", "/d", "/c", "npm.cmd"] if os.name == "nt" else ["npm"]
    result = run(command + ["audit", "--json", "--ignore-scripts", "--audit-level=low",
                            "--include=dev", "--include=optional", "--include=peer"], cwd=folder,
                 capture_output=True, text=True, timeout=120,
                 creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    report = json.loads(result.stdout)
    count = report.get("metadata", {}).get("vulnerabilities", {}).get("total")
    if result.returncode != 0 or type(count) is not int or count != 0 or "error" in report:
        raise ValueError(f"npm audit failed in {folder.name}: exit={result.returncode}, vulnerabilities={count}")


def main(root):
    errors = []
    for folder in (root, root / "app"):
        try:
            npm_audit(folder)
            print(f"PASS npm audit: {folder.name} (production + development)")
        except Exception as error:
            errors.append(str(error))
    try:
        verify_glib(root)
        verify_resolution(root)
        packages = locked_packages(root)
        findings = query_osv(packages)
        for finding in findings:
            status = classify(finding)
            name, version, advisory = finding
            print(f"{status}: {name}@{version} {advisory}")
            if status == "VULNERABLE":
                errors.append(f"{name}@{version}: {advisory}")
        print(f"Rust scan: {len(packages)} locked upstream package versions, all target platforms; glib source hash verified")
    except Exception as error:
        errors.append(str(error))
    for error in errors:
        print(f"FAIL: {error}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent))
