# Setup and Recovery

The repository pins its developer tools in `.mise.toml`, its hosted PageDB
source revision in `harness/Cargo.toml`, and its container bases and security
scanner by digest. Browser and Raspberry Pi model images are reproducible build
outputs, not source artifacts.

## Clean-machine bootstrap

The host needs Git, Mise, Rustup, and a running Docker-compatible engine. Mise's
Rust backend can provision Rustup on a new host. The setup does not require host
browsers, QEMU, an Android phone, or a Raspberry Pi. Every Just recipe prepends
Rustup's proxy directory and sets `RUSTUP_TOOLCHAIN=1.95.0`, while Mise tasks and
Lefthook commands invoke the Mise-pinned Just binary explicitly. An earlier
Homebrew Rust, Just, or Lefthook therefore cannot silently replace repository
tools.

```sh
git clone ssh://git@git.telpher.stream:2222/awb/rust-browser-proofs.git
cd rust-browser-proofs
mise trust .mise.toml
mise install
just setup
just setup-status
```

`just setup` installs the pinned Mise tools, adds Rustup's
`wasm32-unknown-unknown` target, and installs the checked-in Lefthook policy.
`just setup-status` is read-only: it verifies the toolchain, Docker daemon, Wasm
target, immutable dependency pins, hosted-CI contract checks, and external state
locations. When a clean Gitea Actions image lacks Rustup, the workflow downloads
the official Rustup `1.29.0` binary and verifies its pinned SHA-256 before
execution. It never executes the mutable `sh.rustup.rs` network installer.

## External state

Raspberry Pi model firmware and logs default to:

```text
~/.volumes/rust-browser-proofs/raspi4b-model
```

Set `RUST_BROWSER_PROOFS_RASPI4_VOLUME_DIR` to another absolute host path when
the volume belongs on a larger disk. The QEMU runtime receives this directory
through a bounded mount; it does not write large firmware files into the Git
checkout.

Markdown evidence and its `report-cache.sqlite3` mirror default to:

```text
~/cache/rust-browser-proofs/browser-tests
```

Set `RUST_BROWSER_PROOFS_REPORT_DIR` to relocate both. The SQLite file is an
index and durable copy of generated reports, not an input to the proof. Back up
the report directory when evidence retention matters. Deleting it loses local
history but does not affect source or future test correctness.

## Recovery

Removing a locally built Docker image is safe; rerun `just container-build` or
`just raspi4b-model-container-build`. Removing the Raspberry Pi external volume
is also safe when its logs are no longer needed; `just prepare-raspi4b-model`
redownloads the pinned firmware revision and verifies both SHA-256 checksums
before use.

If setup drifts, run these commands in order:

```sh
mise install
just install-wasm32-unknown-unknown
mise exec -- lefthook install
just setup-status
just verify
just container-verify
```

Do not copy a local Cargo lockfile between the vendor-patched checkout and CI.
The harness lockfile is intentionally ignored because local development may use
the ignored `.cargo/config.toml` PageDB path patch, while hosted CI resolves the
immutable Gitea revision.

Home Assistant or physical Raspberry Pi SSH access is optional discovery and
hardware validation only. It is not part of clean-machine setup, no credential
belongs in this repository, and the standard model proof remains the isolated
containerized QEMU lane.
