// SOURCE PINS' TWO TOOLS, for the std-only rule modules' tests (2026-09-26).
//
// Not a module: each test module that pins source takes these with
// `include!("source_pins.rs")`, which resolves next to the including file both
// under cargo and under plain `rustc --test src/<file>.rs`. One copy of the
// comment stripper, so every pin matches CODE (CLAUDE.md: "strip comments
// before matching"), and one tree walk, so a ban sees every file in `src/`.

/// `source` with every comment gone: `//` to the end of the line (newline
/// kept) and `/* */`, nested, as one space. String, raw-string and char
/// literals are copied as they are, so `"https://…"` and `'"'` survive; a
/// lifetime (`'a`) is not a literal.
#[allow(dead_code)]
fn code(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' { i += 1; }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') { depth += 1; i += 2; }
                else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') { depth -= 1; i += 2; if depth == 0 { break; } }
                else { i += 1; }
            }
            out.push(' ');
        } else if c == 'r' && (i == 0 || !ident(chars[i - 1]) || (chars[i - 1] == 'b' && (i < 2 || !ident(chars[i - 2]))))
            && matches!(next, Some('"') | Some('#')) && chars[i + 1..].iter().find(|c| **c != '#') == Some(&'"') {
            // r"…", r#"…"#, br"…": no escapes, closed by a quote and as many #.
            let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
            let start = i;
            i += 2 + hashes;
            while i < chars.len() && !(chars[i] == '"' && chars[i + 1..].iter().take(hashes).filter(|c| **c == '#').count() == hashes) { i += 1; }
            i = (i + 1 + hashes).min(chars.len());
            out.extend(&chars[start..i]);
        } else if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' { if chars[i] == '\\' { i += 1; } i += 1; }
            i = (i + 1).min(chars.len());
            out.extend(&chars[start..i]);
        } else if c == '\'' && (next == Some('\\') || chars.get(i + 2) == Some(&'\'')) {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '\'' { if chars[i] == '\\' { i += 1; } i += 1; }
            i = (i + 1).min(chars.len());
            out.extend(&chars[start..i]);
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Every `.rs` file in `src/` as (file name, source), sorted by name. Under
/// cargo `src/` is the manifest's; under plain rustc it is this file's folder,
/// relative to where rustc ran (run the test binary from there).
#[allow(dead_code)]
fn rust_sources() -> Vec<(String, String)> {
    use std::path::{Path, PathBuf};
    let dir = match option_env!("CARGO_MANIFEST_DIR") {
        Some(root) => PathBuf::from(root).join("src"),
        None => match Path::new(file!()).parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let mut sources: Vec<(String, String)> = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| (path.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&path).expect("a source file")))
        .collect();
    sources.sort();
    sources
}

#[test]
fn the_stripper_keeps_code_and_literals() {
    assert_eq!(code("a // gone\nb"), "a \nb");
    assert_eq!(code("a /* x /* nested */ y */ b"), "a   b");
    assert_eq!(code("let u = \"https://x\"; // c"), "let u = \"https://x\"; ");
    assert_eq!(code("let s = \"say \\\"/*\\\" \"; t"), "let s = \"say \\\"/*\\\" \"; t");
    assert_eq!(code("let q = '\"'; // c\nlet r = '\\''; x"), "let q = '\"'; \nlet r = '\\''; x");
    assert_eq!(code("fn f<'a>(s: &'a str) {} // c"), "fn f<'a>(s: &'a str) {} ");
    assert_eq!(code("let p = r#\"a // \"b\" \"#; // c"), "let p = r#\"a // \"b\" \"#; ");
    assert_eq!(code("/// doc\n//! inner\nfn x() {}"), "\n\nfn x() {}");
    assert_eq!(code("error // c"), "error ");
}
