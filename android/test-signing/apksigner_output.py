#!/usr/bin/env python3
"""Read the sole certificate identity from successful apksigner PEM output.

Callers must first require `apksigner verify --verbose --print-certs-pem` to
exit successfully. We hash the actual X.509 DER, not version-dependent labels
such as `Signer #1`, `Signer (minSdkVersion=...)`, or `V2 Signer:`.

The PEM format is defined by ApkSignerTool.printCertificate in AOSP:
https://android.googlesource.com/platform/tools/apksig/+/179f60df00d242f6bb22acf828b0884eac2d5f72/src/apksigner/java/com/android/apksigner/ApkSignerTool.java

Repeated copies of one certificate across signature schemes are allowed.
Distinct certificates, including any distinct source-stamp certificate, are
rejected: these public development APKs do not use rotation or source stamps.
"""

import argparse
import base64
import binascii
import hashlib
import re
import sys


BEGIN = "-----BEGIN CERTIFICATE-----"
END = "-----END CERTIFICATE-----"


def single_signer(output: str, expected: str | None = None) -> str:
    """Return a normalized SHA-256 digest, or fail closed with ValueError."""
    if expected is not None:
        if not re.fullmatch(r"[A-Fa-f0-9]{64}", expected):
            raise ValueError("Invalid expected certificate SHA-256 fingerprint")
        expected = expected.lower()

    certificates = set()
    body = None
    for raw in output.splitlines():
        line = raw.strip()
        if line == BEGIN:
            if body is not None:
                raise ValueError("Nested certificate PEM block")
            body = []
        elif line == END:
            if body is None:
                raise ValueError("Certificate PEM end without a beginning")
            try:
                der = base64.b64decode("".join(body), validate=True)
            except (binascii.Error, ValueError) as error:
                raise ValueError("Invalid certificate PEM base64") from error
            if not der:
                raise ValueError("Empty certificate PEM block")
            certificates.add(hashlib.sha256(der).hexdigest())
            body = None
        elif body is not None:
            body.append(line)
        elif "-----BEGIN CERTIFICATE" in line or "-----END CERTIFICATE" in line:
            raise ValueError("Malformed certificate PEM delimiter")
        elif line.startswith("Number of signers:"):
            if line != "Number of signers: 1":
                raise ValueError("APK does not have exactly one signer: " + line)

    if body is not None:
        raise ValueError("Truncated certificate PEM block")
    if len(certificates) != 1:
        raise ValueError(
            "APK does not have a single certificate identity: "
            + str(sorted(certificates))
        )
    actual = next(iter(certificates))
    if expected is not None and actual != expected:
        raise ValueError("APK does not have the expected public development signer: " + actual)
    return actual


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", help="Required certificate SHA-256 fingerprint")
    args = parser.parse_args(argv)
    output = sys.stdin.read()
    try:
        print(single_signer(output, args.expected))
    except ValueError as error:
        print(str(error), file=sys.stderr)
        print("apksigner public verification output:", file=sys.stderr)
        print(output, end="" if output.endswith("\n") else "\n", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
