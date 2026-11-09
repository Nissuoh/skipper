// SPDX-License-Identifier: MIT
//! The reading of Rust source that `provider-name-check` and `lang-lint` share: one tokenizer and one rule for the code
//! that exists only in test builds (a follow-up of Stage 1 Tasks 4 and 10, which had a copy each).
//!
//! [`tokenize`] splits source into identifiers, string literals, single punctuation characters and a marker for a
//! number or a character literal; comments (`//`, `///`, `//!`, nested `/* */`) are not tokens. [`skip_test_item`] says
//! where the item or list element ends that carries a test-only `#[cfg(..)]`; [`is_test_file`] and
//! [`test_module_names`] find the files that are test code as a whole.
//!
//! The rule behind every skip, never to be relaxed: a skip may end too EARLY, because test code is then judged (a
//! false positive that shows and can be answered), and it must never end too LATE, because code that ships is then left
//! unchecked (a defect: `provider-name-check` is the only enforcement of S1G-15). So a `cfg` counts as test code only
//! when it provably holds in test builds and nowhere else (`test`, `all(test, ..)`, `any(..)` of such predicates), and a
//! file counts as test code only when every declaration of it says so.
//!
//! Where a skip ends early (the tail is then read as code, never the other way round): a `,` or `>` inside the `< >` of
//! a field type; the content of a braced variant or arm; a const block in generics (`impl Foo<{ 3 }>`); a closure or an
//! arm pattern with `|` as the skipped element; a `#[cfg(test)]` statement that starts with `if`, `match` or `for`; the
//! body of a test-only macro call.

/// One token of Rust source. The number in each variant is the line of the token, counted from 1.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Token {
    /// An identifier or a keyword; a raw identifier keeps its `r#`, so `r#type` is never the keyword `type`.
    Ident(String, usize),
    /// A string literal (also raw, byte and C strings) as written between its quotes.
    Str(String, usize),
    Punct(char, usize),
    /// A number or a character literal. It only marks the place (no name can sit in it), so that `= 3 >` is not
    /// read as `=>`.
    Literal,
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// The text of a string literal that starts at `start` (just after the opening quote): up to the closing quote,
/// with `\` taking the next character along. Returns the text, the index after the closing quote and the number
/// of line breaks inside.
fn read_string(chars: &[char], start: usize) -> (String, usize, usize) {
    let mut i = start;
    let mut lines = 0;
    let mut text = String::new();
    while i < chars.len() && chars[i] != '"' {
        if chars[i] == '\\' && i + 1 < chars.len() {
            text.push(chars[i]);
            i += 1;
        }
        if chars[i] == '\n' {
            lines += 1;
        }
        text.push(chars[i]);
        i += 1;
    }
    (text, (i + 1).min(chars.len()), lines)
}

/// The text of a raw string literal that starts at `start` (just after the opening quote) and ends at a quote
/// followed by `hashes` number signs. Returns the text, the index after the end and the number of line breaks.
fn read_raw_string(chars: &[char], start: usize, hashes: usize) -> (String, usize, usize) {
    let mut i = start;
    let mut lines = 0;
    let mut text = String::new();
    while i < chars.len() {
        if chars[i] == '"' && (0..hashes).all(|k| chars.get(i + 1 + k) == Some(&'#')) {
            return (text, i + 1 + hashes, lines);
        }
        if chars[i] == '\n' {
            lines += 1;
        }
        text.push(chars[i]);
        i += 1;
    }
    (text, chars.len(), lines)
}

/// True when `skip_quote(chars, start)`, which returned `next`, skipped a character literal (it ends with the
/// closing quote) and not a lifetime or a loop label.
fn is_char_literal(chars: &[char], start: usize, next: usize) -> bool {
    next > start + 1 && chars.get(next - 1) == Some(&'\'')
}

/// The index after a character literal or a lifetime that starts with the quote at `i`.
fn skip_quote(chars: &[char], i: usize) -> usize {
    match (chars.get(i + 1), chars.get(i + 2)) {
        // An escaped character literal: skip the backslash and the escaped character, then up to the closing quote.
        (Some('\\'), _) => {
            let mut j = i + 3;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            (j + 1).min(chars.len())
        }
        // One character between two quotes, also a double quote.
        (Some(c), Some('\'')) if *c != '\'' => i + 3,
        // A lifetime or a loop label.
        _ => {
            let mut j = i + 1;
            while j < chars.len() && is_ident_continue(chars[j]) {
                j += 1;
            }
            j
        }
    }
}

/// The tokens of `src`, comments left out. Source that ends inside a string, a comment or a literal ends the token
/// there; nothing here panics on truncated or invalid source.
pub(crate) fn tokenize(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut line = 1usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == '"' {
            let (text, next, lines) = read_string(&chars, i + 1);
            tokens.push(Token::Str(text, line));
            line += lines;
            i = next;
        } else if c == '\'' {
            let next = skip_quote(&chars, i);
            if is_char_literal(&chars, i, next) {
                tokens.push(Token::Literal);
            }
            i = next;
        } else if is_ident_start(c) {
            let start = i;
            while i < chars.len() && is_ident_continue(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Prefixes of literals: `r"..."`, `r#"..."#`, `b"..."`, `br"..."`, `c"..."`, `b'x'` and raw identifiers.
            if matches!(word.as_str(), "r" | "br" | "cr") {
                let mut j = i;
                while chars.get(j) == Some(&'#') {
                    j += 1;
                }
                let hashes = j - i;
                if chars.get(j) == Some(&'"') {
                    let (text, next, lines) = read_raw_string(&chars, j + 1, hashes);
                    tokens.push(Token::Str(text, line));
                    line += lines;
                    i = next;
                    continue;
                }
                if word == "r" && hashes == 1 && chars.get(j).is_some_and(|c| is_ident_start(*c)) {
                    let name_start = j;
                    i = j;
                    while i < chars.len() && is_ident_continue(chars[i]) {
                        i += 1;
                    }
                    let name: String = chars[name_start..i].iter().collect();
                    tokens.push(Token::Ident(format!("r#{name}"), line));
                    continue;
                }
            }
            if matches!(word.as_str(), "b" | "c") && chars.get(i) == Some(&'"') {
                let (text, next, lines) = read_string(&chars, i + 1);
                tokens.push(Token::Str(text, line));
                line += lines;
                i = next;
                continue;
            }
            if word == "b" && chars.get(i) == Some(&'\'') {
                let next = skip_quote(&chars, i);
                if is_char_literal(&chars, i, next) {
                    tokens.push(Token::Literal);
                }
                i = next;
                continue;
            }
            tokens.push(Token::Ident(word, line));
        } else if c.is_ascii_digit() {
            while i < chars.len() && is_ident_continue(chars[i]) {
                i += 1;
            }
            tokens.push(Token::Literal);
        } else {
            tokens.push(Token::Punct(c, line));
            i += 1;
        }
    }
    tokens
}

