// SPDX-License-Identifier: MIT
//! `cargo xtask provider-name-check` (plan Task 4; G-08, S1G-15): `skipper-core` names no provider.
//!
//! The check reads every `.rs` file below `crates/skipper-core/src`, splits it into tokens (`rust_tokens`, shared with
//! `lang-lint`), and reports each identifier and each string literal that holds `claude` or `anthropic` in any letter
//! case. Comments (`//`, `///`, `//!`, and nested `/* */`) are not tokens, and an item that carries a test-only
//! `#[cfg(..)]` (`#[cfg(test)]`, `#[cfg(all(test, ..))]`; a test module, usually; also a field, an enum variant, a
//! match arm, a parameter or a list element, up to its own end) is skipped whole: tests may name a provider, the code
//! that ships may not. A `cfg` that can hold in a normal build (`any(test, ..)`, `not(test)`) is judged, and so is
//! every file as a whole: this check is the only enforcement of S1G-15, so it never skips more than an item.
//!
//! Ruling (plan Task 4): three files are exempt, and only those. `door.rs` holds the credential deny-list of ACC-3
//! and must not depend on provider wiring; `canary.rs` and `sim.rs` are the safety simulation. Safety beats purity.
//! Cost if wrong: a stricter reading of G-08 moves those constants into the provider and passes them in.

use std::fs;
use std::path::Path;

use crate::rust_tokens::{skip_test_item, tokenize, Token};

/// The folder the check reads, relative to the workspace root (`cargo xtask` runs there).
const SOURCE_DIR: &str = "crates/skipper-core/src";

/// The files, relative to [`SOURCE_DIR`] with `/` as separator, that may name a provider.
const EXEMPT: [&str; 3] = ["door.rs", "canary.rs", "sim.rs"];

/// What may not appear in an identifier or a string literal, compared in lower case.
const NAMES: [&str; 2] = ["claude", "anthropic"];

/// One identifier or string literal that holds a provider name.
#[derive(Clone, PartialEq, Debug)]
pub struct Finding {
    /// The line of the token, counted from 1.
    pub line: usize,
    /// The identifier, or the text of the literal as written between its quotes.
    pub text: String,
}

fn names_a_provider(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    NAMES.iter().any(|name| lower.contains(name))
}

/// Every identifier and string literal of `src` that holds a provider name, outside comments and outside items
/// that carry a test-only `#[cfg(..)]`.
pub fn findings(src: &str) -> Vec<Finding> {
    let tokens = tokenize(src);
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = skip_test_item(&tokens, i) {
            i = end;
            continue;
        }
        if let Token::Ident(text, line) | Token::Str(text, line) = &tokens[i] {
            if names_a_provider(text) {
                out.push(Finding { line: *line, text: text.clone() });
            }
        }
        i += 1;
    }
    out
}

/// The findings of a source tree, as report lines `<file>:<line>: `<text>``. `files` are (path below
/// [`SOURCE_DIR`] with `/` as separator, text); the files of [`EXEMPT`] are not read.
pub fn check_tree(files: &[(String, String)]) -> Vec<String> {
    let mut report = Vec::new();
    for (path, text) in files {
        if EXEMPT.contains(&path.as_str()) {
            continue;
        }
        for finding in findings(text) {
            report.push(format!("{path}:{}: `{}`", finding.line, finding.text));
        }
    }
    report
}

/// Every `.rs` file below `dir`, as (path relative to `dir` with `/` as separator, text), in name order.
fn read_tree(dir: &Path) -> Result<Vec<(String, String)>, String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), String> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else if path.extension().is_some_and(|x| x == "rs") {
                let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                let relative = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push((relative, text));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    Ok(files)
}

