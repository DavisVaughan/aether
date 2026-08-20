use crate::document::{Block, Cell, Markers};
use aether_r_parser::{RParse, RParserOptions, parse};
use aether_r_syntax::{RSyntaxNode, TextRange};
use biome_console::fmt::{Formatter, Termcolor};
use biome_console::markup;
use biome_diagnostics::{Diagnostic, DiagnosticExt, PrintDiagnostic, termcolor::NoColor};
use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::{SyntaxKind, SyntaxNode};

#[derive(Debug)]
pub enum CheckError {
    RoundTrip {
        expected: String,
        got: String,
    },
    UnexpectedlyPasses,
    ExpectedError,
    UnexpectedError(String),
    BogusNodes(String),
    UndiagnosedBogusNode {
        ranges: Vec<TextRange>,
        ast: String,
    },
    OracleMismatch {
        path: String,
        expected: String,
        actual: String,
    },
}

/// One code block to check
///
/// Emits CST and diagnostic cells to write in the snapshot
pub fn check(block: &Block, markers: Markers) -> Result<Vec<Cell>, CheckError> {
    let parsed = parse(&block.source, RParserOptions::default());
    let ast = format!("{:#?}", parsed.tree());

    let round_tripped = parsed.syntax().to_string();
    if round_tripped != block.source {
        return Err(CheckError::RoundTrip {
            expected: block.source.clone(),
            got: round_tripped,
        });
    }

    let has_errors = parsed.has_errors();
    let has_bogus = has_bogus_nodes_or_missing_required_children(&ast, &parsed.syntax());

    if markers.pending {
        if !has_errors && !has_bogus {
            return Err(CheckError::UnexpectedlyPasses);
        }
        return Ok(vec![Cell::Pending]);
    }

    if markers.error {
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

        if has_bogus {
            return Err(CheckError::BogusNodes(ast));
        }
    }

    let unexplained = undiagnosed_bogus_nodes(&parsed.syntax(), parsed.diagnostics());
    if !unexplained.is_empty() {
        return Err(CheckError::UndiagnosedBogusNode {
            ranges: unexplained
                .iter()
                .map(SyntaxNode::text_trimmed_range)
                .collect(),
            ast,
        });
    }

    if !has_errors
        && !markers.deviation
        && let aether_r_oracle::OracleResult::Mismatch {
            path,
            expected,
            actual,
        } = aether_r_oracle::compare(&block.source, &parsed.syntax())
    {
        return Err(CheckError::OracleMismatch {
            path,
            expected,
            actual,
        });
    }

    let mut cells = vec![Cell::Cst(ast)];
    if !parsed.diagnostics().is_empty() {
        cells.push(Cell::Diagnostics(render_diagnostics(
            &parsed,
            &block.source,
        )));
    }

    Ok(cells)
}

fn undiagnosed_bogus_nodes(
    root: &RSyntaxNode,
    diagnostics: &[ParseDiagnostic],
) -> Vec<RSyntaxNode> {
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

fn has_bogus_nodes_or_missing_required_children(ast: &str, root: &RSyntaxNode) -> bool {
    ast.contains("missing (required)") || root.descendants().any(|node| node.kind().is_bogus())
}

fn render_diagnostics(parse: &RParse, source: &str) -> String {
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
mod tests {
    use super::*;

    const CLEAN: &str = "# a comment\n";

    /// Causes a lexer diagnostic
    const ERRORS: &str = "`a\n";

    fn block(source: &str) -> Block {
        Block {
            source: source.to_string(),
            generated: Vec::new(),
        }
    }

    #[test]
    fn a_clean_block_gets_a_cst_cell_and_no_diagnostics_cell() {
        let cells = check(&block(CLEAN), Markers::default()).expect("parses cleanly");

        assert!(matches!(cells.as_slice(), [Cell::Cst(_)]));
    }

    #[test]
    fn an_error_block_gets_both_cells() {
        let cells = check(
            &block(ERRORS),
            Markers {
                error: true,
                ..Default::default()
            },
        )
        .expect("errors as marked");

        let [Cell::Cst(_), Cell::Diagnostics(diagnostics)] = cells.as_slice() else {
            panic!("expected a cst cell and a diagnostics cell, got {cells:?}");
        };
        assert!(diagnostics.contains("Unterminated backtick-quoted name."));
    }

    #[test]
    fn a_pending_block_that_parses_is_a_failure() {
        let error = check(
            &block(CLEAN),
            Markers {
                pending: true,
                ..Default::default()
            },
        )
        .expect_err("parses cleanly");

        assert!(matches!(error, CheckError::UnexpectedlyPasses));
    }

    #[test]
    fn a_pending_block_that_fails_is_told_nothing_else() {
        let cells = check(
            &block(ERRORS),
            Markers {
                pending: true,
                ..Default::default()
            },
        )
        .expect("does not parse");

        assert_eq!(cells, vec![Cell::Pending]);
    }

    #[test]
    fn an_unmarked_block_may_not_error() {
        let error = check(&block(ERRORS), Markers::default()).expect_err("errors");

        assert!(matches!(error, CheckError::UnexpectedError(_)));
    }

    #[test]
    fn an_error_block_must_error() {
        let error = check(
            &block(CLEAN),
            Markers {
                error: true,
                ..Default::default()
            },
        )
        .expect_err("parses cleanly");

        assert!(matches!(error, CheckError::ExpectedError));
    }

    /// `0x` is deviation 8: one literal here, `0` and the name `x` to
    /// tree-sitter, so a comparison would report a mismatch
    #[test]
    fn a_block_that_does_not_parse_cleanly_is_not_compared_to_the_oracle() {
        check(
            &block("0x\n"),
            Markers {
                error: true,
                ..Default::default()
            },
        )
        .expect("tree-sitter accepts `0x`, and the shapes differ");
    }

    /// `[deviation]` changes nothing here
    #[test]
    fn deviation_changes_nothing() {
        let cells = check(
            &block(CLEAN),
            Markers {
                deviation: true,
                ..Default::default()
            },
        )
        .expect("parses cleanly");

        assert!(matches!(cells.as_slice(), [Cell::Cst(_)]));
    }
}
