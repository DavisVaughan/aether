use biome_rowan::{Language, SyntaxNode, SyntaxToken, TextRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    Node {
        kind: String,
        range: TextRange,
        children: Vec<Shape>,
    },
    Token {
        kind: String,
        range: TextRange,
        leading_comments: Vec<String>,
        trailing_comments: Vec<String>,
    },
}

impl Shape {
    pub(crate) fn kind(&self) -> &str {
        match self {
            Shape::Node { kind, .. } | Shape::Token { kind, .. } => kind,
        }
    }

    pub(crate) fn range(&self) -> TextRange {
        match self {
            Shape::Node { range, .. } | Shape::Token { range, .. } => *range,
        }
    }

    pub(crate) fn describe(&self) -> String {
        format!("{} @ {:?}", self.kind(), self.range())
    }
}

pub(crate) fn shape_of<L: Language>(node: &SyntaxNode<L>) -> Shape {
    let kind = canonical_kind(&debug_kind(node.kind()));

    let children = if kind == "R_STRING_VALUE" {
        vec![collapsed_string(node)]
    } else {
        children_of(node)
    };

    Shape::Node {
        kind,
        range: node.text_trimmed_range(),
        children,
    }
}

/// Air spells a string as an open, an optional content, and a close; aether as
/// one literal token. Both collapse to this.
fn collapsed_string<L: Language>(node: &SyntaxNode<L>) -> Shape {
    let leading = node
        .first_token()
        .map(|token| comments_of(token.leading_trivia().pieces()));
    let trailing = node
        .last_token()
        .map(|token| comments_of(token.trailing_trivia().pieces()));

    Shape::Token {
        kind: "STRING".to_string(),
        range: node.text_trimmed_range(),
        leading_comments: leading.unwrap_or_default(),
        trailing_comments: trailing.unwrap_or_default(),
    }
}

fn children_of<L: Language>(node: &SyntaxNode<L>) -> Vec<Shape> {
    let mut shapes = Vec::new();

    for element in node.children_with_tokens() {
        match element {
            biome_rowan::SyntaxElement::Node(child) => {
                let kind = canonical_kind(&debug_kind(child.kind()));

                if is_transparent_node(&kind) {
                    shapes.extend(children_of(&child));
                } else {
                    shapes.push(shape_of(&child));
                }
            }
            biome_rowan::SyntaxElement::Token(token) => {
                let kind = canonical_kind(&debug_kind(token.kind()));

                if !is_ignored_token(&kind) {
                    shapes.push(shape_of_token(&token, kind));
                }
            }
        }
    }

    shapes
}

fn shape_of_token<L: Language>(token: &SyntaxToken<L>, kind: String) -> Shape {
    Shape::Token {
        kind,
        range: token.text_trimmed_range(),
        leading_comments: comments_of(token.leading_trivia().pieces()),
        trailing_comments: comments_of(token.trailing_trivia().pieces()),
    }
}

fn comments_of<L: Language>(
    pieces: impl Iterator<Item = biome_rowan::SyntaxTriviaPiece<L>>,
) -> Vec<String> {
    pieces
        .filter(|piece| piece.is_comments())
        .map(|piece| piece.text().to_string())
        .collect()
}

fn debug_kind<K: std::fmt::Debug>(kind: K) -> String {
    format!("{kind:?}")
}

fn canonical_kind(kind: &str) -> String {
    match kind {
        "R_EXPRESSION_LIST" => "R_STATEMENT_LIST".to_string(),
        "R_BRACED_EXPRESSIONS" => "R_BRACED_STATEMENTS".to_string(),
        other => other.to_string(),
    }
}

fn is_transparent_node(kind: &str) -> bool {
    kind == "R_STATEMENT"
}

fn is_ignored_token(kind: &str) -> bool {
    kind == "SEMICOLON"
}
