# Raspberry Pi 4 Board-Model Simulation

This lane boots a checksum-pinned official Raspberry Pi kernel under QEMU's
`raspi4b` machine model. It is a small, deterministic board-model smoke test,
not a browser VM. The default path runs QEMU in a dedicated container, so the
host needs Docker but does not need QEMU, Rust, a browser, or a cross compiler.
Firmware and logs stay outside the checkout under
`~/.volumes/rust-browser-proofs/raspi4b-model` by default.

## Commands

```sh
just raspi4b-model-status
just prepare-raspi4b-model
just test-raspi4b-model
```

The image uses a digest-pinned Debian base and installs only QEMU, TLS
certificates, Curl, and checksum utilities. Runtime containers are read-only,
drop every capability, enable `no-new-privileges`, cap memory/CPU/PIDs, and use
an unprivileged host UID. Firmware preparation has network access only for the
pinned HTTPS downloads; the QEMU boot container runs with `--network none`.

`prepare-raspi4b-model` downloads only `kernel8.img` and
`bcm2711-rpi-4-b.dtb` from a pinned commit in the official Raspberry Pi
firmware repository. Both files must match the SHA-256 values embedded in the
runner before QEMU can start. Override the external storage location with an
absolute path:

```sh
RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR=/absolute/external/path \
  just test-raspi4b-model
```

The runner rejects repository-local artifact paths. It runs QEMU at nice level
15 by default, polls the console for required boot markers, and terminates only
the QEMU PID it started. Adjust the bounded boot window or niceness with
`RUST_BROWSER_PROOFS_RASPI4_BOOT_TIMEOUT_SECONDS` and
`RUST_BROWSER_PROOFS_RASPI4_NICE`.

The default Markdown result is written under
`$RUST_BROWSER_PROOFS_REPORT_DIR`, `$XDG_CACHE_HOME/rust-browser-proofs`, or
`$HOME/cache/rust-browser-proofs`. `just test-raspi4b-model` mirrors that report
into the adjacent `report-cache.sqlite3` using the existing native runner.

Machines that already provide a compatible `qemu-system-aarch64` can use the
explicit `just test-raspi4b-model-host` fast path. That command does not install
QEMU and has the same firmware, report, timeout, and proof-boundary checks.

## Passing Evidence

The smoke gate passes only after the emulated serial console contains all three
markers:

- `Booting Linux on physical CPU`
- `Linux version`
- `Machine model: Raspberry Pi 4 Model B`

The QEMU log remains under the external volume for inspection. No guest disk is
attached, and the process stops immediately after the markers are observed.

## Proof Boundary

The result proves that the pinned Raspberry Pi kernel reaches Linux under the
QEMU Raspberry Pi 4B board model. It does not prove Raspberry Pi OS userland,
browser behavior, OPFS, physical Raspberry Pi hardware, VideoCore/GPU behavior,
SD-card durability, thermal behavior, or network behavior.

QEMU does not currently implement the Raspberry Pi 4 PCIe root port or GENET
Ethernet controller. A browser-capable simulated ARM64 Linux lane should use
QEMU `virt` or the existing ARM64 container, but that evidence must remain
separate from the `raspi4b` board-model result.
