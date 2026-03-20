use crate::types::{DeclInfo, DeclKind};
use regex::Regex;
use std::sync::LazyLock;

static RE_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^import\s+([\w.]+)").unwrap());

static RE_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^((?:private|protected|noncomputable|partial|unsafe)\s+)*(theorem|lemma|def|instance|structure|class|inductive|abbrev|axiom|opaque)\s+([\w.]+)",
    )
    .unwrap()
});

static RE_SORRY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsorry\b").unwrap());

/// Strip all Lean block comments (including nested).
/// Returns a new string with comments replaced by whitespace (preserving line count).
/// Optimized: works on bytes directly (Lean source is ASCII-heavy, multi-byte chars
/// never start with `/-` or `-/` or `--` or `"` so byte scanning is safe for delimiters).
pub fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    let mut depth = 0u32;

    while i < len {
        // Line comment at depth 0
        if depth == 0 && i + 1 < len && bytes[i] == b'-' && bytes[i + 1] == b'-' {
            // Check for doc comment `/--`
            if !out.is_empty() && *out.last().unwrap() == b'/' {
                *out.last_mut().unwrap() = b' ';
                depth = 1;
                out.push(b' ');
                out.push(b' ');
                i += 2;
                continue;
            }
            // Regular line comment
            while i < len && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        // Block comment open: `/-`
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'-' {
            depth += 1;
            out.push(b' ');
            out.push(b' ');
            i += 2;
            continue;
        }
        // Block comment close: `-/`
        if depth > 0 && i + 1 < len && bytes[i] == b'-' && bytes[i + 1] == b'/' {
            depth -= 1;
            out.push(b' ');
            out.push(b' ');
            i += 2;
            continue;
        }
        // Inside block comment
        if depth > 0 {
            out.push(if bytes[i] == b'\n' { b'\n' } else { b' ' });
            i += 1;
            continue;
        }
        // String literal
        if bytes[i] == b'"' {
            out.push(b'"');
            i += 1;
            while i < len && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < len {
                    out.push(bytes[i]);
                    out.push(bytes[i + 1]);
                    i += 2;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            if i < len {
                out.push(b'"');
                i += 1;
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Safety: we only manipulated ASCII delimiter bytes; all multi-byte sequences pass through intact
    unsafe { String::from_utf8_unchecked(out) }
}

/// Extract import module names from source.
pub fn extract_imports(src: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(caps) = RE_IMPORT.captures(trimmed) {
            imports.push(caps[1].to_string());
        }
        // Imports must be at the top; stop after first non-import, non-blank, non-comment line
        if !trimmed.is_empty()
            && !trimmed.starts_with("import")
            && !trimmed.starts_with("--")
            && !trimmed.starts_with("/-")
            && !trimmed.starts_with("open")
            && !trimmed.starts_with("set_option")
            && !trimmed.starts_with("section")
            && !trimmed.starts_with("namespace")
            && !trimmed.starts_with("universe")
            && !trimmed.starts_with('#')
        {
            // Could be a declaration — stop scanning for imports
            // But be lenient: there can be `open` and `namespace` between imports
        }
    }
    imports
}

/// Count sorry occurrences in comment-stripped source.
pub fn count_sorrys(stripped: &str) -> usize {
    RE_SORRY.find_iter(stripped).count()
}

/// Find line numbers of all sorry occurrences in comment-stripped source.
pub fn find_sorry_lines(stripped: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(stripped.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    for m in RE_SORRY.find_iter(stripped) {
        let offset = m.start();
        let line = line_starts.partition_point(|&s| s <= offset);
        lines.push(line); // 1-indexed
    }
    lines
}

/// Extract declarations from comment-stripped source.
/// Optimized: collect all match positions first, then assign line_end from the next match.
pub fn extract_declarations(stripped: &str) -> Vec<DeclInfo> {
    // Pre-compute line start offsets for O(1) line number lookup
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(stripped.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let total_lines = line_starts.len();

    // Collect all declaration matches with their byte offsets
    let mut raw: Vec<(usize, String, DeclKind)> = Vec::new();
    for caps in RE_DECL.captures_iter(stripped) {
        let keyword = &caps[2];
        let name = caps[3].to_string();
        if let Some(kind) = DeclKind::from_keyword(keyword) {
            let offset = caps.get(0).unwrap().start();
            raw.push((offset, name, kind));
        }
    }

    let mut decls = Vec::with_capacity(raw.len());
    for (idx, (offset, name, kind)) in raw.iter().enumerate() {
        let line_start = line_starts.partition_point(|&s| s <= *offset);

        // line_end = line of the NEXT declaration (or end of file)
        let line_end = if idx + 1 < raw.len() {
            line_starts.partition_point(|&s| s <= raw[idx + 1].0)
        } else {
            total_lines
        };

        // Check for sorry within this declaration's byte range
        let end_offset = if idx + 1 < raw.len() {
            raw[idx + 1].0
        } else {
            stripped.len()
        };
        let decl_slice = &stripped[*offset..end_offset];
        let has_sorry = RE_SORRY.is_match(decl_slice);

        decls.push(DeclInfo {
            name: name.clone(),
            kind: *kind,
            line_start,
            line_end,
            has_sorry,
        });
    }
    decls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_line_comments() {
        let src = "def foo := 42 -- this is a comment\ndef bar := 7";
        let stripped = strip_comments(src);
        assert!(!stripped.contains("this is a comment"));
        assert!(stripped.contains("def foo"));
        assert!(stripped.contains("def bar"));
    }

    #[test]
    fn test_strip_nested_block_comments() {
        let src = "/- outer /- inner -/ still outer -/ visible";
        let stripped = strip_comments(src);
        assert!(stripped.contains("visible"));
        assert!(!stripped.contains("inner"));
        assert!(!stripped.contains("outer"));
    }

    #[test]
    fn test_sorry_not_in_comment() {
        let src = "/- sorry -/\ndef foo := sorry";
        let stripped = strip_comments(src);
        assert_eq!(count_sorrys(&stripped), 1);
    }

    #[test]
    fn test_sorry_in_identifier_counts() {
        // `sorry` as a standalone word should match; `sorryHelper` should not
        // Our regex uses \b which handles this correctly
        let src = "def sorryHelper := 1\ndef bar := sorry";
        let stripped = strip_comments(src);
        assert_eq!(count_sorrys(&stripped), 1);
    }

    #[test]
    fn test_extract_imports() {
        let src = "import Mathlib.Tactic\nimport HeytingLean.Core\n\ndef foo := 1";
        let imports = extract_imports(src);
        assert_eq!(imports, vec!["Mathlib.Tactic", "HeytingLean.Core"]);
    }

    #[test]
    fn test_extract_declarations() {
        let src = "theorem myThm : True := trivial\n\ndef myDef := 42\n\nlemma myLemma : False := sorry";
        let stripped = strip_comments(src);
        let decls = extract_declarations(&stripped);
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].name, "myThm");
        assert_eq!(decls[0].kind, DeclKind::Theorem);
        assert!(!decls[0].has_sorry);
        assert_eq!(decls[1].name, "myDef");
        assert_eq!(decls[1].kind, DeclKind::Def);
        assert_eq!(decls[2].name, "myLemma");
        assert_eq!(decls[2].kind, DeclKind::Lemma);
        assert!(decls[2].has_sorry);
    }
}
