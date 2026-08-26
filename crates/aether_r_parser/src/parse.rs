use crate::RLosslessTreeSink;
use crate::options::RParserOptions;
use crate::parser::RParser;
use aether_r_syntax::RRoot;
use aether_r_syntax::RSyntaxNode;
use biome_parser::AnyParse;
use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::AstNode;
use biome_rowan::NodeCache;
use biome_rowan::SendNode;

/// A utility struct for managing the result of a parser job
///
/// This struct holds a handle to the root node of the parsed syntax tree, along
/// with the diagnostics the parser emitted while producing it.
///
/// It can be dynamically downcast into a concrete [RSyntaxNode] or [RRoot].
///
/// # Why a [SendNode] not an [RSyntaxNode]?
///
/// Every other Biome parser stores its own language's node and converts to
/// [SendNode] at the [AnyParse] boundary, which is the only place biome needs
/// a tree to cross a thread. Air never reaches that boundary: its LSP keeps
/// an [RParse] in every `Document` and clones the whole `WorldState` into
/// `spawn_blocking`, whose handlers are `Send + 'static`. So [RParse] itself
/// has to be [Send], and [RSyntaxNode] is not — it holds an `Rc`.
///
/// The price is that [`RParse::syntax()`] rebuilds a red root on every call
/// instead of bumping a refcount. Cheap, but not free, so hold the result
/// rather than calling it in a loop.
///
/// TODO: We should probably investigate if we really need this in Air and Ark
/// so we can be more like Biome
#[derive(Clone, Debug)]
pub struct RParse {
    root: SendNode,
    diagnostics: Vec<ParseDiagnostic>,
}

impl RParse {
    fn new(root: RSyntaxNode, diagnostics: Vec<ParseDiagnostic>) -> Self {
        // Safety: This method is not exposed, we only use it internally
        let root = root.as_send().unwrap();
        Self { root, diagnostics }
    }

    /// The syntax node represented by this parse result
    pub fn syntax(&self) -> RSyntaxNode {
        self.root
            .clone()
            .into_node()
            .unwrap_or_else(|| panic!("Could not downcast root node to R language"))
    }

    /// Convert this parse result into a typed AST node
    pub fn tree(&self) -> RRoot {
        RRoot::unwrap_cast(self.syntax())
    }

    /// Every diagnostic the parser emitted, in source order
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Every diagnostic the parser emitted, consuming the result
    pub fn into_diagnostics(self) -> Vec<ParseDiagnostic> {
        self.diagnostics
    }

    /// Returns [true] if the parser encountered some errors during parsing
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::is_error)
    }
}

impl From<RParse> for AnyParse {
    fn from(parse: RParse) -> Self {
        Self::new(parse.root, parse.diagnostics)
    }
}

/// Parses `text`
pub fn parse(text: &str, options: RParserOptions) -> RParse {
    let mut cache = NodeCache::default();
    parse_with_cache(text, options, &mut cache)
}

