# Stable signing for development APKs

Android debug builds now use one intentionally public AOSP test signing identity,
checked into `android/test-signing`. This fixes a concrete update problem:
independent CI jobs previously generated their own default debug keystores, so
successive APK downloads could have different signing certificates even when the
application ID was unchanged.

Android accepts an update only when the application ID matches, the signing
certificate matches (or has valid rotation proof), and the version code does not
decrease. A different certificate normally requires uninstalling the old app,
which removes its private app data. These are platform rules documented in
[How app updates work](https://developer.android.com/google/play/app-updates).

## Development identity and provenance

The source is AOSP's existing generic `testkey`, not a project-generated private
production key. Original material was retrieved from
[`aosp-mirror/platform_build` at commit `045a3d6a3e359633a14853a5a5e1e4f2a11cbdae`](https://github.com/aosp-mirror/platform_build/tree/045a3d6a3e359633a14853a5a5e1e4f2a11cbdae/target/product/security).
Its PKCS8 key and PEM certificate are retained verbatim in this repository so the
provenance can be checked independently. Their Git blob hashes match upstream.
`aosp-public-testkey.p12` converts that same material into the PKCS12 format used
by the Android Gradle Plugin; it does not generate another key or certificate.

| Property | Value |
| --- | --- |
| Keystore | `android/test-signing/aosp-public-testkey.p12` |
| Store type | `PKCS12` |
| Alias | `androiddebugkey` |
| Store and key password | `android` — deliberately public |
| RSA public key size | 2048 bits |
| Certificate validity (UTC) | 2008-02-29 01:33:46 through 2035-07-17 01:33:46 |
| Certificate SHA-256 (DER) | `a40da80a59d170caa950cf15c18c454d47a39b26989d8b640ecd745ba71bf5dc` |
| Original PKCS8 SHA-256 | `495675d32e89a149d5abe191f4e9c0e218b9068714e9b53a7c91e164a0741a23` |
| Original PEM file SHA-256 | `a4384ba815b9499a5ce349b4e33c1755278873fe2eac150a068823f526e6dbde` |
| Committed PKCS12 SHA-256 | `fe4a98360b31944e4b151ac6acbe2591812d29a8cadc57aff87994303fad8c4d` |

The PKCS12 password is a format compatibility setting, not a security boundary.
AOSP explicitly identifies these test keys as public development material in its
[signing guide](https://source.android.com/docs/core/ota/sign_builds) and
[upstream README](https://github.com/aosp-mirror/platform_build/blob/045a3d6a3e359633a14853a5a5e1e4f2a11cbdae/target/product/security/README).
Anyone can sign a different APK with this key. The certificate therefore provides
stable development-update compatibility, not proof that an APK came from this
project. Use the project's trusted CI/download source and published artifact
SHA-256 when obtaining a test build.

`android/app/build.gradle` applies this fixture **only to `debug`**. Both ABI
configurations use it. No release signing key is configured or embedded. A future
production/Google Play release needs a separately managed private signing
identity; this public fixture must not serve that purpose. The legacy AOSP
certificate's self-signature uses SHA1withRSA, which Java reports as weak; the
fixture is retained for development compatibility, not selected as a modern
production certificate.

## Verification

Run this against each built debug APK, including both the ARM64 phone artifact and
the x86_64 emulator artifact:

```sh
bash android/test-signing/verify-apk.sh path/to/app-debug.apk
```

The gate runs Android SDK `apksigner verify --verbose --print-certs`, requires
verification success and exactly one signer with the committed certificate
fingerprint. It examines the actual artifact, so an accidental return to a
runner's generated debug keystore fails the gate. It does not sign or modify the
APK. `APKSIGNER`, `PATH`, `ANDROID_SDK_ROOT` and `ANDROID_HOME` are supported lookup
locations.

To inspect the checked-in fixture with Java:

```sh
keytool -list -v -storetype PKCS12 \
  -keystore android/test-signing/aosp-public-testkey.p12 \
  -storepass android -alias androiddebugkey
```

`provenance.json` records the exact source revision, source hashes, converted
keystore hash, certificate fingerprint and validity dates. The key/certificate
pair and its self-signature were checked before conversion, and the resulting
keystore was read successfully by Java. APK build/signature and emulator-update
results belong to the delivered revision's CI evidence.

## Existing installations and saved games

This change cannot recreate the private debug key from an earlier ephemeral CI
runner. An old APK signed by that runner cannot be directly updated to the new
identity merely by keeping its application ID or changing its version number.
The first switch to this stable identity needs a verified backup and migration.
Use [the Android test-update procedure](ANDROID-TEST-UPDATES.md), implemented by
`scripts/update-android-test-build.py`. It uses normal replacement when possible
and provides an explicit migration path for a verified certificate mismatch.

After that one-time switch, subsequent debug builds using this fixture can follow
the normal Android update path, subject to the platform's package/version rules.
The helper's backup and restoration verification remain the source of truth for
save retention; a successful signer check alone does not prove a save was copied.

## Reproducing the conversion

The upstream README documents the same PKCS8-to-PKCS12 conversion. To reproduce it
without overwriting the committed fixture:

```sh
public_test_signing_tmp=$(mktemp -d)
openssl pkcs8 -inform DER -nocrypt \
  -in android/test-signing/testkey.pk8 \
  -out "$public_test_signing_tmp/testkey.pem"
openssl pkcs12 -export \
  -in android/test-signing/testkey.x509.pem \
  -inkey "$public_test_signing_tmp/testkey.pem" \
  -out "$public_test_signing_tmp/reproduced.p12" \
  -name androiddebugkey -passout pass:android
keytool -list -storetype PKCS12 \
  -keystore "$public_test_signing_tmp/reproduced.p12" \
  -storepass android -alias androiddebugkey
```

PKCS12 uses random encryption salts, so a newly converted file can have a different
file hash while containing the same key and certificate. Compare the certificate
fingerprint for signing identity. CI consumes the committed fixture rather than
regenerating it on each job.