/// True when the token at `i` is the punctuation character `c`.
pub(crate) fn is_punct(tokens: &[Token], i: usize, c: char) -> bool {
    matches!(tokens.get(i), Some(Token::Punct(p, _)) if *p == c)
}

/// True when the token at `i` is the identifier `name`.
pub(crate) fn is_ident(tokens: &[Token], i: usize, name: &str) -> bool {
    matches!(tokens.get(i), Some(Token::Ident(w, _)) if w == name)
}

/// The identifier at `i`.
pub(crate) fn ident_at(tokens: &[Token], i: usize) -> Option<&str> {
    match tokens.get(i) {
        Some(Token::Ident(name, _)) => Some(name.as_str()),
        _ => None,
    }
}

/// The text and the line of the string literal at `i`.
pub(crate) fn str_at(tokens: &[Token], i: usize) -> Option<(&str, usize)> {
    match tokens.get(i) {
        Some(Token::Str(text, line)) => Some((text.as_str(), *line)),
        _ => None,
    }
}

/// How deep a `cfg` predicate may nest before it is read as malformed (and so judged); no real one comes near it.
const MAX_PREDICATE_DEPTH: usize = 32;

/// What the `cfg` predicate that starts at `at` says, and the index after it: `Some((test_only, after))`, or `None` when
/// the tokens are no predicate (the end of the source, a stray token, nesting deeper than [`MAX_PREDICATE_DEPTH`]).
///
/// `test_only` is true when the predicate holds in test builds and nowhere else, parsed over the tokens and never over
/// text:
/// - `test` alone is. `test = ".."` is a name and a value, and `r#test` is another name: neither is.
/// - `all(p, ..)` is when at least one argument is (it needs that argument to hold). `all()` always holds: it is not.
/// - `any(p, ..)` is when there is at least one argument and every argument is. `any()` never holds, and
///   `any(test, windows)` holds on Windows in a normal build: neither is.
/// - `not(..)` is not, whatever it holds, and so is every other name, `name = "value"` and `name(..)`.
fn predicate(tokens: &[Token], at: usize, depth: usize) -> Option<(bool, usize)> {
    if depth > MAX_PREDICATE_DEPTH {
        return None;
    }
    let name = ident_at(tokens, at)?;
    let next = at + 1;
    if is_punct(tokens, next, '=') {
        return str_at(tokens, next + 1).map(|_| (false, next + 2));
    }
    if !is_punct(tokens, next, '(') {
        return Some((name == "test", next));
    }
    // The arguments: predicates, separated by commas, a comma allowed after the last one.
    let mut arguments: Vec<bool> = Vec::new();
    let mut at = next + 1;
    while !is_punct(tokens, at, ')') {
        let (test_only, after) = predicate(tokens, at, depth + 1)?;
        arguments.push(test_only);
        at = after;
        if is_punct(tokens, at, ',') {
            at += 1;
        } else if !is_punct(tokens, at, ')') {
            return None;
        }
    }
    let test_only = match name {
        "all" => arguments.iter().any(|argument| *argument),
        "any" => !arguments.is_empty() && arguments.iter().all(|argument| *argument),
        _ => false,
    };
    Some((test_only, at + 1))
}

/// True when a test-only predicate starts at `at` (the first token inside `cfg(`) and the attribute ends right after it:
/// an optional comma, `)` and `]`. Anything else, a malformed attribute included, is false.
fn is_test_only_cfg(tokens: &[Token], at: usize) -> bool {
    let Some((true, after)) = predicate(tokens, at, 0) else {
        return false;
    };
    let after = if is_punct(tokens, after, ',') { after + 1 } else { after };
    is_punct(tokens, after, ')') && is_punct(tokens, after + 1, ']')
}

/// The index after the attribute at `i` when it is `#[cfg(..)]` with a test-only predicate (see [`predicate`]); `None`
/// for anything else, `cfg_attr` and a malformed attribute included.
fn cfg_test_end(tokens: &[Token], i: usize) -> Option<usize> {
    let opens = is_punct(tokens, i, '#')
        && is_punct(tokens, i + 1, '[')
        && is_ident(tokens, i + 2, "cfg")
        && is_punct(tokens, i + 3, '(');
    (opens && is_test_only_cfg(tokens, i + 4)).then(|| skip_attribute(tokens, i))
}

/// True when the file starts with an inner attribute `#![cfg(..)]` with a test-only predicate: the whole file is test
/// code. Only the leading `#![..]` attributes are looked at.
pub(crate) fn is_test_file(tokens: &[Token]) -> bool {
    let mut i = 0;
    while is_punct(tokens, i, '#') && is_punct(tokens, i + 1, '!') && is_punct(tokens, i + 2, '[') {
        if is_ident(tokens, i + 3, "cfg") && is_punct(tokens, i + 4, '(') && is_test_only_cfg(tokens, i + 5) {
            return true;
        }
        i = skip_attribute(tokens, i);
    }
    false
}

