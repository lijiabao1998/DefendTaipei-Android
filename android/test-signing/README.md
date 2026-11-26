# PUBLIC AOSP development signing fixture

This directory deliberately contains an already-public development key from the
Android Open Source Project. It is not a newly generated project secret and must
not be used as a production or Google Play release identity. Anyone can obtain
this key from AOSP and produce a package with the same signing certificate.

The Android `debug` build uses `aosp-public-testkey.p12` for both ARM64 and x86_64,
so independent local/CI builds have the same signer. The `release` build has no
signing configuration added by this change.

Upstream: Android Open Source Project, `platform/build`, commit
`045a3d6a3e359633a14853a5a5e1e4f2a11cbdae`:

- [Original testkey.pk8](https://github.com/aosp-mirror/platform_build/blob/045a3d6a3e359633a14853a5a5e1e4f2a11cbdae/target/product/security/testkey.pk8)
- [Original testkey.x509.pem](https://github.com/aosp-mirror/platform_build/blob/045a3d6a3e359633a14853a5a5e1e4f2a11cbdae/target/product/security/testkey.x509.pem)
- [Upstream development-only guidance and conversion example](https://github.com/aosp-mirror/platform_build/blob/045a3d6a3e359633a14853a5a5e1e4f2a11cbdae/target/product/security/README)

The two original files are byte-for-byte copies; `provenance.json` records their
SHA-256 and upstream Git blob IDs. The PKCS12 file contains that same key and
certificate, with alias `androiddebugkey` and the intentionally public store/key
password `android`. The certificate is valid until **2035-07-17 01:33:46 UTC**.

Expected certificate SHA-256, over its DER encoding:

```text
a40da80a59d170caa950cf15c18c454d47a39b26989d8b640ecd745ba71bf5dc
```

From the repository root, verify a built artifact with:

```sh
bash android/test-signing/verify-apk.sh path/to/app-debug.apk
```

This invokes Android SDK `apksigner`, verifies the APK signatures and then checks
for exactly this one certificate. Set `APKSIGNER` to its executable path if it is
not on `PATH` or under `ANDROID_SDK_ROOT` / `ANDROID_HOME`.

See [test-signing design](../../docs/TEST-SIGNING.md) and the
[save-preserving update helper](../../docs/ANDROID-TEST-UPDATES.md).