pub fn run_cli(args: &[String]) -> u8 {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask provider-name-check");
        return 2;
    }
    let files = match read_tree(Path::new(SOURCE_DIR)) {
        Ok(files) if !files.is_empty() => files,
        Ok(_) => {
            println!("provider-name-check: no source file below {SOURCE_DIR} (run it from the workspace root)");
            return 1;
        }
        Err(why) => {
            println!("provider-name-check: {why}");
            return 1;
        }
    };
    let report = check_tree(&files);
    if report.is_empty() {
        println!("provider-name-check: ok");
        0
    } else {
        for line in &report {
            println!("provider-name-check: {SOURCE_DIR}/{line}");
        }
        println!("provider-name-check: {} provider name(s) in skipper-core (G-08, S1G-15)", report.len());
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-file source tree: the path below `crates/skipper-core/src` and the file's text.
    fn tree_with(name: &str, text: &str) -> Vec<(String, String)> {
        vec![(name.to_owned(), text.to_owned())]
    }

    #[test]
    fn provider_name_in_identifier_or_literal_is_found() {
        assert_eq!(findings("fn find_claude() {}").len(), 1);
        assert_eq!(findings("let s = \"Claude\";").len(), 1);
    }

    #[test]
    fn provider_name_in_comment_is_not_a_finding() {
        assert!(findings("// runs Claude Code\n/* claude */ fn f() {}").is_empty());
    }

    #[test]
    fn exempt_files_are_skipped() {
        assert!(check_tree(&tree_with("door.rs", "\".credentials.json\" \"Anthropic\"")).is_empty());
    }

    #[test]
    fn only_the_three_files_at_the_top_are_exempt() {
        for name in ["door.rs", "canary.rs", "sim.rs"] {
            assert!(check_tree(&tree_with(name, "const A: &str = \"claude\";")).is_empty(), "{name}");
        }
        for name in ["launch.rs", "session/door.rs", "term/sim.rs"] {
            assert_eq!(check_tree(&tree_with(name, "const A: &str = \"claude\";")).len(), 1, "{name}");
        }
    }

    #[test]
    fn both_names_are_found_in_any_letter_case() {
        for text in ["CLAUDE", "Claude", "claude", "ANTHROPIC", "Anthropic", "anthropic"] {
            assert_eq!(findings(&format!("let s = \"{text}_x\";")).len(), 1, "{text}");
            assert_eq!(findings(&format!("fn pre_{text}_post() {{}}")).len(), 1, "{text}");
        }
        assert!(findings("let s = \"Claud\"; fn anthrop() {}").is_empty());
    }

    #[test]
    fn a_test_module_is_skipped_and_the_code_after_it_is_not() {
        let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn claude_x() { let s = \"Claude\"; }\n}\nfn find_claude() {}\n";
        let found = findings(src);
        assert_eq!(found, vec![Finding { line: 6, text: "find_claude".to_owned() }]);
    }

    fn texts(src: &str) -> Vec<String> {
        findings(src).into_iter().map(|f| f.text).collect()
    }

    #[test]
    fn a_cfg_test_parameter_or_list_element_never_swallows_the_next_block() {
        // The closer of the enclosing list ends the skip and is not taken: the block that follows is real code.
        assert_eq!(texts("fn f(a: u8, #[cfg(test)] b: u8) { let p = \"claude.exe\"; }"), vec!["claude.exe".to_owned()]);
        assert_eq!(texts("fn f(#[cfg(test)] b: u8) {}\nfn claude() {}"), vec!["claude".to_owned()]);
        assert_eq!(texts("pub fn spawn(path: &str, #[cfg(test)] hook: u8) -> u8 { let p = \"claude.exe\"; 0 }"), vec!["claude.exe".to_owned()]);
        assert_eq!(texts("let v = [a, #[cfg(test)] b]; fn claude() {}"), vec!["claude".to_owned()]);
        // The array has no `;` before the block here, so only the `]` can end the skip.
        assert_eq!(texts("match [a, #[cfg(test)] b] { _ => claude() }"), vec!["claude".to_owned()]);
        assert_eq!(texts("call(a, #[cfg(test)] b); fn claude() {}"), vec!["claude".to_owned()]);
        // A parameter in the middle is skipped up to its comma.
        assert_eq!(texts("fn f(#[cfg(test)] b: (u8, u8), c: u8) { claude() }"), vec!["claude".to_owned()]);
        assert!(texts("fn f(#[cfg(test)] claude: u8, c: u8) {}").is_empty());
    }

    #[test]
    fn a_stacked_attribute_does_not_make_a_list_an_item_position() {
        // The `]` of an attribute in front of the `cfg` is no item position: the element ends at its comma and the
        // rest of the list is read as code.
        assert_eq!(texts("struct S(#[allow(dead_code)] #[cfg(test)] fn() -> u8, ClaudeHook);"), vec!["ClaudeHook".to_owned()]);
        assert_eq!(texts("call(#[allow(x)] #[cfg(test)] async || 1, \"claude.exe\");"), vec!["claude.exe".to_owned()]);
        assert_eq!(texts("struct S<#[allow(x)] #[cfg(test)] const N: usize, ClaudeMarker> {}"), vec!["ClaudeMarker".to_owned()]);
        // The `]` of an index is none either.
        assert_eq!(texts("call(w[1] #[cfg(test)] fn() -> u8, \"claude.exe\");"), vec!["claude.exe".to_owned()]);
    }

    #[test]
    fn an_attribute_stack_in_front_of_an_item_keeps_the_item_position() {
        assert!(texts("#[derive(Debug)] #[cfg(test)] struct S<A, B> { claude: (A, B) }").is_empty());
        assert!(texts("fn a() {}\n#[allow(x)] #[cfg(test)] fn helper<A, B>() -> HashMap<A, B> { claude() }").is_empty());
    }

    #[test]
    fn a_cfg_that_holds_only_in_test_builds_is_skipped() {
        assert!(texts("#[cfg(all(test, windows))] fn claude() {}").is_empty());
    }

    #[test]
    fn a_cfg_that_can_hold_in_a_normal_build_is_judged() {
        assert_eq!(findings("#[cfg(any(test, feature = \"x\"))] fn claude() {}").len(), 1);
    }

    #[test]
    fn a_file_that_starts_with_an_inner_cfg_test_is_still_judged() {
        // This gate skips an item at most, never a whole file (S1G-15): `is_test_file` is for lang-lint only.
        assert_eq!(findings("#![cfg(test)]\nfn claude() {}").len(), 1);
    }

    #[test]
    fn lines_are_counted_across_strings_and_comments() {
        let src = "// one\n/* two\nthree */\nfn a() { let s = \"x\ny\"; }\nfn claude() {}\n";
        assert_eq!(findings(src), vec![Finding { line: 6, text: "claude".to_owned() }]);
    }

    #[test]
    fn a_raw_identifier_that_names_a_provider_is_found() {
        assert_eq!(findings("let r#claude = 1;").len(), 1);
    }

    #[test]
    fn the_report_names_file_line_and_text() {
        let report = check_tree(&tree_with("launch.rs", "\n\nfn find_claude() {}"));
        assert_eq!(report, vec!["launch.rs:3: `find_claude`".to_owned()]);
    }

    #[test]
    fn the_core_source_of_this_checkout_names_no_provider() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask sits in the workspace root").join(SOURCE_DIR);
        let files = read_tree(&dir).expect("the core source can be read");
        assert!(files.iter().any(|(path, _)| path == "provider.rs"), "the tree holds the provider module");
        assert_eq!(check_tree(&files), Vec::<String>::new());
    }
}
