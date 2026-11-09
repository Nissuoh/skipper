// SPDX-License-Identifier: MIT
//! The library of the `xtask` package: the guard both binaries start with, the model catalogue builder [`catalog`], the
//! installer step [`installer`] and the update pack of the release step [`pack`].
//!
//! `xtask` (the push gate's scanner and the project's checks) keeps its own modules in `main.rs` and uses only
//! [`guard`], [`catalog`], [`installer`] and [`pack`] from here, so it links no network code.

pub mod catalog;
/// The network half of the catalogue job: fetch, publish and the docs fetch. It reaches `skipper-net`, so it exists only
/// with the feature `catalog-ci` (the second binary `xtask-catalog`); the push gate's `xtask.exe` links no network crate.
#[cfg(feature = "catalog-ci")]
pub mod catalog_net;
pub mod guard;
/// The installer step of Stage 1: the per-user installer is the program itself, shipped as one more file of
/// `dist/stage1/`, `skipper-setup-<version>.exe`, a byte copy of `skipper.exe`; this module makes the copy, lists it in
/// `SHA256SUMS.txt`, checks the folder and scans the installer's sources for what an installer must never touch. Plain code over
/// `std` and the `pack` module, so the push gate's `xtask.exe` may use it, and its test (`tests/installer.rs`) uses it by name.
pub mod installer;
/// The update pack of Stage 1: the format `SKPK1` that `cargo xtask dist --stage 1` writes, the file set that
/// ships, the checksum file and the assembly of `dist/stage1/`. Plain code over `std`, `sha2` and `toml` with no product crate
/// behind it, so the push gate's `xtask.exe` may use it. It lives in the library so that its
/// test (`tests/pack.rs`) uses it by name, as the tests of `catalog` do.
pub mod pack;