/// The index after the attribute that starts at `i` (`#`, maybe `!`, then a bracket group).
fn skip_attribute(tokens: &[Token], mut i: usize) -> usize {
    i += 1;
    if is_punct(tokens, i, '!') {
        i += 1;
    }
    let mut depth = 0i32;
    while i < tokens.len() {
        if is_punct(tokens, i, '[') {
            depth += 1;
        } else if is_punct(tokens, i, ']') {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    i
}

/// The index after the block whose `{` is at `i`.
fn skip_block(tokens: &[Token], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < tokens.len() {
        if is_punct(tokens, i, '{') {
            depth += 1;
        } else if is_punct(tokens, i, '}') {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    i
}

/// The words that start an item (or a `let` statement) rather than a list element. `union` and `macro_rules` are only
/// keywords in front of a name or a `!`, so they are told apart in [`starts_an_item`].
const ITEM_KEYWORDS: [&str; 14] =
    ["fn", "mod", "impl", "struct", "enum", "trait", "type", "const", "static", "use", "extern", "unsafe", "async", "let"];

/// True when the `>` at `i` is the tail of `->` or `=>`, not a closing angle bracket.
fn is_arrow_tail(tokens: &[Token], i: usize) -> bool {
    i > 0 && (is_punct(tokens, i - 1, '-') || is_punct(tokens, i - 1, '='))
}

/// The index after the balanced `( ... )` group whose `(` is at `i`.
fn skip_parens(tokens: &[Token], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < tokens.len() {
        if is_punct(tokens, i, '(') {
            depth += 1;
        } else if is_punct(tokens, i, ')') {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    i
}

/// True when the tokens at `i` (after the attributes, an optional `pub` and an optional `pub(...)`) start an item:
/// the next word is one of [`ITEM_KEYWORDS`], `union` before a name, or `macro_rules` before `!`.
fn starts_an_item(tokens: &[Token], mut i: usize) -> bool {
    if is_ident(tokens, i, "pub") {
        i += 1;
        if is_punct(tokens, i, '(') {
            i = skip_parens(tokens, i);
        }
    }
    match tokens.get(i) {
        Some(Token::Ident(word, _)) => match word.as_str() {
            "union" => matches!(tokens.get(i + 1), Some(Token::Ident(..))),
            "macro_rules" => is_punct(tokens, i + 1, '!'),
            other => ITEM_KEYWORDS.contains(&other),
        },
        _ => false,
    }
}

/// The index of the `#` of the attribute whose closing `]` is at `close`, or `None` when that `]` closes something
/// else (an index, an array).
fn attribute_start(tokens: &[Token], close: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut at = close;
    loop {
        if is_punct(tokens, at, ']') {
            depth += 1;
        } else if is_punct(tokens, at, '[') {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        at = at.checked_sub(1)?;
    }
    let mut hash = at.checked_sub(1)?;
    if is_punct(tokens, hash, '!') {
        hash = hash.checked_sub(1)?;
    }
    is_punct(tokens, hash, '#').then_some(hash)
}

/// True when the attribute that starts at `i` stands where an item or a statement can start: at the start of the
/// file, or after a `;`, a `{` or a `}`, looking past the attributes in front of it (`#[a] #[b] #[cfg(test)] ...`).
/// After anything else (a `(`, a `[`, a `<`, a `,`, a `|`, a word, or the `]` of something that is no attribute) it
/// stands inside a list, whatever word follows it.
fn at_item_position(tokens: &[Token], mut i: usize) -> bool {
    while i > 0 && is_punct(tokens, i - 1, ']') {
        match attribute_start(tokens, i - 1) {
            Some(start) => i = start,
            None => return false,
        }
    }
    i == 0 || [';', '{', '}'].iter().any(|c| is_punct(tokens, i - 1, *c))
}

/// The index after the one item or list element that follows an attribute: any further attributes, then everything
/// up to its end. Two rules. An attribute that stands where an item can start ([`at_item_position`]) and is followed
/// by an item keyword takes the item rule; every other attribute takes the element rule.
///
/// - An item (`fn`, `mod`, `impl`, `struct`, `enum`, `trait`, `type`, `const`, `static`, `use`, `extern`, `unsafe`,
///   `async`, `let`, `union NAME`, `macro_rules!`, after an optional `pub`) ends at a `;` outside `( )` and `[ ]`,
///   taken with it, or after its first `{ ... }` block. Commas do not end it (`impl<'a, T>`, `-> HashMap<K, V>`).
/// - Anything else is an element of a list (a struct field, an enum variant, a match arm, a parameter, an array or
///   call element, a generic parameter, a where predicate, a struct-literal field). It ends at a `,` or `;` outside
///   `( )` and `[ ]`, taken with it.
///
/// Both rules end the skip *before* a closer that has no opener inside the skip, without taking it: `}`, `)` or `]`
/// closes the enclosing list (the last field, variant, arm, parameter or element has no comma), so the scan goes on
/// after it and the next block of real code is not swallowed. The element rule also ends, without taking it,
/// - before a `{`: a block never belongs to a skipped element (the body of a function whose last where predicate
///   carries the attribute, the fields of a variant, the block of an arm; the last two are then read as code);
/// - before a `|` that is not its first token: it closes the parameter list of a closure (`|a, #[cfg(test)] b| body`);
/// - before a `>` that is not part of `->` or `=>`: it closes the generic parameters (`<A, #[cfg(test)] B>`).
///
/// An item ends before a `>` that closes no `<` of its own, and before a `{` it skips as its body.
///
/// `< >` is not tracked for the element rule, because a `<` that is a comparison would then never close. So a `,` or
/// `>` inside the angle brackets of a field type (`HashMap<A, B>`, `Option<T>`) ends the element early and its tail
/// is read as code, and so does the content of a braced variant or arm: the check can then report a name that sits
/// in test-only code. The skip is never made longer by this; it always ends at the first terminator it meets.
fn skip_item(tokens: &[Token], mut i: usize, item_position: bool) -> usize {
    while is_punct(tokens, i, '#') {
        i = skip_attribute(tokens, i);
    }
    let start = i;
    let whole_item = item_position && starts_an_item(tokens, i);
    let mut depth = 0i32;
    let mut angle = 0i32;
    while i < tokens.len() {
        if depth == 0 {
            if is_punct(tokens, i, ';') || (!whole_item && is_punct(tokens, i, ',')) {
                return i + 1;
            }
            if is_punct(tokens, i, '}') || is_punct(tokens, i, ')') || is_punct(tokens, i, ']') {
                return i;
            }
            if is_punct(tokens, i, '{') {
                return if whole_item { skip_block(tokens, i) } else { i };
            }
            if !whole_item && i != start && is_punct(tokens, i, '|') {
                return i;
            }
            if is_punct(tokens, i, '>') && !is_arrow_tail(tokens, i) {
                if !whole_item || angle == 0 {
                    return i;
                }
                angle -= 1;
            } else if whole_item && is_punct(tokens, i, '<') {
                angle += 1;
            }
        }
        if is_punct(tokens, i, '(') || is_punct(tokens, i, '[') {
            depth += 1;
        } else if is_punct(tokens, i, ')') || is_punct(tokens, i, ']') {
            depth -= 1;
        }
        i += 1;
    }
    i
}

/// The index after the item or list element that carries the test-only `#[cfg(..)]` that starts at `i`, or `None` when
/// no such attribute starts there: not a `cfg` (`cfg_attr` is none), a predicate that can hold in a normal build, a
/// malformed attribute. A caller walks the tokens and jumps to the returned index, which is always greater than `i`.
///
/// A `cfg` is test-only when its predicate holds in test builds and nowhere else: `test`, `all(..)` with at least one
/// such argument, `any(..)` of which every argument is such. `not(..)`, `any(test, windows)` (it holds on Windows in a
/// normal build) and every other predicate are judged. [`at_item_position`] decides which rule of [`skip_item`] ends the
/// skip; both are called here, with the same `i`, so that no caller can pass a wrong position.
pub(crate) fn skip_test_item(tokens: &[Token], i: usize) -> Option<usize> {
    let end = cfg_test_end(tokens, i)?;
    Some(skip_item(tokens, end, at_item_position(tokens, i)))
}

/// The names of the modules that the file declares at file level as `mod name;` and that exist in test builds only: every
/// declaration of the name carries a test-only `cfg` (before or after other attributes, behind an optional `pub` or
/// `pub(..)`), and none redirects the file with `#[path]` (or with a `cfg_attr`, which may carry one). Their files are
/// test code. A name that is declared once for test builds and once for normal builds (`#[cfg(test)] mod x;` and
/// `#[cfg(not(test))] mod x;`) is not returned: `x.rs` ships. Known limit: a declaration inside an inline module is not
/// seen, and the file is then judged (a false positive only).
pub(crate) fn test_module_names(tokens: &[Token]) -> Vec<String> {
    // Every `mod name;` at file level: its name, and whether this declaration alone is provably test-only.
    let mut declared: Vec<(&str, bool)> = Vec::new();
    let mut depth = 0i32;
    // What the attributes in front of the item that starts at `i` say: a test-only `cfg`; a `path` or a `cfg_attr`.
    let mut test_only = false;
    let mut redirected = false;
    let mut i = 0;
    while i < tokens.len() {
        if depth == 0 && is_punct(tokens, i, '#') && is_punct(tokens, i + 1, '[') {
            test_only |= cfg_test_end(tokens, i).is_some();
            redirected |= is_ident(tokens, i + 2, "path") || is_ident(tokens, i + 2, "cfg_attr");
            i = skip_attribute(tokens, i);
            continue;
        }
        if depth == 0 {
            let mut at = i;
            if is_ident(tokens, at, "pub") {
                at += 1;
                if is_punct(tokens, at, '(') {
                    at = skip_parens(tokens, at);
                }
            }
            if is_ident(tokens, at, "mod") && is_punct(tokens, at + 2, ';') {
                if let Some(name) = ident_at(tokens, at + 1) {
                    declared.push((name, test_only && !redirected));
                }
                (test_only, redirected) = (false, false);
                i = at + 3;
                continue;
            }
        }
        // Any other token ends the attributes in front of an item.
        if is_punct(tokens, i, '{') {
            depth += 1;
        } else if is_punct(tokens, i, '}') {
            depth -= 1;
        }
        (test_only, redirected) = (false, false);
        i += 1;
    }
    let mut out: Vec<String> = Vec::new();
    for &(name, _) in &declared {
        let every = declared.iter().filter(|(other, _)| *other == name).all(|(_, provable)| *provable);
        if every && !out.iter().any(|known| known == name) {
            out.push(name.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identifiers and strings that hold "claude" (any letter case) and that a caller's loop looks at: the tokens
    /// outside the items that `skip_test_item` skips. The tests put `claude` where code ships and `claude_hidden` where
    /// a test-only item must hide it.
    fn seen(src: &str) -> Vec<String> {
        let tokens = tokenize(src);
        assert_moves_forward(&tokens);
        let mut out = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            if let Some(end) = skip_test_item(&tokens, i) {
                i = end;
                continue;
            }
            if let Token::Ident(text, _) | Token::Str(text, _) = &tokens[i] {
                if text.to_ascii_lowercase().contains("claude") {
                    out.push(text.clone());
                }
            }
            i += 1;
        }
        out
    }

    /// `skip_test_item` never returns the index it was given or an earlier one: the callers' loops end. Every source
    /// that goes through `seen` is checked at every token.
    fn assert_moves_forward(tokens: &[Token]) {
        for i in 0..tokens.len() {
            if let Some(end) = skip_test_item(tokens, i) {
                assert!(end > i, "skip_test_item({i}) returned {end}");
            }
        }
    }

    fn strings(src: &str) -> Vec<String> {
        tokenize(src)
            .into_iter()
            .filter_map(|token| match token {
                Token::Str(text, _) => Some(text),
                _ => None,
            })
            .collect()
    }

    fn names(src: &str) -> Vec<String> {
        test_module_names(&tokenize(src))
    }

    // The tokenizer.

    #[test]
    fn comments_are_not_tokens() {
        assert!(seen("// claude\n/// claude\n//! claude\nfn f() {}").is_empty());
        assert!(seen("/* a /* claude */ still a comment, claude */ fn f() {}").is_empty());
        assert_eq!(seen("/* claude */ fn claude() {}"), ["claude"]);
        // A comment marker inside a string is no comment.
        assert_eq!(seen("let s = \"// claude\"; fn g() {}"), ["// claude"]);
    }

    #[test]
    fn lines_are_counted_across_strings_and_comments() {
        let tokens = tokenize("// one\n/* two\nthree */\nfn a() { let s = \"x\ny\"; }\nfn claude() {}\n");
        assert!(tokens.contains(&Token::Str("x\ny".to_owned(), 4)));
        assert!(tokens.contains(&Token::Ident("claude".to_owned(), 6)));
    }

    #[test]
    fn raw_strings_byte_strings_and_c_strings_are_strings() {
        assert_eq!(strings("let s = r#\"x claude \"y\"#;"), ["x claude \"y"]);
        assert_eq!(strings("let s = br\"claude\"; let t = b\"claude\"; let u = c\"claude\";"), ["claude", "claude", "claude"]);
        assert_eq!(strings("let s = cr#\"claude\"#;"), ["claude"]);
        // A `//` after a raw string is a comment.
        assert_eq!(strings("let s = r#\"plain\"#; // \"claude\""), ["plain"]);
        assert!(seen("let s = r#\"plain\"#; // claude").is_empty());
    }

    #[test]
    fn a_raw_identifier_keeps_its_prefix_and_is_no_keyword() {
        assert_eq!(tokenize("r#type"), vec![Token::Ident("r#type".to_owned(), 1)]);
        assert!(!starts_an_item(&tokenize("r#type x"), 0));
        assert!(starts_an_item(&tokenize("type x"), 0));
    }

    #[test]
    fn character_literals_and_lifetimes_do_not_open_strings() {
        // A double quote in a character literal must not start a string that swallows the next name.
        assert_eq!(seen("let q = '\"'; let n = \"claude\";"), ["claude"]);
        assert_eq!(seen("let q = '\\''; let n = \"claude\";"), ["claude"]);
        assert_eq!(seen("let q = b'\"'; fn claude() {}"), ["claude"]);
        assert!(seen("fn f<'a>(x: &'a str) -> &'a str { x } // claude").is_empty());
        assert_eq!(seen("fn f<'a>(x: &'a str) { let claude = x; }"), ["claude"]);
    }

    #[test]
    fn a_number_or_character_literal_is_a_token_so_it_is_never_read_as_an_arrow() {
        let tokens = tokenize("= 3 > = 'a' > = b'a' > = 0x1f >");
        let closers: Vec<usize> = (0..tokens.len()).filter(|&i| is_punct(&tokens, i, '>')).collect();
        assert_eq!(closers.len(), 4);
        for i in closers {
            assert!(!is_arrow_tail(&tokens, i), "{i}");
        }
        assert_eq!(tokens.iter().filter(|t| matches!(t, Token::Literal)).count(), 4);
        // A lifetime is no literal and no token; an arrow is still an arrow.
        assert!(tokenize("'a 'static").is_empty());
        let arrows = tokenize("=> ->");
        assert!(is_arrow_tail(&arrows, 1) && is_arrow_tail(&arrows, 3));
    }

    #[test]
    fn truncated_source_never_panics() {
        for src in [
            "\"abc",
            "/* x",
            "r#\"x",
            "'",
            "b'",
            "'\\",
            "#[cfg(test",
            "#[cfg(",
            "#[cfg(test)",
            "#[cfg(test)]",
            "#[cfg(test)] fn f(",
            "#[cfg(test)] mod t",
            "#![cfg(",
            "#![cfg(test)",
            "] #[cfg(test)]",
            "mod tests",
        ] {
            let tokens = tokenize(src);
            for i in 0..=tokens.len() {
                let _ = skip_test_item(&tokens, i);
            }
            let _ = is_test_file(&tokens);
            let _ = test_module_names(&tokens);
        }
    }

    // Where a test-only item or list element ends.

    #[test]
    fn a_test_module_is_skipped_and_the_code_after_it_is_not() {
        let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn claude_x() { let s = \"Claude\"; }\n}\nfn find_claude() {}\n";
        assert_eq!(seen(src), ["find_claude"]);
    }

    #[test]
    fn a_cfg_test_struct_field_skips_only_that_field() {
        let src = "pub struct S {\n    #[cfg(test)]\n    pub fake: bool,\n}\npub fn find_claude() { let p = \"claude.exe\"; }";
        assert_eq!(seen(src), ["find_claude", "claude.exe"]);
        // The last field has no comma: the item ends before the brace that closes the struct.
        assert_eq!(seen("struct S { #[cfg(test)] f: bool }\nfn claude() {}"), ["claude"]);
        assert_eq!(seen("struct S { a: u8, #[cfg(test)] f: u8, }\nfn claude() {}"), ["claude"]);
        // A comma inside a tuple type or a generic call is not the end of the field.
        assert_eq!(seen("struct S { #[cfg(test)] f: (u8, u8), claude: u8 }"), ["claude"]);
        assert!(seen("struct S { #[cfg(test)] f: (claude, u8), g: u8 }").is_empty());
    }

    #[test]
    fn a_cfg_test_enum_variant_skips_only_that_variant() {
        assert_eq!(seen("enum E { #[cfg(test)] A, B }\nfn claude() {}"), ["claude"]);
        assert!(seen("enum E { #[cfg(test)] Claude, B }").is_empty());
        assert_eq!(seen("enum E { #[cfg(test)] A { x: u8 }, Claude }"), ["Claude"]);
        assert_eq!(seen("enum E { #[cfg(test)] A(u8), Claude(u8) }"), ["Claude"]);
        assert_eq!(seen("enum E { A, #[cfg(test)] B(u8), C }\nfn claude() {}"), ["claude"]);
        assert_eq!(seen("enum E { A, #[cfg(test)] B }\nfn claude() {}"), ["claude"]);
    }

    #[test]
    fn a_cfg_test_match_arm_skips_only_that_arm() {
        assert_eq!(seen("match x { #[cfg(test)] A => 1, _ => claude() }"), ["claude"]);
        assert!(seen("match x { #[cfg(test)] A => claude(), _ => 2 }").is_empty());
        assert_eq!(seen("match x { #[cfg(test)] A => { 1 } _ => claude() }"), ["claude"]);
        assert!(seen("match x { _ => 2, #[cfg(test)] A => claude() }").is_empty());
        assert_eq!(seen("fn f(v: u8) { match v { 0 => 1, #[cfg(test)] 1 => 2 } claude() }"), ["claude"]);
    }

    #[test]
    fn a_cfg_test_struct_literal_field_skips_only_that_field() {
        assert_eq!(seen("S { #[cfg(test)] a: claude_hidden(), b: \"claude\" }"), ["claude"]);
        // The last field has no comma: the skip ends before the brace that closes the literal.
        assert!(seen("S { a: 1, #[cfg(test)] b: claude_hidden() }").is_empty());
        assert_eq!(seen("let s = S { #[cfg(test)] a: claude_hidden() };\nfn claude() {}"), ["claude"]);
    }

    #[test]
    fn a_cfg_test_parameter_or_list_element_never_swallows_the_next_block() {
        // The closer of the enclosing list ends the skip and is not taken: the block that follows is real code.
        assert_eq!(seen("fn f(a: u8, #[cfg(test)] b: u8) { let p = \"claude.exe\"; }"), ["claude.exe"]);
        assert_eq!(seen("fn f(#[cfg(test)] b: u8) {}\nfn claude() {}"), ["claude"]);
        assert_eq!(seen("pub fn spawn(path: &str, #[cfg(test)] hook: u8) -> u8 { let p = \"claude.exe\"; 0 }"), ["claude.exe"]);
        assert_eq!(seen("let v = [a, #[cfg(test)] b]; fn claude() {}"), ["claude"]);
        // The array has no `;` before the block here, so only the `]` can end the skip.
        assert_eq!(seen("match [a, #[cfg(test)] b] { _ => claude() }"), ["claude"]);
        assert_eq!(seen("match [a, #[cfg(test)] b] { _ => { claude() } }"), ["claude"]);
        assert_eq!(seen("call(a, #[cfg(test)] b); fn claude() {}"), ["claude"]);
        // A parameter or an element in the middle is skipped up to its comma and no further.
        assert_eq!(seen("fn f(#[cfg(test)] b: (u8, u8), c: u8) { claude() }"), ["claude"]);
        assert!(seen("fn f(#[cfg(test)] claude: u8, c: u8) {}").is_empty());
        assert_eq!(seen("struct S { a: u8, #[cfg(test)] f: (u8, u8), g: u8 }\nfn h() { claude() }"), ["claude"]);
        assert_eq!(seen("match v { #[cfg(test)] A => claude_hidden(), _ => 2 }\nclaude()"), ["claude"]);
    }

    #[test]
    fn a_cfg_test_generic_parameter_never_swallows_the_next_block() {
        assert_eq!(seen("fn f<A, #[cfg(test)] B>() { let p = \"claude\"; }"), ["claude"]);
        assert_eq!(seen("fn f<A, #[cfg(test)] B: Fn() -> u8>() { let p = \"claude\"; }"), ["claude"]);
        assert_eq!(seen("fn f<A, #[cfg(test)] B: Fn() -> u8>() { claude() }"), ["claude"]);
        // A const generic parameter starts like a constant: the `>` that closes no `<` of its own ends it.
        assert_eq!(seen("fn f<A, #[cfg(test)] const N: usize>() { let p = \"claude\"; }"), ["claude"]);
        assert_eq!(seen("fn f<A, #[cfg(test)] const N: usize>() { claude() }"), ["claude"]);
        assert!(seen("struct S<A, #[cfg(test)] Claude> { a: A }").is_empty());
    }

    #[test]
    fn a_whole_cfg_test_item_is_not_cut_at_a_comma_inside_its_angle_brackets() {
        assert!(seen("#[cfg(test)] impl<'a, T> X<'a, T> { fn claude_fake() {} }").is_empty());
        assert!(seen("#[cfg(test)] fn helper() -> HashMap<String, String> { \"Claude\" }").is_empty());
        assert!(seen("#[cfg(test)] type M = HashMap<K, Claude>;").is_empty());
        assert!(seen("#[cfg(test)] pub(crate) fn helper() -> HashMap<A, B> { \"Claude\" }").is_empty());
        assert!(seen("#[cfg(test)] impl<T: Into<u8>> X for Y where T: Z { fn claude() {} }").is_empty());
        assert!(seen("#[cfg(test)] const X: [u8; 2] = [1, 2];\n#[cfg(test)] static Y: &str = \"Claude\";").is_empty());
        assert_eq!(seen("#[cfg(test)] fn helper() -> HashMap<A, B> { 1 }\nfn claude() {}"), ["claude"]);
        let helper = "#[cfg(test)]\nfn helper<A, B>(a: A, b: B) -> Option<(A, B)> { claude_hidden(); }\nfn g() { claude(); }";
        assert_eq!(seen(helper), ["claude"]);
        // Each head is skipped whole, and the function after it is read as code.
        for head in [
            "#[cfg(test)] fn helper() -> HashMap<String, String> { claude_hidden(); }",
            "#[cfg(test)] impl<'a, T> X<'a, T> { fn f() { claude_hidden(); } }",
            "#[cfg(test)] mod t { fn f() { let a = [1, 2]; claude_hidden(); } struct S { f: u8, } }",
            "#[cfg(test)] macro_rules! m { () => { claude_hidden() } }",
            "#[derive(Debug)] #[cfg(test)] fn h<A, B>() { claude_hidden(); }",
            "fn a() {}\n#[cfg(test)] fn helper(a: u8, b: u8) -> HashMap<A, B> { claude_hidden(); }",
        ] {
            assert!(seen(head).is_empty(), "{head}");
            assert_eq!(seen(&format!("{head}\nfn g() {{ claude() }}")), ["claude"], "{head}");
        }
        // An item that ends at a semicolon leaves the next function judged.
        for head in ["#[cfg(test)] type M = HashMap<K, V>;", "fn a() {}\n#[cfg(test)] const X: u8 = 1;", "#[cfg(test)]\nuse a::b;"] {
            assert_eq!(seen(&format!("{head} fn g() {{ claude() }}")), ["claude"], "{head}");
        }
    }

    #[test]
    fn a_cfg_test_closure_parameter_never_swallows_the_closure_body() {
        // The `|` that closes the parameter list ends the skip, whether the body has braces or not.
        assert_eq!(seen("let f = |a, #[cfg(test)] b| { \"claude.exe\" };"), ["claude.exe"]);
        assert_eq!(seen("let f = |a, #[cfg(test)] b| \"claude.exe\";"), ["claude.exe"]);
        assert_eq!(seen("let f = |a: u8, #[cfg(test)] b: u8| { let p = \"claude.exe\"; };"), ["claude.exe"]);
        assert_eq!(seen("let f = |#[cfg(test)] b| claude_run(b);"), ["claude_run"]);
        // A closure that is itself the skipped element ends at its second `|`; the rest is read as code.
        assert_eq!(seen("call(#[cfg(test)] |x| x, 1); fn claude() {}"), ["claude"]);
        assert_eq!(seen("call(#[cfg(test)] async || 1, \"claude.exe\");"), ["claude.exe"]);
    }

    #[test]
    fn a_block_never_belongs_to_a_skipped_element() {
        // The attribute on the last where predicate: the `{` after it is the body of the function.
        assert_eq!(seen("fn f<T>() where #[cfg(test)] T: Clone { let p = \"claude\"; }"), ["claude"]);
        assert_eq!(seen("fn f<T>() where T: A, #[cfg(test)] T: B { claude() }"), ["claude"]);
        assert!(seen("fn f<T>() where #[cfg(test)] T: Clone + Claude { 1 }").is_empty());
        // A braced variant or arm: the head is skipped, the braces are read as code (a name inside is reported).
        assert_eq!(seen("enum E { #[cfg(test)] A { claude: u8 }, B }"), ["claude"]);
        assert_eq!(seen("enum E { #[cfg(test)] A { x: u8 }, B }\nfn g() { claude() }"), ["claude"]);
        assert_eq!(seen("match x { #[cfg(test)] A => { 1 } _ => claude() }"), ["claude"]);
        assert_eq!(seen("match x { #[cfg(test)] A => { 1 } _ => { claude() } }"), ["claude"]);
    }

    #[test]
    fn an_attribute_inside_a_list_takes_the_element_rule_whatever_word_follows() {
        assert_eq!(seen("struct S(#[cfg(test)] fn(), u8); fn claude() {}"), ["claude"]);
        assert_eq!(seen("struct S(#[cfg(test)] fn() -> u8, ClaudeHook);"), ["ClaudeHook"]);
        assert_eq!(seen("struct S<#[cfg(test)] const N: usize, ClaudeMarker> {}"), ["ClaudeMarker"]);
        assert_eq!(seen("call(#[cfg(test)] fn() -> u8, claude());"), ["claude"]);
        assert_eq!(seen("call(#[cfg(test)] unsafe fn() -> u8, claude());"), ["claude"]);
        // A raw identifier is an identifier, not the keyword.
        assert_eq!(seen("fn f(#[cfg(test)] r#type: u8, x: u8) { let p = \"claude\"; }"), ["claude"]);
        assert_eq!(seen("run(#[cfg(test)] r#type, \"claude.exe\");"), ["claude.exe"]);
        assert_eq!(seen("match x { #[cfg(test)] r#type => 1, _ => claude() }"), ["claude"]);
        // Where an item can start, the item rule still holds: after `}`, after `;`, after another attribute.
        assert!(seen("fn a() {}\n#[cfg(test)] fn helper(a: u8, b: u8) -> HashMap<A, B> { \"Claude\" }").is_empty());
        assert_eq!(seen("fn a() {}\n#[cfg(test)] const X: u8 = 1; fn claude() {}"), ["claude"]);
        assert!(seen("#[derive(Debug)] #[cfg(test)] struct S<A, B> { claude: (A, B) }").is_empty());
        assert!(seen("use a::b;\n#[cfg(test)] type M = HashMap<K, Claude>;").is_empty());
    }

    #[test]
    fn a_field_named_like_a_contextual_keyword_is_still_a_field() {
        assert_eq!(seen("struct S { #[cfg(test)] union: u8, claude: u8 }"), ["claude"]);
        assert_eq!(seen("struct S { #[cfg(test)] macro_rules: u8, claude: u8 }"), ["claude"]);
        assert_eq!(seen("struct S { #[cfg(test)] pub(crate) f: (u8, u8), claude: u8 }"), ["claude"]);
        assert!(seen("#[cfg(test)] union U { claude: u8 }").is_empty());
        assert!(seen("#[cfg(test)] macro_rules! m { () => { claude } }").is_empty());
    }

    #[test]
    fn a_cfg_test_module_with_a_braced_body_is_still_skipped_whole() {
        assert!(seen("#[cfg(test)] mod t { fn claude(){} }").is_empty());
        assert!(seen("#[cfg(test)] mod t { fn f() { let a = [1, 2]; let s = \"Claude\"; } struct S { claude: u8, } }").is_empty());
        assert_eq!(seen("#[cfg(test)] mod t { fn f() {} }\nfn claude() {}"), ["claude"]);
        assert_eq!(seen("fn a() {}\n#[cfg(test)]\nuse claude::x;\nfn claude() {}"), ["claude"]);
    }

    #[test]
    fn other_cfg_test_items_are_skipped_up_to_their_end() {
        assert!(seen("#[cfg(test)]\nuse claude::x;\nfn ok() {}").is_empty());
        assert!(seen("#[cfg(test)]\n#[allow(dead_code)]\nfn claude_helper() { let s = \"Claude\"; }\nfn ok() {}").is_empty());
        assert_eq!(seen("#[cfg(not(test))]\nfn claude_helper() {}"), ["claude_helper"]);
    }

    #[test]
    fn a_number_or_character_literal_is_never_read_as_an_arrow() {
        // `= 3 >` closes the generic list, it is not `=>`.
        assert_eq!(seen("struct S<#[cfg(test)] const N: usize = 3> { claude: u8 }"), ["claude"]);
        assert_eq!(seen("struct S<#[cfg(test)] const N: i32 = -1> { claude: u8 }"), ["claude"]);
        assert_eq!(seen("struct S<#[cfg(test)] const C: char = 'a'> { claude: u8 }"), ["claude"]);
        assert_eq!(seen("struct S<#[cfg(test)] const C: u8 = b'a'> { claude: u8 }"), ["claude"]);
        assert_eq!(seen("#[cfg(test)] const N: usize = 3; fn f() -> bool { 1 > 0 } fn claude() {}"), ["claude"]);
        // A lifetime is no literal and no token.
        assert_eq!(seen("fn f<'a>(x: &'a str) { claude(x) }"), ["claude"]);
    }

    #[test]
    fn the_attributes_in_front_of_the_cfg_attribute_do_not_make_a_list_an_item_position() {
        // The `]` of `#[allow(x)]` is not an item position: the element ends at its comma and `claude()` after it is code.
        assert_eq!(seen("struct S(#[allow(x)] #[cfg(test)] fn() -> u8, u8); fn g() { claude() }"), ["claude"]);
        assert_eq!(seen("call(#[allow(x)] #[cfg(test)] fn() -> u8, claude());"), ["claude"]);
        assert_eq!(seen("call(#[a] #[b] #[cfg(test)] fn() -> u8, claude());"), ["claude"]);
        // After `;`, `{`, `}` or at the start of the file, attributes in front keep the position: the item rule holds.
        assert!(seen("#[allow(x)] #[cfg(test)] fn h<A, B>() { claude_hidden(); }").is_empty());
        assert!(seen("fn a() {}\n#[a] #[b] #[cfg(test)] fn h<A, B>() { claude_hidden(); }").is_empty());
        // A `]` that closes an index is no attribute: the position is not an item position.
        assert_eq!(seen("let a = v[0]; call(w[1] #[cfg(test)] fn() -> u8, claude());"), ["claude"]);
    }

    #[test]
    fn a_stacked_attribute_does_not_make_a_list_an_item_position() {
        assert_eq!(seen("struct S(#[allow(dead_code)] #[cfg(test)] fn() -> u8, ClaudeHook);"), ["ClaudeHook"]);
        assert_eq!(seen("call(#[allow(x)] #[cfg(test)] async || 1, \"claude.exe\");"), ["claude.exe"]);
        assert_eq!(seen("struct S<#[allow(x)] #[cfg(test)] const N: usize, ClaudeMarker> {}"), ["ClaudeMarker"]);
        assert_eq!(seen("call(w[1] #[cfg(test)] fn() -> u8, \"claude.exe\");"), ["claude.exe"]);
    }

    #[test]
    fn an_attribute_stack_in_front_of_an_item_keeps_the_item_position() {
        assert!(seen("#[derive(Debug)] #[cfg(test)] struct S<A, B> { claude: (A, B) }").is_empty());
        assert!(seen("fn a() {}\n#[a] #[b] #[cfg(test)] fn h<A, B>() { claude() }").is_empty());
        assert!(seen("use a::b;\n#[allow(x)] #[cfg(test)] type M = HashMap<K, Claude>;").is_empty());
        assert!(seen("mod m { #[allow(x)] #[cfg(test)] fn h<A, B>() { claude() } }").is_empty());
        // An inner attribute in front is walked back over as well.
        assert!(seen("#![allow(x)]\n#[cfg(test)] mod t { fn claude() {} }").is_empty());
    }

    #[test]
    fn skip_test_item_always_moves_forward() {
        // Every source of the tests above goes through `assert_moves_forward` (it is part of `seen`); these end at the
        // edge of the source or hold nothing but attributes.
        for src in [
            "#[cfg(test)]",
            "#[cfg(test)] #[allow(x)]",
            "#[cfg(test)] fn f(",
            "struct S { #[cfg(test)] f: u8 }",
            "fn f(#[cfg(test)] a: u8) {}",
            "let v = [#[cfg(test)] a];",
            "call(#[cfg(test)] |x| x);",
            "#[cfg(test)] }",
            "#[cfg(test)] ) ] >",
            "a #[cfg(test)] b #[cfg(test)] c",
            "#[cfg(test)] #[cfg(test)] #[cfg(test)] fn f() {}",
            "#[cfg(all(test, windows))] #[cfg(test)]",
        ] {
            assert_moves_forward(&tokenize(src));
        }
    }

    // What counts as test code.

    #[test]
    fn a_cfg_that_holds_only_in_test_builds_is_test_code() {
        for predicate in [
            "test",
            "all(test)",
            "all(test, windows)",
            "all(windows, test)",
            "all(test, not(windows))",
            "all(all(test, a), b)",
            "any(test, test)",
            " test ",
            "test,",
        ] {
            let src = format!("#[cfg({predicate})]\nfn claude_hidden() {{}}\nfn claude() {{}}");
            assert!(skip_test_item(&tokenize(&src), 0).is_some(), "{predicate}");
            assert_eq!(seen(&src), ["claude"], "{predicate}");
        }
    }

    #[test]
    fn a_cfg_that_can_hold_in_a_normal_build_is_judged() {
        for attribute in [
            "#[cfg(any(test, feature = \"fixtures\"))]",
            "#[cfg(any(test, windows))]",
            "#[cfg(any())]",
            "#[cfg(all())]",
            "#[cfg(not(test))]",
            "#[cfg(all(not(test), windows))]",
            "#[cfg(windows)]",
            "#[cfg(feature = \"test\")]",
            "#[cfg(test = \"x\")]",
            "#[cfg_attr(test, derive(Debug))]",
            "#[cfg(r#test)]",
        ] {
            assert!(skip_test_item(&tokenize(attribute), 0).is_none(), "{attribute}");
            assert_eq!(seen(&format!("{attribute}\nfn claude() {{}}")), ["claude"], "{attribute}");
        }
    }

    #[test]
    fn a_malformed_cfg_is_never_test_code() {
        for src in [
            "#[cfg(test",
            "#[cfg(all(test]",
            "#[cfg(all(test)]",
            "#[cfg(]",
            "#[cfg()]",
            "#[cfg(,)]",
            "#[cfg]",
            "#[cfg(test)",
            "#[cfg(test) junk]",
            "#[cfg(test junk)]",
            "#[cfg(all(test,, windows))]",
            "#[cfg(test = )]",
        ] {
            assert!(skip_test_item(&tokenize(src), 0).is_none(), "{src}");
        }
        // A predicate nested deeper than any real one is judged, and the scan does not overflow the stack.
        let deep = format!("#[cfg({}test{})]", "all(".repeat(200), ")".repeat(200));
        assert!(skip_test_item(&tokenize(&deep), 0).is_none());
    }

    #[test]
    fn an_inner_cfg_test_makes_the_whole_file_test_code() {
        for src in [
            "#![cfg(test)]\nfn t() {}",
            "//! doc line\n#![allow(dead_code)]\n#![cfg(test)]\nfn t() {}",
            "#![cfg(all(test, windows))]\nfn t() {}",
        ] {
            assert!(is_test_file(&tokenize(src)), "{src}");
        }
        for src in [
            "#![cfg(not(test))]\nfn t() {}",
            "#![cfg(any(test, feature = \"x\"))]\nfn t() {}",
            "#![cfg(windows)]\nfn t() {}",
            "fn a() {}\n#![cfg(test)]",
            "#[cfg(test)]\nfn t() {}",
        ] {
            assert!(!is_test_file(&tokenize(src)), "{src}");
        }
    }

    #[test]
    fn a_cfg_test_module_declaration_names_a_test_file() {
        for src in [
            "#[cfg(test)]\nmod tests;",
            "#[cfg(test)]\npub(crate) mod tests;",
            "#[cfg(test)]\npub mod tests;",
            "#[cfg(test)] #[allow(dead_code)] mod tests;",
            "#[allow(dead_code)] #[cfg(test)] mod tests;",
            "#[cfg(all(test, windows))]\nmod tests;",
        ] {
            assert_eq!(names(src), ["tests"], "{src}");
        }
        for src in [
            "mod tests;",
            "mod tests {}",
            "#[cfg(test)] mod tests {}",
            "mod a { #[cfg(test)] mod t; }",
            "#[cfg(not(test))] mod tests;",
            "#[cfg(any(test, feature = \"x\"))] mod tests;",
            "#[cfg(test)] fn tests() {}",
        ] {
            assert!(names(src).is_empty(), "{src}");
        }
    }

    #[test]
    fn a_module_file_that_ships_under_another_cfg_is_not_test_code() {
        assert!(names("#[cfg(test)] mod tests; #[cfg(not(test))] mod tests;").is_empty());
        assert!(names("#[cfg(test)] mod tests; mod tests;").is_empty());
        // Another name in the same file is not touched by it.
        assert_eq!(names("#[cfg(test)] mod tests; mod other;"), ["tests"]);
        assert_eq!(names("#[cfg(test)] mod tests; #[cfg(not(test))] mod other;"), ["tests"]);
        // A test-only item in front is no attribute of the declaration after it.
        assert!(names("#[cfg(test)] fn helper() {}\nmod tests;").is_empty());
        assert!(names("#[cfg(test)] use a::b;\nmod tests;").is_empty());
        assert!(names("#[cfg(test)] pub fn f() {}\nmod tests;").is_empty());
    }

    #[test]
    fn a_module_with_a_path_attribute_is_not_judged_by_its_name() {
        assert!(names("#[cfg(test)] #[path = \"x.rs\"] mod tests;").is_empty());
        assert!(names("#[path = \"x.rs\"] #[cfg(test)] mod tests;").is_empty());
        // A `cfg_attr` may carry the path.
        assert!(names("#[cfg(test)] #[cfg_attr(test, path = \"x.rs\")] mod tests;").is_empty());
    }
}
