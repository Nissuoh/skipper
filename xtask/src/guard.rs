// SPDX-License-Identifier: MIT
//! The guard of `xtask` (plan Task 2 Step 7; G4, NR-17, ruling PR-6).
//!
//! `xtask` refuses to run unless it was started through `skipper-cap`: the current process must sit in a job
//! object (`IsProcessInJob`) and either its token is an app-container token (`TokenIsAppContainer`) or, as for
//! `skipper-cap run --no-box`, the user profile holds no Claude sign-in (NB-10). The skeleton's `main` calls
//! `require_capped` first and, on `Err`, prints `xtask: run me through skipper-cap (G4)` and exits with code 2. A
//! direct `cargo` call still builds `xtask` once without the cap, which G5 forbids; the guard only stops the run
//! itself.

use std::path::Path;

/// The decision, kept apart from the Windows calls so every platform can test it; `sign_in` names the Claude
/// sign-in found in the profile, if any.
fn verdict(in_job: bool, app_container: bool, sign_in: Option<&str>) -> Result<(), String> {
    match (in_job, app_container, sign_in) {
        (false, true, _) => Err("the process is not inside a job object".to_string()),
        (false, false, _) => Err("the process is neither inside a job object nor an app container".to_string()),
        (true, true, _) | (true, false, None) => Ok(()),
        (true, false, Some(name)) => Err(format!(
            "the process runs outside the build box and the profile holds {name}; without the box only on a machine \
             without a Claude sign-in (NB-10)"
        )),
    }
}

/// The Claude sign-in in `profile`, if there is one: the same check as `skipper-cap`'s `--no-box` (NB-5).
fn sign_in(profile: &Path) -> Option<&'static str> {
    [".claude", ".claude.json"].into_iter().find(|name| profile.join(name).exists())
}

/// `Ok` only when the process is in a job object and runs in the box or beside no Claude sign-in.
#[cfg(windows)]
pub fn require_capped() -> Result<(), String> {
    let in_job = process_in_job()?;
    let app_container = token_is_app_container()?;
    let profile = std::env::var_os("USERPROFILE").ok_or("USERPROFILE is not set")?;
    verdict(in_job, app_container, sign_in(Path::new(&profile)))
}

/// The cap and the build box exist on Windows only.
#[cfg(not(windows))]
pub fn require_capped() -> Result<(), String> {
    Err("skipper-cap and its build box exist on Windows only".to_string())
}

/// The decision of [`require_capped_or_ci`], kept apart from the environment so every platform can test it (R13).
///
/// `capped` is the answer of [`require_capped`]. When it is `Ok`, the process runs as it always did. When it is `Err`, the
/// process passes only as a step of a GitHub-hosted runner: `github_actions` is `Some("true")`, `runner_env` is
/// `Some("github-hosted")` (a self-hosted runner is a machine of ours or of someone else, with sign-ins on it), and
/// `sign_in` finds no Claude sign-in in the profile. The environment variables can be set by anyone, so the last rule is
/// the one that keeps the main PC out: it holds a sign-in and is refused whatever the variables say.
fn ci_verdict(
    capped: Result<(), String>,
    github_actions: Option<&str>,
    runner_env: Option<&str>,
    sign_in: Option<&str>,
) -> Result<(), String> {
    let Err(why) = capped else { return Ok(()) };
    if github_actions != Some("true") {
        return Err(why);
    }
    if runner_env != Some("github-hosted") {
        return Err(format!("{why}; a CI run passes only on a GitHub-hosted runner"));
    }
    match sign_in {
        None => Ok(()),
        Some(name) => Err(format!("{why}; the profile holds {name}, so this is no clean CI runner")),
    }
}

/// [`require_capped`], or a step of a GitHub-hosted runner (ruling R13, a reviewed exception to G4): the catalogue job
/// runs there without `skipper-cap`, because the runner is a fresh virtual machine that holds no sign-in, and it has no
/// job object to join. Only the second binary `xtask-catalog` calls this; `xtask` keeps [`require_capped`].
#[cfg(windows)]
pub fn require_capped_or_ci() -> Result<(), String> {
    let capped = require_capped();
    let Err(why) = &capped else { return Ok(()) };
    let var = |name: &str| std::env::var(name).ok();
    // A run id is set on every real run; a bare `GITHUB_ACTIONS=true` typed into a shell is not enough.
    let github_actions = if var("GITHUB_RUN_ID").is_some_and(|id| !id.is_empty()) { var("GITHUB_ACTIONS") } else { None };
    let runner_env = var("RUNNER_ENVIRONMENT");
    let Some(profile) = std::env::var_os("USERPROFILE") else {
        return Err(format!("{why}; USERPROFILE is not set"));
    };
    ci_verdict(capped.clone(), github_actions.as_deref(), runner_env.as_deref(), sign_in(Path::new(&profile)))
}

