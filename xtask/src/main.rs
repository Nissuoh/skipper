// SPDX-License-Identifier: MIT
//! `cargo xtask` - the project's own checks and build tasks: `pin-check`, `leak-scan`, `pe-check`, `deps-check` and
//! `dist`, `stage0-check`, `provider-name-check`, `lang-lint`, `template-lint`, `source-lint`, `skill-lint` and `hub-lint`
//! (net-lint, the vault folder in cred-lint, secret-lint; a plain `source-lint` run includes it) and `supply`
//! (cargo-deny, cargo-audit, cargo-vet, offline, fail closed) and `stage1-check` (the signed Stage 1 checklist, its
//! answers and the probe facts) and `installer` (the setup file of the release folder and its check). The catalogue
//! job's entry is the other binary, `xtask-catalog`; this program links none of it.

use std::process::ExitCode;

mod deps;
mod dist;
mod hub_lint;
mod lang_lint;
mod leak;
mod pe;
mod pins;
mod provider_names;
mod rust_tokens;
mod skill_lint;
mod source_lint;
mod stage0;
mod stage1;
mod supply;
mod template_lint;

fn main() -> ExitCode {
    // The guard lives in the library, because `xtask-catalog` needs it too.
    if let Err(why) = xtask::guard::require_capped() {
        eprintln!("xtask: run me through skipper-cap (G4)");
        eprintln!("xtask: {why}");
        return ExitCode::from(2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.split_first() {
        Some((command, rest)) if command == "pin-check" => pins::run_cli(rest),
        Some((command, rest)) if command == "leak-scan" => leak::run_cli(rest),
        Some((command, rest)) if command == "pe-check" => pe::run_cli(rest),
        Some((command, rest)) if command == "deps-check" => deps::run_cli(rest),
        Some((command, rest)) if command == "dist" => dist::run_cli(rest),
        // The per-user installer of the release (RL-36, Task 43): the setup file of dist/stage1, its check and the scan of its sources.
        Some((command, rest)) if command == "installer" => xtask::installer::run_cli(rest),
        Some((command, rest)) if command == "stage0-check" => stage0::run_cli(rest),
        Some((command, rest)) if command == "stage1-check" => stage1::run_cli(rest),
        Some((command, rest)) if command == "provider-name-check" => provider_names::run_cli(rest),
        Some((command, rest)) if command == "lang-lint" => lang_lint::run_cli(rest),
        Some((command, rest)) if command == "template-lint" => template_lint::run_cli(rest),
        Some((command, rest)) if command == "source-lint" => {
            let own = source_lint::run_cli(rest);
            // The hub lints (HUB-13) ride along with the plain run, so the gate needs no new step.
            if rest.is_empty() { own.max(hub_lint::run_cli(rest)) } else { own }
        }
        Some((command, rest)) if command == "hub-lint" => hub_lint::run_cli(rest),
        Some((command, rest)) if command == "skill-lint" => skill_lint::run_cli(rest),
        Some((command, rest)) if command == "supply" => supply::run_cli(rest),
        _ => {
            eprintln!("usage: cargo xtask (pin-check | leak-scan ... | pe-check ... | deps-check | dist ... | installer ... | stage0-check ... | stage1-check ... | provider-name-check | lang-lint | template-lint | source-lint | hub-lint | skill-lint | supply ...)");
            2
        }
    };
    ExitCode::from(code)
}
