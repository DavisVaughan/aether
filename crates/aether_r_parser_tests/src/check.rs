use crate::document::{Block, Cell, Markers};
use aether_r_parser::{RParse, RParserOptions, parse};
use aether_r_syntax::{RSyntaxNode, TextRange};
use biome_diagnostics::Diagnostic;
use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::{Language, SyntaxKind, SyntaxNode, SyntaxSlot};

#[derive(Debug)]
pub enum CheckError {
    RoundTrip { expected: String, got: String },
    UnexpectedlyPasses,
    ExpectedError,
    UnexpectedError(String),
    MissingRequired(String),
    BogusNodes(String),
    UndiagnosedBogusNode { ranges: Vec<TextRange>, ast: String },
}

/// One code block to check
///
/// Emits CST and diagnostic cells to write in the snapshot
pub fn check(block: &Block, markers: Markers) -> Result<Vec<Cell>, CheckError> {
    let parsed = parse(&block.source, RParserOptions::default());

    let round_tripped = parsed.syntax().to_string();
    if round_tripped != block.source {
        return Err(CheckError::RoundTrip {
            expected: block.source.clone(),
            got: round_tripped,
        });
    }

    let has_errors = parsed.has_errors();
    let has_bogus = has_bogus_nodes_or_empty_slots(&parsed.syntax());

    if markers.contains(Markers::PENDING) {
        if !has_errors && !has_bogus {
            return Err(CheckError::UnexpectedlyPasses);
        }
        return Ok(vec![Cell::Pending]);
    }

    if markers.contains(Markers::ERROR) {
        if !has_errors {
            return Err(CheckError::ExpectedError);
        }
    } else {
        if has_errors {
            return Err(CheckError::UnexpectedError(render_diagnostics(
                &parsed,
                &block.source,
            )));
        }

        let ast = format!("{:#?}", parsed.tree());
        // `has_bogus_nodes_or_empty_slots` only inspects empty slots of *list*
        // nodes, so a missing required slot elsewhere needs the string
        if ast.contains("missing (required)") {
            return Err(CheckError::MissingRequired(ast));
        }
        if has_bogus {
            return Err(CheckError::BogusNodes(ast));
        }
    }

    // Invariant 2, per-node rather than "are there any diagnostics at all" —
    // the whole-parse form is satisfied by the diagnostic that masks the problem
    let unexplained = undiagnosed_bogus_nodes(&parsed.syntax(), parsed.diagnostics());
    if !unexplained.is_empty() {
        return Err(CheckError::UndiagnosedBogusNode {
            ranges: unexplained
                .iter()
                .map(SyntaxNode::text_trimmed_range)
                .collect(),
            ast: format!("{:#?}", parsed.tree()),
        });
    }

    let mut cells = vec![Cell::Cst(format!("{:#?}", parsed.tree()))];
    if !parsed.diagnostics().is_empty() {
        cells.push(Cell::Diagnostics(render_diagnostics(
            &parsed,
            &block.source,
        )));
    }

    Ok(cells)
}

/// Bogus nodes that no diagnostic points at
///
/// Overlap, not equality: a recovery that consumes three tokens may
/// legitimately be explained by a diagnostic anchored on only the first.
fn undiagnosed_bogus_nodes(
    root: &RSyntaxNode,
    diagnostics: &[ParseDiagnostic],
) -> Vec<RSyntaxNode> {
    // Primary ranges only. A `with_detail` label lives in the advices, and a
    // diagnostic that merely gestures at a bogus node does not explain it.
    let spans: Vec<TextRange> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.location().span)
        .collect();

    root.descendants()
        .filter(|node| node.kind().is_bogus())
        .filter(|node| {
            let range = node.text_trimmed_range();
            !spans.iter().any(|span| span.intersect(range).is_some())
        })
        .collect()
}

/// Copied from air's `crates/air_r_parser/tests/spec_test.rs`
///
/// The empty-slot half only covers list nodes, which is why [`check()`] greps
/// for `missing (required)` as well
fn has_bogus_nodes_or_empty_slots<L: Language>(node: &SyntaxNode<L>) -> bool {
    node.descendants().any(|descendant| {
        let kind = descendant.kind();
        if kind.is_bogus() {
            return true;
        }

        if kind.is_list() {
            return descendant
                .slots()
                .any(|slot| matches!(slot, SyntaxSlot::Empty { .. }));
        }

        false
    })
}

pub fn render_diagnostics(parse: &RParse, source: &str) -> String {
    use biome_console::fmt::{Formatter, Termcolor};
    use biome_console::markup;
    use biome_diagnostics::{DiagnosticExt, PrintDiagnostic, termcolor::NoColor};

    let mut out = Vec::new();

    for diagnostic in parse.diagnostics() {
        let diagnostic = diagnostic
            .clone()
            .with_file_path("test.R")
            .with_file_source_code(source);

        Formatter::new(&mut Termcolor(NoColor::new(&mut out)))
            .write_markup(markup! { {PrintDiagnostic::verbose(&diagnostic)} })
            .expect("writing a diagnostic to a string cannot fail");
    }

    let rendered = String::from_utf8(out).expect("rendered diagnostics are UTF-8");

    // Biome pads its frames out to the terminal width. Committed snapshots have
    // to survive an editor that strips trailing whitespace.
    let mut trimmed = String::with_capacity(rendered.len());
    for line in rendered.lines() {
        trimmed.push_str(line.trim_end());
        trimmed.push('\n');
    }

    trimmed.truncate(trimmed.trim_end().len());
    trimmed.push('\n');
    trimmed
}

#[cfg(test)]
mod tests;