/// The cap, the build box and the hosted runner of the job exist on Windows only.
#[cfg(not(windows))]
pub fn require_capped_or_ci() -> Result<(), String> {
    Err("skipper-cap and the catalogue job's runner exist on Windows only".to_string())
}

#[cfg(windows)]
fn process_in_job() -> Result<bool, String> {
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // A Win32 BOOL is an i32; spelled out because windows-sys moved its `BOOL` alias between versions.
    let mut result: i32 = 0;
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing; a null job handle asks "any job";
    // `result` points at a live local for the whole call.
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut result) };
    if ok == 0 {
        return Err(format!("IsProcessInJob failed: {}", std::io::Error::last_os_error()));
    }
    Ok(result != 0)
}

#[cfg(windows)]
fn token_is_app_container() -> Result<bool, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenIsAppContainer, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle of the current process is valid; `token` points at a live local.
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if opened == 0 {
        return Err(format!("OpenProcessToken failed: {}", std::io::Error::last_os_error()));
    }
    let mut flag: u32 = 0;
    let mut returned: u32 = 0;
    // SAFETY: `token` was opened with TOKEN_QUERY; the buffer is a live u32 and its size is passed exactly.
    let got = unsafe {
        GetTokenInformation(
            token,
            TokenIsAppContainer,
            (&mut flag as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )
    };
    let last = std::io::Error::last_os_error();
    // SAFETY: `token` is a handle this function opened and has not closed yet.
    unsafe {
        CloseHandle(token);
    }
    if got == 0 {
        return Err(format!("GetTokenInformation failed: {last}"));
    }
    Ok(flag != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_and_app_container_pass() {
        assert_eq!(verdict(true, true, None), Ok(()));
    }

    #[test]
    fn box_passes_beside_a_sign_in() {
        // The main PC holds a sign-in and builds in the box.
        assert_eq!(verdict(true, true, Some(".claude")), Ok(()));
    }

    #[test]
    fn missing_job_is_refused() {
        assert!(verdict(false, true, None).is_err());
    }

    #[test]
    fn job_without_box_passes_where_no_sign_in_lies() {
        // The build machine runs every step with `skipper-cap run --no-box` (NB-5, NB-10).
        assert_eq!(verdict(true, false, None), Ok(()));
    }

    #[test]
    fn job_without_box_is_refused_beside_a_sign_in() {
        let refusal = verdict(true, false, Some(".claude.json")).unwrap_err();
        assert!(refusal.contains(".claude.json"), "{refusal}");
    }

    #[test]
    fn neither_is_refused() {
        assert!(verdict(false, false, None).is_err());
    }

    #[test]
    fn sign_in_is_found_as_folder_or_file() {
        let base = std::env::temp_dir().join(format!("xtask-guard-{}", std::process::id()));
        let clean = base.join("clean");
        let folder = base.join("folder");
        let file = base.join("file");
        std::fs::create_dir_all(&clean).unwrap();
        std::fs::create_dir_all(folder.join(".claude")).unwrap();
        std::fs::create_dir_all(&file).unwrap();
        std::fs::write(file.join(".claude.json"), b"{}").unwrap();
        assert_eq!(sign_in(&clean), None);
        assert_eq!(sign_in(&folder), Some(".claude"));
        assert_eq!(sign_in(&file), Some(".claude.json"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_is_refused() {
        assert!(require_capped().is_err());
    }

    #[test]
    fn ci_passes_only_on_a_github_hosted_runner_without_a_sign_in() {
        let off = Err("no job".to_string());
        assert!(ci_verdict(off.clone(), Some("true"), Some("github-hosted"), None).is_ok());
        assert!(ci_verdict(off.clone(), Some("true"), Some("self-hosted"), None).is_err());
        assert!(ci_verdict(off.clone(), Some("true"), Some("github-hosted"), Some(".claude")).is_err());
        assert!(ci_verdict(off, None, None, None).is_err());
        assert!(ci_verdict(Ok(()), None, None, None).is_ok());
    }

    #[test]
    fn ci_needs_both_variables_and_names_the_first_refusal() {
        let off = Err("no job".to_string());
        assert_eq!(ci_verdict(off.clone(), Some("false"), Some("github-hosted"), None), off);
        assert!(ci_verdict(off.clone(), Some("true"), None, None).is_err());
        assert!(ci_verdict(off, Some("true"), Some("github-hosted"), Some(".claude.json")).unwrap_err().contains(".claude.json"));
    }

    #[test]
    fn a_capped_process_passes_whatever_the_variables_say() {
        assert!(ci_verdict(Ok(()), Some("false"), Some("self-hosted"), Some(".claude")).is_ok());
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_ci_is_refused_too() {
        assert!(require_capped_or_ci().is_err());
    }
}
