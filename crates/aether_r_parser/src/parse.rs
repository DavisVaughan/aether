use crate::RLosslessTreeSink;
use crate::options::RParserOptions;
use crate::parser::RParser;
use aether_r_syntax::{RRoot, RSyntaxNode};
use biome_parser::AnyParse;
use biome_parser::diagnostic::ParseDiagnostic;
use biome_rowan::{AstNode, NodeCache, SendNode};

/// A utility struct for managing the result of a parser job
///
/// This struct holds a handle to the root node of the parsed syntax tree, along
/// with the diagnostics the parser emitted while producing it.
///
/// It can be dynamically downcast into a concrete [RSyntaxNode] or [RRoot].
///
/// # Why the root is a [SendNode] and not an [RSyntaxNode]
///
/// Every other Biome parser stores its own language's node and converts to
/// [SendNode] at the [AnyParse] boundary, which is the only place biome needs a
/// tree to cross a thread. Air never reaches that boundary: its LSP keeps a
/// [Parse] in every `Document` and clones the whole `WorldState` into
/// `spawn_blocking`, whose handlers are `Send + 'static`. So [Parse] itself has
/// to be [Send], and [RSyntaxNode] is not — it holds an `Rc`.
///
/// The price is that [`Parse::syntax()`] rebuilds a red root on every call
/// instead of bumping a refcount. Cheap, but not free, so hold the result rather
/// than calling it in a loop.
#[derive(Clone, Debug)]
pub struct Parse {
    root: SendNode,
    diagnostics: Vec<ParseDiagnostic>,
}

impl Parse {
    fn new(root: RSyntaxNode, diagnostics: Vec<ParseDiagnostic>) -> Self {
        // Safety: This method is not exposed, we only use it internally
        let root = root.as_send().unwrap();
        Self { root, diagnostics }
    }

    /// The syntax node represented by this Parse result
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

    /// Returns [true] if the parser encountered some errors during the parsing.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(ParseDiagnostic::is_error)
    }
}

impl From<Parse> for AnyParse {
    fn from(parse: Parse) -> Self {
        Self::new(parse.root, parse.diagnostics)
    }
}

pub fn parse(text: &str, options: RParserOptions) -> Parse {
    let mut cache = NodeCache::default();
    parse_with_cache(text, options, &mut cache)
}

/// Parses `text`, structurally sharing what it can with the trees already in
/// `cache`
pub fn parse_with_cache(text: &str, options: RParserOptions, cache: &mut NodeCache) -> Parse {
    let mut parser = RParser::new(text, options);
    crate::syntax::parse_root(&mut parser);
    let (events, trivia, diagnostics) = parser.finish();

    let mut tree_sink = RLosslessTreeSink::with_cache(text, &trivia, cache);
    biome_parser::event::process(&mut tree_sink, events, diagnostics);
    let (green, diagnostics) = tree_sink.finish();

    Parse::new(green, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_r_syntax::RSyntaxKind;
    use biome_diagnostics::PrintDescription;
    use biome_rowan::AstNodeList;

    /// Air's LSP clones a `WorldState` full of `Parse`s into `spawn_blocking`,
    /// so this is a real constraint on the root's type. Failing here beats
    /// failing at swap-in.
    #[test]
    fn parse_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Parse>();
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

    /// The lexer's diagnostics reach `Parse` through the tree sink
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