/// Parses `text`, structurally sharing what it can with the trees already in
/// `cache`
fn parse_with_cache(text: &str, options: RParserOptions, cache: &mut NodeCache) -> RParse {
    let mut parser = RParser::new(text, options);
    crate::syntax::parse_root(&mut parser);
    let (events, trivia, diagnostics) = parser.finish();

    let mut tree_sink = RLosslessTreeSink::with_cache(text, &trivia, cache);
    biome_parser::event::process(&mut tree_sink, events, diagnostics);
    let (green, diagnostics) = tree_sink.finish();

    RParse::new(green, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_r_syntax::RSyntaxKind;
    use biome_diagnostics::PrintDescription;
    use biome_rowan::AstNodeList;

    /// Air's LSP currently needs this
    #[test]
    fn parse_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<RParse>();
    }

    #[test]
    fn parse_empty() {
        let parse = parse("", RParserOptions::default());
        assert!(parse.diagnostics().is_empty());
        assert_eq!(parse.syntax().to_string(), "");

        let root = parse.tree();
        assert!(root.statements().is_empty());
        assert_eq!(root.eof_token().unwrap().kind(), RSyntaxKind::EOF);
    }

    /// Trivia with no token to trail lands on `EOF`
    #[test]
    fn parse_trivia_only() {
        let source = "   \n# hi\n";
        let parse = parse(source, RParserOptions::default());
        assert!(parse.diagnostics().is_empty());
        assert_eq!(parse.syntax().to_string(), source);

        let eof = parse.tree().eof_token().unwrap();
        assert_eq!(eof.leading_trivia().text(), source);
        assert_eq!(eof.trailing_trivia().text(), "");
    }

    /// A BOM is a token of `RRoot`, not trivia
    #[test]
    fn parse_bom() {
        let source = "\u{feff}";
        let with_bom = parse(source, RParserOptions::default());
        assert!(with_bom.diagnostics().is_empty());
        assert_eq!(with_bom.syntax().to_string(), source);
        assert!(with_bom.tree().bom_token().is_some());

        let without_bom = parse("", RParserOptions::default());
        assert!(without_bom.tree().bom_token().is_none());
    }

    /// Not corpus tests: the CST of a tree this deep runs to thousands of
    /// lines, and a snapshot of it says nothing a reader can check
    mod nesting {
        use super::*;
        use crate::parser::MAX_NESTING_DEPTH;

        const DEPTH: usize = MAX_NESTING_DEPTH as usize;

        /// Every shape that costs a stack frame per level, closed off so that
        /// the depth guard is the only thing that can complain
        fn nested(n: usize) -> Vec<(&'static str, String)> {
            vec![
                ("unary", format!("{}1", "-".repeat(n))),
                ("paren", format!("{}1{}", "(".repeat(n), ")".repeat(n))),
                ("brace", format!("{}1{}", "{".repeat(n), "}".repeat(n))),
                ("call", format!("{}1{}", "f(".repeat(n), ")".repeat(n))),
                ("assign", format!("{}1", "a<-".repeat(n))),
                ("exponent", format!("{}1", "2^".repeat(n))),
                ("if", format!("{}1", "if (a) ".repeat(n))),
            ]
        }

        /// The innermost literal is a level of its own, so the most that fits
        /// is `MAX_NESTING_DEPTH - 1` operators
        #[test]
        fn the_limit_falls_where_it_says() {
            let deepest = format!("{}1", "-".repeat(DEPTH - 1));
            assert!(!parse(&deepest, RParserOptions::default()).has_errors());

            let one_more = format!("{}1", "-".repeat(DEPTH));
            assert!(parse(&one_more, RParserOptions::default()).has_errors());
        }

        /// Shapes spend levels at different rates, so these stay well clear of
        /// the boundary rather than restating it
        #[test]
        fn nesting_within_the_limit_parses() {
            for (label, source) in nested(DEPTH / 2) {
                let parse = parse(&source, RParserOptions::default());
                assert!(
                    !parse.has_errors(),
                    "{label}: expected a clean parse, got {:?}",
                    parse.diagnostics(),
                );
            }
        }

        #[test]
        fn nesting_past_the_limit_is_an_error() {
            for (label, source) in nested(DEPTH * 2) {
                let parse = parse(&source, RParserOptions::default());
                let messages: Vec<String> = parse
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| PrintDescription(diagnostic).to_string())
                    .collect();

                assert!(
                    messages
                        .iter()
                        .any(|message| message == "This expression is nested too deeply."),
                    "{label}: expected a nesting diagnostic, got {messages:?}",
                );
            }
        }

        /// Without the guard these abort the process, which no caller can catch
        #[test]
        fn absurd_nesting_neither_aborts_nor_loses_text() {
            for (label, source) in nested(100_000) {
                let parse = parse(&source, RParserOptions::default());
                assert_eq!(parse.syntax().to_string(), source, "{label} round trip");
                assert!(parse.has_errors(), "{label}");
            }
        }
    }

    /// The lexer's diagnostics reach `RParse` through the tree sink
    #[test]
    fn parse_reports_lexer_diagnostics() {
        let source = "`a";
        let parse = parse(source, RParserOptions::default());
        assert_eq!(parse.syntax().to_string(), source);

        assert!(parse.has_errors());
        assert_eq!(parse.diagnostics().len(), 1);
        assert_eq!(
            PrintDescription(&parse.diagnostics()[0]).to_string(),
            "Unterminated backtick-quoted name."
        );
    }
}
