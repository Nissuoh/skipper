// SPDX-License-Identifier: MIT
//! `xtask-catalog` - the entry of the daily catalogue job (MODEL-15, CAT-CI; R13): `fetch`, `build`, `publish`, and the
//! docs fetch `cc-fetch`. Built only with the feature `catalog-ci`. It starts with
//! `require_capped_or_ci`: inside `skipper-cap` as every other tool, or as a step of a GitHub-hosted runner without a
//! sign-in, where no job object exists to join.

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(why) = xtask::guard::require_capped_or_ci() {
        eprintln!("xtask-catalog: run me through skipper-cap, or on a GitHub-hosted runner (G4, R13)");
        eprintln!("xtask-catalog: {why}");
        return ExitCode::from(2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(xtask::catalog_net::run(&args))
}
