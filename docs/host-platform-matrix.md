# Host Platform Matrix

Last reviewed: 2026-07-16. This matrix records host operating-system evidence
separately from browser-engine evidence. A passing browser on one Linux
distribution or architecture does not prove another distribution or actual
hardware.

Install the Rustup-owned Wasm target independently of the rest of project setup:

```sh
just install-wasm32-unknown-unknown
```

The command is idempotent. `just setup` runs it after `mise install`, so the
Mise-selected Rustup toolchain owns the target instead of the ambient Homebrew
Rust sysroot.

Container recipes that compile Rust keep Cargo and target caches under
`$RUST_BROWSER_PROOFS_CONTAINER_CACHE_DIR`, or the platform-specific
`~/cache/rust-browser-proofs/container-<architecture>` default. This avoids
large generated writes in the repository and bounds pressure on Docker's
writable container layer.

## Current Coverage

| Host environment | Chrome-family lane | Current status | What remains |
|---|---|---|---|
| macOS 26.5 arm64 | Installed Google Chrome through `just test-consumer-battery-chrome` | **Verified 2026-07-16:** Chrome 150.0.7871.116 with ChromeDriver 150.0.7871.115 passed both generic OPFS tests. [Execution report](/Users/andrew/cache/rust-browser-proofs/browser-tests/1784220236704-test-status.md). | Re-run the explicit browser recipe after browser or driver updates. Safari and iOS proofs are tracked separately. |
| Debian Linux Trixie arm64 container | Distro Chromium and matching `chromium-driver` through `just container-test-consumer-chrome` | **Verified 2026-07-16:** Chromium and ChromeDriver 150.0.7871.124 passed both generic OPFS tests in the rebuilt image. | This is Chromium, not branded Google Chrome. An x86_64 image run would be separate architecture evidence. |
| Ubuntu Linux x86_64 | Gitea Actions uses `ubuntu-latest` for native and Wasm compilation, but installs no browser | **Compile-only; browser unverified** | Add a browser-capable Ubuntu runner or image, then pass the generic Chrome battery there. The CI compile pass is not browser evidence. |
| Manjaro Linux x86_64 | No named runner or image | **Unverified** | Add a maintained Manjaro lane with distro Chromium and a version-matched driver, then pass the generic Chrome battery. Do not infer it from Debian. |
| Windows x86_64 | No named Windows runner; the current `justfile` and ChromeDriver bootstrap are POSIX-only | **Blocked on runner support** | Add a Windows-compatible command path and ChromeDriver location, then run the generic Chrome battery on a real Windows runner. WSL is Linux evidence, not Windows browser evidence. |
| Raspberry Pi OS arm64 | No Raspberry Pi hardware runner; the ChromeDriver bootstrap does not support Linux arm64 | **Blocked on real hardware** | Run on an actual Pi with Raspberry Pi OS, distro Chromium, and its matching driver. Debian arm64 in Docker/QEMU is useful preflight evidence but is not a Raspberry Pi proof. |

## Acceptance Gate For A New Host

A host row becomes verified only when all of these succeed on that exact host:

1. `just install-wasm32-unknown-unknown`
2. `rustup target list --installed` includes `wasm32-unknown-unknown`
3. The installed Chrome or Chromium and driver major versions match
4. `just test-consumer-battery-chrome` exits successfully after the browser
   reports both generic OPFS battery tests as passed

PageDB durability is a separate, stronger gate. A host that also claims PageDB
support must pass `just test-chrome`; the generic consumer battery cannot be
promoted into a PageDB durability result.
