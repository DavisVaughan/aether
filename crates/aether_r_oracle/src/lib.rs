mod shape;

use crate::shape::{Shape, shape_of};
use aether_r_syntax::RSyntaxNode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleResult {
    Match,
    Mismatch {
        path: String,
        expected: String,
        actual: String,
    },
    OracleUnavailable(String),
}

pub fn compare(source: &str, actual: &RSyntaxNode) -> OracleResult {
    let reference = air_r_parser::parse(source, air_r_parser::RParserOptions::default());

    if let Some(error) = reference.error() {
        return OracleResult::OracleUnavailable(error.to_string());
    }

    let expected = shape_of(&reference.syntax());
    let actual = shape_of(actual);

    match difference(&expected, &actual, &mut Vec::new()) {
        Some(result) => result,
        None => OracleResult::Match,
    }
}

fn difference(expected: &Shape, actual: &Shape, path: &mut Vec<String>) -> Option<OracleResult> {
    if expected.kind() != actual.kind() || expected.range() != actual.range() {
        return Some(mismatch(expected, actual, path));
    }

    match (expected, actual) {
        (
            Shape::Node {
                children: expected_children,
                ..
            },
            Shape::Node {
                children: actual_children,
                ..
            },
        ) => {
            path.push(expected.kind().to_string());

            for index in 0..expected_children.len().max(actual_children.len()) {
                let found = match (expected_children.get(index), actual_children.get(index)) {
                    (Some(expected_child), Some(actual_child)) => {
                        path.push(format!("[{index}]"));
                        let found = difference(expected_child, actual_child, path);
                        path.pop();
                        found
                    }
                    (Some(expected_child), None) => Some(missing(expected_child, index, path)),
                    (None, Some(actual_child)) => Some(unexpected(actual_child, index, path)),
                    (None, None) => None,
                };

                if found.is_some() {
                    path.pop();
                    return found;
                }
            }

            path.pop();
            None
        }
        (
            Shape::Token {
                leading_comments: expected_leading,
                trailing_comments: expected_trailing,
                ..
            },
            Shape::Token {
                leading_comments: actual_leading,
                trailing_comments: actual_trailing,
                ..
            },
        ) => {
            if expected_leading != actual_leading {
                return Some(comment_mismatch(
                    expected,
                    "leading",
                    expected_leading,
                    actual_leading,
                    path,
                ));
            }

            if expected_trailing != actual_trailing {
                return Some(comment_mismatch(
                    expected,
                    "trailing",
                    expected_trailing,
                    actual_trailing,
                    path,
                ));
            }

            None
        }
        _ => Some(mismatch(expected, actual, path)),
    }
}

fn mismatch(expected: &Shape, actual: &Shape, path: &[String]) -> OracleResult {
    OracleResult::Mismatch {
        path: render_path(path, expected.kind()),
        expected: expected.describe(),
        actual: actual.describe(),
    }
}

fn missing(expected: &Shape, index: usize, path: &[String]) -> OracleResult {
    OracleResult::Mismatch {
        path: render_path(path, &format!("[{index}]")),
        expected: expected.describe(),
        actual: "nothing".to_string(),
    }
}

fn unexpected(actual: &Shape, index: usize, path: &[String]) -> OracleResult {
    OracleResult::Mismatch {
        path: render_path(path, &format!("[{index}]")),
        expected: "nothing".to_string(),
        actual: actual.describe(),
    }
}

fn comment_mismatch(
    token: &Shape,
    position: &str,
    expected: &[String],
    actual: &[String],
    path: &[String],
) -> OracleResult {
    OracleResult::Mismatch {
        path: render_path(path, &format!("{} {position} comments", token.kind())),
        expected: format!("{expected:?}"),
        actual: format!("{actual:?}"),
    }
}

