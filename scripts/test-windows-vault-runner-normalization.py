#!/usr/bin/env python3
"""Portable source-semantics checks; these do not execute PowerShell or Windows.

Read the runner's actual String.Replace arguments using PowerShell character casts
or single-quoted literal rules (only doubled apostrophes are escaped). Python's
replacement models .NET's ordinal replacement for these ASCII arguments.
"""

import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


def runner_replacement():
    source = (ROOT / "scripts/test-windows-vault-boundary.ps1").read_text()
    line, = [line for line in source.splitlines() if line.startswith("$actualNames =")]
    char_match = re.search(
        r"\.Substring\(\$root\.Length \+ 1\)\.Replace\(\[char\](\d+),\[char\](\d+)\)",
        line,
    )
    if char_match is not None:
        return tuple(chr(int(value)) for value in char_match.groups())
    match = re.search(
        r"\.Substring\(\$root\.Length \+ 1\)\.Replace\('((?:[^']|'')*)','((?:[^']|'')*)'\)",
        line,
    )
    if match is None:
        raise AssertionError("runner normalization expression needs a new semantics check")
    return tuple(literal.replace("''", "'") for literal in match.groups())


def normalized_relative_name(root, full_name):
    if not full_name.startswith(root + "\\"):
        raise AssertionError("synthetic full path must be under the root")
    old, new = runner_replacement()
    return full_name[len(root) + 1:].replace(old, new)


class RunnerNormalization(unittest.TestCase):
    def test_autocrlf_checkout_preserves_reviewed_helper_hashes(self):
        manifest = json.loads((ROOT / "vault-helper/resources-win-x64.json").read_text())
        names = ("helper.mjs", "crypto.mjs", "package.json", "package-lock.json")
        with tempfile.TemporaryDirectory(prefix="shixu-checkout-") as temporary:
            repository = Path(temporary) / "repository"
            checkout = Path(temporary) / "checkout"
            repository.mkdir()
            checkout.mkdir()
            (repository / "vault-helper").mkdir()
            (repository / ".gitattributes").write_bytes((ROOT / ".gitattributes").read_bytes())
            for name in names:
                canonical = subprocess.run(
                    ["git", "-C", str(ROOT), "show", f"HEAD:vault-helper/{name}"],
                    check=True, capture_output=True,
                ).stdout
                (repository / "vault-helper" / name).write_bytes(canonical)
            git = ["git", "-C", str(repository), "-c", "core.autocrlf=true", "-c", "core.safecrlf=false"]
            subprocess.run(git + ["init", "--quiet"], check=True, capture_output=True)
            subprocess.run(git + ["add", "--", ".gitattributes", "vault-helper"], check=True, capture_output=True)
            subprocess.run(
                git + ["checkout-index", "--all", "--prefix=" + checkout.as_posix() + "/"],
                check=True, capture_output=True,
            )
            for name in names:
                with self.subTest(name=name):
                    digest = hashlib.sha256((checkout / "vault-helper" / name).read_bytes()).hexdigest()
                    self.assertEqual(digest, manifest["files"]["helper/" + name])

    def test_actual_literal_is_one_windows_separator(self):
        self.assertEqual(runner_replacement(), ("\\", "/"))

    def test_faulty_two_backslash_literal_does_not_normalize(self):
        # Positive sensitivity control for the review's proposed defect: ordinary
        # Windows separators stay unchanged when the old substring has length 2.
        self.assertEqual("runtime\\node.exe".replace("\\\\", "/"), "runtime\\node.exe")
        self.assertNotEqual("runtime\\node.exe".replace("\\\\", "/"), "runtime/node.exe")

    def test_nested_names_with_unicode_and_spaces(self):
        root = "C:\\synthetic 空 格\\resources\\vault-win-x64"
        for relative in ("runtime/node.exe", "nested folder/子目录/file name.js"):
            with self.subTest(relative=relative):
                full_name = root + "\\" + relative.replace("/", "\\")
                self.assertEqual(normalized_relative_name(root, full_name), relative)

    def test_prepared_198_file_set_matches_manifest(self):
        prepared = ROOT / "resources/vault-win-x64"
        self.assertTrue(prepared.is_dir(), "prepare pinned resources before this check")
        manifest = json.loads((ROOT / "vault-helper/resources-win-x64.json").read_text())
        root = "C:\\synthetic 空 格\\resources\\vault-win-x64"
        actual = sorted(
            normalized_relative_name(
                root, root + "\\" + "\\".join(path.relative_to(prepared).parts)
            )
            for path in prepared.rglob("*") if path.is_file()
        )
        self.assertEqual(len(actual), 198)
        self.assertEqual(len(manifest["files"]), 198)
        self.assertEqual(actual, sorted(manifest["files"]))


if __name__ == "__main__":
    unittest.main(verbosity=2)
