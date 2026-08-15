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
    let cells = check(&block(CLEAN), Markers::empty()).expect("parses cleanly");

    assert!(matches!(cells.as_slice(), [Cell::Cst(_)]));
}

#[test]
fn an_error_block_gets_both_cells() {
    let cells = check(&block(ERRORS), Markers::ERROR).expect("errors as marked");

    let [Cell::Cst(_), Cell::Diagnostics(diagnostics)] = cells.as_slice() else {
        panic!("expected a cst cell and a diagnostics cell, got {cells:?}");
    };
    assert!(diagnostics.contains("Unterminated backtick-quoted name."));
}

#[test]
fn a_pending_block_that_parses_is_a_failure() {
    let error = check(&block(CLEAN), Markers::PENDING).expect_err("parses cleanly");

    assert!(matches!(error, CheckError::UnexpectedlyPasses));
}

#[test]
fn a_pending_block_that_fails_is_told_nothing_else() {
    let cells = check(&block(ERRORS), Markers::PENDING).expect("does not parse");

    assert_eq!(cells, vec![Cell::Pending]);
}

#[test]
fn an_unmarked_block_may_not_error() {
    let error = check(&block(ERRORS), Markers::empty()).expect_err("errors");

    assert!(matches!(error, CheckError::UnexpectedError(_)));
}

#[test]
fn an_error_block_must_error() {
    let error = check(&block(CLEAN), Markers::ERROR).expect_err("parses cleanly");

    assert!(matches!(error, CheckError::ExpectedError));
}

/// `[deviation]` changes nothing here
#[test]
fn deviation_changes_nothing() {
    let cells = check(&block(CLEAN), Markers::DEVIATION).expect("parses cleanly");

    assert!(matches!(cells.as_slice(), [Cell::Cst(_)]));
}
