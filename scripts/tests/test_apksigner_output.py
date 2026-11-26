"""Public certificate gate regressions; no SDK or device is required."""

import base64
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "android" / "test-signing"
SCRIPT = FIXTURE / "apksigner_output.py"
SPEC = importlib.util.spec_from_file_location("apksigner_output", SCRIPT)
signers = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(signers)
CERTIFICATE = (FIXTURE / "testkey.x509.pem").read_text()
EXPECTED = (FIXTURE / "certificate.sha256").read_text().strip()


def output(label="V2 Signer:", certificate=CERTIFICATE, count=1):
    return (f"Verifies\nNumber of signers: {count}\n"
            f"{label} certificate SHA-256 digest: {EXPECTED}\n"
            f"{label} public key SHA-256 digest: {'1' * 64}\n"
            + certificate)


class ApksignerOutputTests(unittest.TestCase):
    def test_current_and_legacy_labels_use_the_actual_public_certificate(self):
        # Actual label families from AOSP main and android17-release, including
        # the colon/no-# single-signer output which broke our first CI gate.
        for label in (
            "Signer #1", "V1 Signer:", "V2 Signer:", "V3.0 Signer:",
            "Signer (minSdkVersion=28, maxSdkVersion=2147483647)",
            "V3.1 Signer: (minSdkVersion=33 (dev release=true), maxSdkVersion=2147483647)",
        ):
            with self.subTest(label=label):
                self.assertEqual(signers.single_signer(output(label), EXPECTED), EXPECTED)

    def test_repeated_same_certificate_across_schemes_is_one_identity(self):
        data = output("V3.1 Signer:") + "V3.0 Signer:\n" + CERTIFICATE
        self.assertEqual(signers.single_signer(data, EXPECTED.upper()), EXPECTED)

    def test_digest_text_cannot_override_the_certificate(self):
        data = output().replace(EXPECTED, "0" * 64)
        self.assertEqual(signers.single_signer(data, EXPECTED), EXPECTED)
        with self.assertRaisesRegex(ValueError, "expected public development signer"):
            signers.single_signer(data, "0" * 64)

    def test_multiple_signers_are_rejected_even_if_the_certificate_repeats(self):
        with self.assertRaisesRegex(ValueError, "exactly one signer"):
            signers.single_signer(output(count=2) + CERTIFICATE, EXPECTED)

    def test_different_certificate_or_source_stamp_is_rejected(self):
        # Changing the signature byte retains the DER structure while creating a
        # different identity; apksigner itself is responsible for cryptographic
        # verification before this parser is called.
        der = base64.b64decode("".join(CERTIFICATE.splitlines()[1:-1]))
        other = (signers.BEGIN + "\n"
                 + base64.b64encode(der[:-1] + bytes([der[-1] ^ 1])).decode()
                 + "\n" + signers.END + "\n")
        for label in ("V3.1 Signer:", "Source Stamp Signer:"):
            with self.subTest(label=label):
                with self.assertRaisesRegex(ValueError, "single certificate identity"):
                    signers.single_signer(output() + label + "\n" + other, EXPECTED)

    def test_text_digest_or_public_key_block_is_not_a_certificate(self):
        for data in (
            "", f"Signer #1 certificate SHA-256 digest: {EXPECTED}\n",
            "-----BEGIN PUBLIC KEY-----\nYWJj\n-----END PUBLIC KEY-----\n",
        ):
            with self.subTest(data=data):
                with self.assertRaisesRegex(ValueError, "single certificate identity"):
                    signers.single_signer(data, EXPECTED)

    def test_malformed_empty_truncated_and_nested_blocks_fail_closed(self):
        for certificate in (
            signers.BEGIN + "\n" + signers.END,
            signers.BEGIN + "\n!invalid!\n" + signers.END,
            signers.BEGIN + "\nYWJ\n" + signers.END,
            CERTIFICATE.replace(signers.END, ""),
            CERTIFICATE.replace(signers.BEGIN, ""),
            signers.BEGIN + "\n" + CERTIFICATE + signers.END,
            CERTIFICATE + "-----BEGIN CERTIFICATE----\n",
        ):
            with self.subTest(certificate=certificate[:55]):
                with self.assertRaises(ValueError):
                    signers.single_signer(output(certificate=certificate), EXPECTED)

    def test_invalid_expected_fingerprint_fails_closed(self):
        for expected in ("", EXPECTED[:-1], "z" * 64):
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(ValueError, "Invalid expected"):
                    signers.single_signer(output(), expected)

    def test_crlf_pem_output_is_supported(self):
        self.assertEqual(signers.single_signer(output().replace("\n", "\r\n")), EXPECTED)

    def test_cli_failure_exposes_public_output_and_no_success_digest(self):
        data = output().replace(signers.END, "")
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--expected", EXPECTED],
            input=data, capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("Truncated certificate PEM", result.stderr)
        self.assertIn(data, result.stderr)

    def test_shell_gate_requires_successful_apksigner_and_pem(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            apk = root / "debug build.apk"
            apk.touch()
            fake = root / "fake-apksigner"
            fake.write_text(
                "#!/usr/bin/env bash\n"
                '[[ "$1 $2 $3" == "verify --verbose --print-certs-pem" ]] || exit 9\n'
                'cat "$PUBLIC_VERIFICATION"\n'
                'exit "${VERIFY_EXIT:-0}"\n'
            )
            fake.chmod(0o755)
            capture = root / "public-verification.log"
            env = dict(os.environ, APKSIGNER=str(fake), PUBLIC_VERIFICATION=str(capture))
            for case, data, code in (
                ("valid", output(), "0"),
                ("unverified", output(), "1"),
                ("text_without_pem", f"Signer #1 certificate SHA-256 digest: {EXPECTED}\n", "0"),
                ("truncated", output().replace(signers.END, ""), "0"),
            ):
                with self.subTest(case=case):
                    capture.write_text(data)
                    result = subprocess.run(
                        ["bash", str(FIXTURE / "verify-apk.sh"), str(apk)],
                        env=dict(env, VERIFY_EXIT=code), capture_output=True, text=True, check=False,
                    )
                    if case == "valid":
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertIn("APK development signer verified: " + EXPECTED, result.stdout)
                    else:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertNotIn("signer verified", result.stdout)
                        self.assertIn(data.rstrip(), result.stderr)


if __name__ == "__main__":
    unittest.main()