fn render_path(path: &[String], leaf: &str) -> String {
    let mut segments: Vec<&str> = path.iter().map(String::as_str).collect();
    segments.push(leaf);
    segments.join(" > ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_r_parser::RParserOptions;

    fn oracle(source: &str) -> OracleResult {
        let parsed = aether_r_parser::parse(source, RParserOptions::default());
        compare(source, &parsed.syntax())
    }

    #[test]
    fn an_empty_file_matches() {
        assert_eq!(oracle(""), OracleResult::Match);
    }

    #[test]
    fn whitespace_only_matches() {
        assert_eq!(oracle("  \n\n  "), OracleResult::Match);
    }

    /// Deviation 8: tree-sitter re-splits `0x` into `0` and the name `x`, where
    /// aether keeps one literal and diagnoses it
    #[test]
    fn a_shape_difference_is_reported() {
        let OracleResult::Mismatch {
            path,
            expected,
            actual,
        } = oracle("0x")
        else {
            panic!("tree-sitter cannot agree with aether on `0x`");
        };

        assert_eq!(
            path,
            "R_ROOT > [0] > R_STATEMENT_LIST > [0] > R_DOUBLE_VALUE"
        );
        assert!(expected.contains("0..1"), "{expected}");
        assert!(actual.contains("0..2"), "{actual}");
    }

    #[test]
    fn a_source_tree_sitter_rejects_is_unavailable() {
        assert!(matches!(oracle("f("), OracleResult::OracleUnavailable(_)));
    }

    #[test]
    fn a_missing_extract_rhs_is_not_an_error_to_tree_sitter() {
        assert!(!matches!(oracle("a$"), OracleResult::OracleUnavailable(_)));
    }

    #[test]
    fn r_statement_wrappers_are_unwrapped() {
        let parsed = aether_r_parser::parse("1", RParserOptions::default());
        let shape = shape_of(&parsed.syntax());

        assert!(!flatten(&shape).contains(&"R_STATEMENT".to_string()));
    }

    #[test]
    fn air_list_kinds_are_renamed_to_aether_s() {
        let reference = air_r_parser::parse("1", air_r_parser::RParserOptions::default());
        let kinds = flatten(&shape_of(&reference.syntax()));

        assert!(kinds.contains(&"R_STATEMENT_LIST".to_string()), "{kinds:?}");
        assert!(
            !kinds.contains(&"R_EXPRESSION_LIST".to_string()),
            "{kinds:?}"
        );
    }

    #[test]
    fn braced_expressions_are_renamed_to_braced_statements() {
        let reference = air_r_parser::parse("{ 1 }", air_r_parser::RParserOptions::default());
        let kinds = flatten(&shape_of(&reference.syntax()));

        assert!(
            kinds.contains(&"R_BRACED_STATEMENTS".to_string()),
            "{kinds:?}"
        );
        assert!(
            !kinds.contains(&"R_BRACED_EXPRESSIONS".to_string()),
            "{kinds:?}"
        );
    }

    #[test]
    fn string_values_collapse_to_one_token_on_both_sides() {
        let reference = air_r_parser::parse("\"a\"", air_r_parser::RParserOptions::default());
        let expected = flatten(&shape_of(&reference.syntax()));

        let parsed = aether_r_parser::parse("\"a\"", RParserOptions::default());
        let actual = flatten(&shape_of(&parsed.syntax()));

        assert_eq!(expected, actual);
        assert!(expected.contains(&"STRING".to_string()), "{expected:?}");
        assert!(
            !expected.contains(&"STRING_OPEN".to_string()),
            "{expected:?}"
        );
    }

    #[test]
    fn semicolons_are_dropped() {
        let reference = air_r_parser::parse("{ 1; 2 }", air_r_parser::RParserOptions::default());

        assert!(!flatten(&shape_of(&reference.syntax())).contains(&"SEMICOLON".to_string()));
    }

    #[test]
    fn whitespace_polarity_does_not_reach_the_comparison() {
        let spaced = air_r_parser::parse("1 + 1", air_r_parser::RParserOptions::default());
        let tight = air_r_parser::parse("1+1", air_r_parser::RParserOptions::default());

        assert_eq!(
            flatten(&shape_of(&spaced.syntax())),
            flatten(&shape_of(&tight.syntax()))
        );
    }

    #[test]
    fn comments_are_compared_and_whitespace_is_not() {
        let commented = air_r_parser::parse("1 # note\n", air_r_parser::RParserOptions::default());
        let bare = air_r_parser::parse("1\n", air_r_parser::RParserOptions::default());

        assert!(
            difference(
                &shape_of(&commented.syntax()),
                &shape_of(&bare.syntax()),
                &mut Vec::new()
            )
            .is_some()
        );
    }

    fn flatten(shape: &Shape) -> Vec<String> {
        let mut kinds = vec![shape.kind().to_string()];

        if let Shape::Node { children, .. } = shape {
            for child in children {
                kinds.extend(flatten(child));
            }
        }

        kinds
    }
}
