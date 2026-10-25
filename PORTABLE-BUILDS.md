# Download Craft 0.9.8

Use [the current release instructions](RELEASE-NOTES.md) for Windows, universal
Mac, Fedora/RPM, Debian-family DEB and AppImage installation and media helpers.

`VERSION` contains the public app version. Cargo, UI, CLI and browser versions
are aligned at 0.9.8.

Build native binaries with Cargo. Package with `scripts/package.py`, or build
Linux glibc-2.28 binaries using cargo-zigbuild and run `scripts/package_linux.py`.
The zig toolchain has no libgcc, and the vendored UnRAR sources ask the
compiler which CPU features the machine has - a question Clang answers with a
reference to libgcc's `__cpu_model`, so the link fails with "undefined symbol:
__cpu_model". Build the Linux packages with

```
CXXFLAGS_x86_64-unknown-linux-gnu="-D__builtin_cpu_supports(x)=0" \
  cargo zigbuild --locked --release --target x86_64-unknown-linux-gnu.2.28
```

"No features" is what a compiler answers for a CPU without them: UnRAR then
uses its own AES and copy routines, which give the same archive a little more
slowly. Mac cross-packaging accepts `--signer <rcodesign>` to sign the app's
resources as well as its universal executable. Native Mac packaging uses
system codesign.

All previous deep-review fixes are included; proposals in REVIEW-0.9.6.1.md are
not implemented. Paused/failed downloads may retain `.dcmeta` for resuming;
successful jobs remove it.
