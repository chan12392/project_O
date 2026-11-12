import importlib.util
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import shutil
from subprocess import CompletedProcess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("dependencies", ROOT / "scripts/check-dependencies.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class DependencyCheck(unittest.TestCase):
    def test_checked_in_patch_and_both_locks(self):
        CHECK.verify_glib(ROOT)
        packages = CHECK.locked_packages(ROOT)
        self.assertIn(("glib", "0.18.5"), packages)
        self.assertIn(("gtk", "0.18.2"), packages)
        self.assertEqual(len(packages), len(set(packages)))

    def test_source_tampering_including_extra_files_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="project-o-dependencies-") as directory:
            root = Path(directory)
            shutil.copytree(ROOT / CHECK.GLIB_PATH, root / CHECK.GLIB_PATH)
            shutil.copy(ROOT / "Cargo.toml", root / "Cargo.toml")
            (root / "tests/glib-variant").mkdir(parents=True)
            shutil.copy(ROOT / "tests/glib-variant/Cargo.toml", root / "tests/glib-variant/Cargo.toml")
            source = root / CHECK.GLIB_PATH / "src/variant_iter.rs"
            original = source.read_bytes()
            source.write_bytes(original.replace(b"&mut p,", b"&p,"))
            with self.assertRaisesRegex(ValueError, "differs"):
                CHECK.verify_glib(root)
            source.write_bytes(original)
            (root / CHECK.GLIB_PATH / "build.rs").write_text("fn main() {}")
            with self.assertRaisesRegex(ValueError, "differs"):
                CHECK.verify_glib(root)

    def test_original_registry_glib_does_not_receive_a_backport_exemption(self):
        original = CHECK.read_toml(ROOT / "Cargo.lock")
        next(p for p in original["package"] if p["name"] == "glib")["source"] = CHECK.REGISTRY
        with patch.object(CHECK, "read_toml", return_value=original):
            with self.assertRaisesRegex(ValueError, "local 0.18.5"):
                CHECK.locked_packages(ROOT)

    def test_cargo_must_resolve_the_reviewed_source_with_a_current_lockfile(self):
        package = {"name": "glib", "version": "0.18.5", "source": None,
                   "manifest_path": str(ROOT / CHECK.GLIB_PATH / "Cargo.toml")}
        def result(code=0, **change):
            return lambda *args, **kwargs: CompletedProcess([], code, json.dumps({"packages": [{**package, **change}]}), "")
        CHECK.verify_resolution(ROOT, result())
        for run in (result(1), result(source=CHECK.REGISTRY), result(manifest_path="another/Cargo.toml")):
            with self.assertRaises(ValueError):
                CHECK.verify_resolution(ROOT, run)

    def test_batch_order_and_pagination_preserve_the_affected_version(self):
        calls = []
        def request(url, body):
            calls.append(body["queries"])
            if len(calls) == 1:
                return {"results": [{"vulns": [{"id": "FIRST"}], "next_page_token": "next"}, {}]}
            self.assertEqual(body["queries"][0]["page_token"], "next")
            self.assertEqual(body["queries"][0]["package"]["name"], "crate-a")
            return {"results": [{"vulns": [{"id": "LAST"}]}]}
        self.assertEqual(CHECK.query_osv([("crate-a", "1.2.3"), ("crate-b", "2.0.0")], request),
                         [("crate-a", "1.2.3", "FIRST"), ("crate-a", "1.2.3", "LAST")])

    def test_empty_malformed_or_repeating_results_never_pass(self):
        for result in ({}, {"results": []}, {"results": [{"vulns": None}]},
                       {"results": [{"vulns": [{"id": None}]}]},
                       {"results": [{"next_page_token": "again"}]}):
            with self.subTest(result=result), self.assertRaises(ValueError):
                CHECK.query_osv([("glib", "0.18.5")], lambda *args: result)

    def test_only_the_exact_reviewed_backport_is_classified_fixed(self):
        self.assertEqual(CHECK.classify(("glib", "0.18.5", "GHSA-wrw7-89jp-8q8g")), "BACKPORTED")
        self.assertEqual(CHECK.classify(("glib", "0.18.4", "GHSA-wrw7-89jp-8q8g")), "VULNERABLE")
        self.assertEqual(CHECK.classify(("glib", "0.18.5", "GHSA-new-advisory")), "VULNERABLE")
        self.assertEqual(CHECK.classify(("another", "0.18.5", "GHSA-wrw7-89jp-8q8g")), "VULNERABLE")

    def test_unmaintained_is_visible_but_unsound_is_a_failure(self):
        def record(kind):
            return {"id": "RUSTSEC-example", "affected": [{"package": {"name": "example", "ecosystem": "crates.io"},
                                                           "database_specific": {"informational": kind}}]}
        finding = ("example", "1.0.0", "RUSTSEC-example")
        self.assertEqual(CHECK.classify(finding, lambda *args: record("unmaintained")), "UNMAINTAINED")
        self.assertEqual(CHECK.classify(finding, lambda *args: record("unsound")), "VULNERABLE")

    def test_npm_error_or_nonzero_exit_never_looks_clean(self):
        clean = {"metadata": {"vulnerabilities": {"total": 0}}}
        CHECK.npm_audit(ROOT, lambda *args, **kwargs: CompletedProcess([], 0, json.dumps(clean), ""))
        for code, report in ((1, clean), (0, {}), (0, {**clean, "error": {"code": "NETWORK"}}),
                             (0, {"metadata": {"vulnerabilities": {"total": 1}}})):
            with self.subTest(code=code, report=report), self.assertRaises(ValueError):
                CHECK.npm_audit(ROOT, lambda *args, **kwargs: CompletedProcess([], code, json.dumps(report), ""))

    def test_transport_failure_and_new_vulnerability_fail_the_command(self):
        output = StringIO()
        with redirect_stdout(output), redirect_stderr(output), patch.object(CHECK, "npm_audit"), patch.object(CHECK, "verify_resolution"), \
                patch.object(CHECK, "query_osv", side_effect=OSError("offline")):
            self.assertEqual(CHECK.main(ROOT), 1)
        self.assertIn("FAIL: offline", output.getvalue())
        with redirect_stdout(output), redirect_stderr(output), patch.object(CHECK, "npm_audit"), patch.object(CHECK, "verify_resolution"), \
                patch.object(CHECK, "query_osv", return_value=[("glib", "0.18.5", "GHSA-new")]):
            self.assertEqual(CHECK.main(ROOT), 1)
        self.assertIn("FAIL: glib@0.18.5: GHSA-new", output.getvalue())


if __name__ == "__main__":
    unittest.main()
