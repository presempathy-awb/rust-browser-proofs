# Host Platform Matrix

Last reviewed: 2026-07-17. This matrix records host operating-system evidence
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
| Windows 11 25H2 arm64 (local UTM guest) | Native PageDB lock proof: `cargo test -p pagedb-opfs-harness --test windows_cross_process_lock`; non-Windows hosts can type-check the code with `just check-windows-lock`, which currently uses Rustup `stable`'s installed **x86_64 GNU** target only | **Native lock verified 2026-07-17; browser unverified:** Rust 1.97.1 on `aarch64-pc-windows-msvc`, MSVC v143 `HostARM64\\arm64\\link.exe`, and the local PageDB bugfix source passed all three cross-process tests. The first build completed in 8m06s; the test binary completed in 0.35s. | Add a Windows-compatible browser command path and ChromeDriver location before claiming generic OPFS browser coverage. The x86_64 host-side compile check remains source compatibility evidence, not ARM64 runtime evidence. WSL is Linux evidence, not Windows browser evidence. |
| Home Assistant OS 18.1 on Raspberry Pi 4 Model B Rev 1.5 | Read-only inventory through the existing Terminal & SSH app container on port 2222 | **Physical ARM64 hardware identified 2026-07-17; browser unverified:** four Cortex-A72 cores, 3.7 GiB RAM, `aarch64`, and the HAOS `6.18.34-haos-raspi` kernel were observed. The Alpine app container has no Chromium, Firefox, Rust, Node, or Wasm toolchain installed. | Do not install a browser toolchain into the production Home Assistant appliance. A purpose-built, resource-bounded app could provide physical-hardware evidence later, but it would remain HAOS/container evidence rather than Raspberry Pi OS evidence. |
| QEMU raspi4b board model | `just test-raspi4b-model` runs containerized QEMU and boots a checksum-pinned official Raspberry Pi kernel and Pi 4 DTB with no writable disk image or network device | **Board-model boot evidence:** the gate requires Linux CPU boot, kernel-version, and `Machine model: Raspberry Pi 4 Model B` markers before stopping the exact QEMU process. The Markdown result is mirrored into the report SQLite cache. Host QEMU is optional. | This is not a host or browser result. QEMU's model lacks the Pi 4 PCIe root port and GENET Ethernet controller, so it cannot establish Raspberry Pi OS userland, OPFS, browser, network, GPU, storage-durability, thermal, or physical-hardware claims. |
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

The QEMU `raspi4b` row is a board-model smoke gate and never satisfies this host
acceptance gate. See [Raspberry Pi 4 board-model simulation](raspi4b-model.md).
