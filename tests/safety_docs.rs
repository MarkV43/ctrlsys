//! Enforces `specs/mission.md` Article 3 for the `unsafe` items clippy cannot see.
//!
//! `clippy::undocumented_unsafe_blocks` covers `// SAFETY:` on unsafe *blocks* and
//! *impls*, and `clippy::missing_safety_doc` covers `# Safety` on *exported* `unsafe fn`
//! and `unsafe trait` declarations. Neither covers a private or `pub(crate)`
//! declaration — which is precisely where this crate keeps its byte-cast helpers, whose
//! contract every other safety argument depends on.
//!
//! This test walks `src/` and fails on any `unsafe fn` or `unsafe trait` whose attached
//! doc block lacks a `# Safety` heading, at any visibility.

use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// True if the doc block immediately above `idx` carries a `# Safety` heading.
///
/// Walks upward through contiguous doc-comment and attribute lines. A blank line or a
/// line of code ends the block, because a doc comment separated by a blank line does
/// not attach to the item.
fn has_safety_section(lines: &[&str], idx: usize) -> bool {
    for line in lines[..idx].iter().rev() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("#[") || trimmed.starts_with("//") {
            if trimmed.contains("# Safety") {
                return true;
            }
        } else {
            return false;
        }
    }
    false
}

fn is_unsafe_decl(trimmed: &str) -> bool {
    ["unsafe fn ", "unsafe trait "].iter().any(|kw| {
        trimmed.starts_with(kw)
            || trimmed.starts_with(&format!("pub {kw}"))
            || trimmed.starts_with(&format!("pub(crate) {kw}"))
            || trimmed.starts_with(&format!("pub(super) {kw}"))
    })
}

#[test]
fn every_unsafe_item_documents_its_safety_contract() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no sources found under {}", src.display());

    let mut offenders = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).expect("source file is readable UTF-8");
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if is_unsafe_decl(trimmed) && !has_safety_section(&lines, i) {
                let rel = path.strip_prefix(&src).unwrap_or(path);
                offenders.push(format!("src/{}:{}  {trimmed}", rel.display(), i + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "`unsafe` item without a `# Safety` doc section (specs/mission.md Article 3):\n  {}",
        offenders.join("\n  ")
    );
}
