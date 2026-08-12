#[cfg(test)]
mod tests;

use crate::diagnostics::{
    unexpected_character, unterminated_backtick_identifier, unterminated_exponent,
    unterminated_hexadecimal, unterminated_hexadecimal_binary_exponent, unterminated_special,
};
use aether_r_syntax::RSyntaxKind::{self, *};
use aether_r_syntax::T;
use biome_parser::diagnostic::ParseDiagnostic;
use biome_parser::lexer::{
    LexContext, Lexer, LexerCheckpoint, LexerWithCheckpoint, ReLexer, TokenFlags,
};
use biome_rowan::{SyntaxKind, TextRange, TextSize};
use unicode_ident::{is_xid_continue, is_xid_start};

/// The context the parser lexes the next token in
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RLexContext {
    #[default]
    Regular,

    /// Lex the body of a string opened with this delimiter
    StringContent(RStringDelimiter),

    /// Lex the closing delimiter of a string
    StringEnd(RStringDelimiter),
}

impl LexContext for RLexContext {
    fn is_regular(&self) -> bool {
        matches!(self, RLexContext::Regular)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RStringDelimiter {
    Single,
    Double,
    /// `r"---[...]---"`, closed by the matching bracket, the same number of
    /// hyphens, and the same quote
    Raw {
        quote: u8,
        hyphens: u8,
        bracket: RawStringBracket,
    },
}

impl RStringDelimiter {
    /// The string quote character
    fn quote(self) -> u8 {
        match self {
            Self::Single => b'\'',
            Self::Double => b'"',
            Self::Raw { quote, .. } => quote,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawStringBracket {
    Paren,
    Bracket,
    Brace,
}

impl RawStringBracket {
    fn from_open(byte: u8) -> Option<Self> {
        match byte {
            b'(' => Some(Self::Paren),
            b'[' => Some(Self::Bracket),
            b'{' => Some(Self::Brace),
            _ => None,
        }
    }

    fn close(self) -> u8 {
        match self {
            Self::Paren => b')',
            Self::Bracket => b']',
            Self::Brace => b'}',
        }
    }
}

/// The re-lexing requests the parser can make
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RReLexContext {
    /// Re-lex `]` as `]]` when the very next byte is another `]`. A no-op on any
    /// other current token.
    JoinBrackets,
}

impl RLexer<'_> {
    fn re_lex_join_brackets(&mut self) -> RSyntaxKind {
        match self.current_kind {
            T![']'] if self.peek_byte() == Some(b']') => self.eat_bytes(2, T!["]]"]),
            kind => kind,
        }
    }
}

pub(crate) struct RLexer<'src> {
    source: &'src str,

    /// Byte offset of the next token
    position: usize,

    /// Byte offset of the current token. Its range is `current_start..position`.
    current_start: TextSize,

    current_kind: RSyntaxKind,

    current_flags: TokenFlags,

    /// Sticky across `next_token` calls: set by `consume_newline_or_whitespaces`
    /// and cleared only once a non-trivia token has been stamped with it. This
    /// is what lets the lexer report a preceding line break even though
    /// `NEWLINE` is a token in its own right, and it is exactly
    /// `biome_js_parser`'s mechanism.
    after_line_break: bool,

    /// Byte length of the BOM, if the source starts with one
    ///
    /// Nothing in R reads this. It exists because [`LexerCheckpoint`] carries
    /// it, and setting it from `consume_potential_bom` costs one line where a
    /// hardcoded `0` would be a lie the next reader has to re-derive.
    unicode_bom_length: usize,

    /// Set when `STRING_OPEN` is emitted, cleared after `STRING_CLOSE`
    ///
    /// This is how the caller knows which [`RLexContext`] to lex the body and
    /// the closing delimiter in. The debug assertions in `lex_string_content`
    /// and `lex_string_end` catch if this is ever done incorrectly.
    string_delimiter: Option<RStringDelimiter>,

    diagnostics: Vec<ParseDiagnostic>,
}

impl<'src> Lexer<'src> for RLexer<'src> {
    const NEWLINE: Self::Kind = NEWLINE;
    const WHITESPACE: Self::Kind = WHITESPACE;

    type Kind = RSyntaxKind;
    type LexContext = RLexContext;
    type ReLexContext = RReLexContext;

    fn source(&self) -> &'src str {
        self.source
    }

    fn current(&self) -> Self::Kind {
        self.current_kind
    }

    fn current_start(&self) -> TextSize {
        self.current_start
    }

    fn next_token(&mut self, context: Self::LexContext) -> Self::Kind {
        self.current_start = TextSize::from(self.position as u32);
        self.current_flags = TokenFlags::empty();

        let kind = if self.is_eof() {
            EOF
        } else {
            match context {
                RLexContext::Regular => self.lex_token(),
                RLexContext::StringContent(delimiter) => self.lex_string_content(delimiter),
                RLexContext::StringEnd(delimiter) => self.lex_string_end(delimiter),
            }
        };

        // Stamped after lexing so that the lex functions are free to set other
        // flags first
        self.current_flags
            .set(TokenFlags::PRECEDING_LINE_BREAK, self.after_line_break);
        self.current_kind = kind;

        // The bit survives across trivia, so `1\n  # c\n  + 2` reports the break
        // on `+` rather than on the whitespace that happens to precede it
        if !kind.is_trivia() {
            self.after_line_break = false;
        }

        debug_assert!(self.current_start <= TextSize::from(self.position as u32));
        kind
    }

    fn has_preceding_line_break(&self) -> bool {
        self.current_flags.has_preceding_line_break()
    }

    /// R has no token-level unicode escapes
    fn has_unicode_escape(&self) -> bool {
        false
    }

    fn rewind(&mut self, checkpoint: LexerCheckpoint<Self::Kind>) {
        let LexerCheckpoint {
            position,
            current_start,
            current_kind,
            current_flags,
            after_line_break,
            unicode_bom_length,
            diagnostics_pos,
        } = checkpoint;

        self.position = u32::from(position) as usize;
        self.current_start = current_start;
        self.current_kind = current_kind;
        self.current_flags = current_flags;
        self.after_line_break = after_line_break;
        self.unicode_bom_length = unicode_bom_length;
        self.diagnostics.truncate(diagnostics_pos as usize);
    }

    fn finish(self) -> Vec<ParseDiagnostic> {
        self.diagnostics
    }

    fn current_flags(&self) -> TokenFlags {
        self.current_flags
    }

    fn position(&self) -> usize {
        self.position
    }

    fn push_diagnostic(&mut self, diagnostic: ParseDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn advance(&mut self, n: usize) {
        self.position += n;
    }

    fn advance_char_unchecked(&mut self) {
        let char = self.current_char_unchecked();
        self.position += char.len_utf8();
    }

    /// Overridden to update `after_line_break`. The default cannot since this is
    /// a custom field. `biome_js_parser` overrides it for the same reason.
    ///
    /// Inheriting the trait's `consume_newline` also gets `\r\n` treated as a
    /// single newline for free.
    fn consume_newline_or_whitespaces(&mut self) -> Self::Kind {
        if self.consume_newline() {
            self.after_line_break = true;
            NEWLINE
        } else {
            self.consume_whitespaces();
            WHITESPACE
        }
    }

    /// R's whitespace is ASCII: space, tab, vertical tab, and form feed
    ///
    /// The trait's body is written for CSS and diagnoses vertical tab and form
    /// feed, which are ordinary whitespace here.
    fn consume_whitespaces(&mut self) {
        while let Some(b'\t' | b' ' | b'\x0B' | b'\x0C') = self.current_byte() {
            self.advance(1);
        }
    }
}

impl<'src> ReLexer<'src> for RLexer<'src> {
    fn re_lex(&mut self, context: Self::ReLexContext) -> Self::Kind {
        let old_position = self.position;
        self.position = u32::from(self.current_start) as usize;

        let new_kind = match context {
            RReLexContext::JoinBrackets => self.re_lex_join_brackets(),
        };

        if self.current_kind == new_kind {
            // Nothing was re-lexed. Return the existing token untouched.
            self.position = old_position;
        } else {
            self.current_kind = new_kind;
        }

        new_kind
    }
}

impl<'src> LexerWithCheckpoint<'src> for RLexer<'src> {
    fn checkpoint(&self) -> LexerCheckpoint<Self::Kind> {
        LexerCheckpoint {
            position: TextSize::from(self.position as u32),
            current_start: self.current_start,
            current_kind: self.current_kind,
            current_flags: self.current_flags,
            after_line_break: self.after_line_break,
            unicode_bom_length: self.unicode_bom_length,
            diagnostics_pos: self.diagnostics.len() as u32,
        }
    }
}

impl<'src> RLexer<'src> {
    pub(crate) fn from_str(source: &'src str) -> Self {
        Self {
            source,
            position: 0,
            current_start: TextSize::from(0),
            current_kind: TOMBSTONE,
            current_flags: TokenFlags::empty(),
            after_line_break: false,
            unicode_bom_length: 0,
            string_delimiter: None,
            diagnostics: Vec::new(),
        }
    }

    /// The order of these arms is load-bearing, and several of the decisions
    /// come straight from tree-sitter-r's `grammar.js` comments, where they
    /// record real bugs
    fn lex_token(&mut self) -> RSyntaxKind {
        debug_assert!(!self.is_eof());

        match self.current_byte_unchecked() {
            b'\n' | b'\r' | b'\t' | b' ' | b'\x0B' | b'\x0C' => {
                self.consume_newline_or_whitespaces()
            }

            b'#' => self.consume_comment(),

            b'"' => self.consume_string_open(RStringDelimiter::Double),
            b'\'' => self.consume_string_open(RStringDelimiter::Single),
            b'`' => self.consume_backtick_identifier(),

            // Ahead of the digit arm, which would otherwise take the `0`
            b'0' if matches!(self.peek_byte(), Some(b'x' | b'X')) => self.consume_hexadecimal(),
            b'0'..=b'9' => self.consume_number(),

            // `.` is overloaded four ways: `...`, `..1`, `.5`, and `.foo`
            b'.' => self.consume_dot(),

            b'%' => self.consume_special(),

            // Raw strings share this arm with every other name
            // rather than taking one of their own, which would split the letter
            // range and cost the whole match its jump table for `s` onwards.
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => match self.scan_raw_string_open() {
                Some(open) => self.consume_raw_string_open(open),
                None => self.consume_identifier_or_keyword(),
            },

            b';' => self.eat_byte(T![;]),
            b',' => self.eat_byte(T![,]),
            b'{' => self.eat_byte(T!['{']),
            b'}' => self.eat_byte(T!['}']),
            b'(' => self.eat_byte(T!['(']),
            b')' => self.eat_byte(T![')']),
            b'?' => self.eat_byte(T![?]),
            b'~' => self.eat_byte(T![~]),
            b'+' => self.eat_byte(T![+]),
            b'/' => self.eat_byte(T![/]),
            b'^' => self.eat_byte(T![^]),
            b'$' => self.eat_byte(T![$]),
            b'@' => self.eat_byte(T![@]),
            b'\\' => self.eat_byte(T!["\\"]),

            // Lexer emits `]` but never `]]`. Only the parser can decide if we
            // are in a [`RReLexContext::JoinBrackets`] context or not, at which
            // point re-lexing will join contiguous brackets.
            b']' => self.eat_byte(T![']']),

            // `[[` is greedy in R's own `gram.y` and in tree-sitter-r's scanner.
            // Two adjacent opening brackets are always a subset2.
            b'[' => match self.peek_byte() {
                Some(b'[') => self.eat_bytes(2, T!["[["]),
                _ => self.eat_byte(T!['[']),
            },
            b'<' => match (self.peek_byte(), self.byte_at(2)) {
                (Some(b'<'), Some(b'-')) => self.eat_bytes(3, T![<<-]),
                (Some(b'-'), _) => self.eat_bytes(2, T![<-]),
                (Some(b'='), _) => self.eat_bytes(2, T![<=]),
                _ => self.eat_byte(T![<]),
            },
            b'-' => match (self.peek_byte(), self.byte_at(2)) {
                (Some(b'>'), Some(b'>')) => self.eat_bytes(3, T![->>]),
                (Some(b'>'), _) => self.eat_bytes(2, T![->]),
                _ => self.eat_byte(T![-]),
            },
            b':' => match (self.peek_byte(), self.byte_at(2)) {
                (Some(b':'), Some(b':')) => self.eat_bytes(3, T![:::]),
                (Some(b':'), _) => self.eat_bytes(2, T![::]),
                (Some(b'='), _) => self.eat_bytes(2, T![:=]),
                _ => self.eat_byte(T![:]),
            },
            b'>' => match self.peek_byte() {
                Some(b'=') => self.eat_bytes(2, T![>=]),
                _ => self.eat_byte(T![>]),
            },
            b'=' => match self.peek_byte() {
                Some(b'=') => self.eat_bytes(2, T![==]),
                _ => self.eat_byte(T![=]),
            },
            b'!' => match self.peek_byte() {
                Some(b'=') => self.eat_bytes(2, T![!=]),
                _ => self.eat_byte(T![!]),
            },
            b'&' => match self.peek_byte() {
                Some(b'&') => self.eat_bytes(2, T![&&]),
                _ => self.eat_byte(T![&]),
            },
            b'|' => match self.peek_byte() {
                Some(b'|') => self.eat_bytes(2, T![||]),
                Some(b'>') => self.eat_bytes(2, T![|>]),
                _ => self.eat_byte(T![|]),
            },
            b'*' => match self.peek_byte() {
                Some(b'*') => self.eat_bytes(2, T![**]),
                _ => self.eat_byte(T![*]),
            },

            byte if byte >= 128 => self.consume_unicode(),

            _ => self.consume_unexpected(),
        }
    }

    fn current_byte_unchecked(&self) -> u8 {
        debug_assert!(!self.is_eof());
        self.source.as_bytes()[self.position]
    }

    fn eat_byte(&mut self, kind: RSyntaxKind) -> RSyntaxKind {
        self.advance(1);
        kind
    }

    fn eat_bytes(&mut self, n: usize, kind: RSyntaxKind) -> RSyntaxKind {
        self.advance(n);
        kind
    }

    /// `#` to the end of the line, exclusive of the line break itself
    fn consume_comment(&mut self) -> RSyntaxKind {
        self.assert_byte(b'#');
        self.advance(1);

        while let Some(byte) = self.current_byte() {
            if matches!(byte, b'\n' | b'\r') {
                break;
            }
            self.advance_byte_or_char(byte);
        }

        COMMENT
    }

    // -----------------------------------------------------------------------
    // Identifiers and keywords

    /// Consumes an identifier mimicking `[\p{XID_Start}_][\p{XID_Continue}.]*`.
    /// If the consumed identifier maps to a keyword, that is returned instead.
    fn consume_identifier_or_keyword(&mut self) -> RSyntaxKind {
        let start = self.position;
        self.advance_char_unchecked();
        self.consume_identifier_parts();

        Self::keyword_kind(&self.source[start..self.position]).unwrap_or(IDENT)
    }

    /// `[\p{XID_Continue}.]*`
    fn consume_identifier_parts(&mut self) {
        while self.at_identifier_part(0) {
            self.advance_char_unchecked();
        }
    }

    /// Whether `[\p{XID_Continue}.]` matches `offset` bytes ahead of the current
    /// position
    fn at_identifier_part(&self, offset: usize) -> bool {
        match self.byte_at(offset) {
            Some(b'.' | b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z') => true,
            Some(byte) if byte >= 128 => self.char_at(offset).is_some_and(is_xid_continue),
            _ => false,
        }
    }

    /// The character `offset` bytes ahead, or `None` past the end of the source
    /// or at a byte that is not a character boundary
    fn char_at(&self, offset: usize) -> Option<char> {
        self.source.get(self.position + offset..)?.chars().next()
    }

    /// R's keywords and constants, spelled as they appear in source
    fn keyword_kind(text: &str) -> Option<RSyntaxKind> {
        Some(match text {
            "function" => T![function],
            "for" => T![for],
            "in" => T![in],
            "while" => T![while],
            "repeat" => T![repeat],
            "if" => T![if],
            "else" => T![else],
            "next" => T![next],
            "break" => T![break],
            "TRUE" => T![true],
            "FALSE" => T![false],
            "NULL" => T![null],
            "Inf" => T![inf],
            "NaN" => T![nan],
            "NA" => T![na_logical],
            "NA_integer_" => T![na_integer],
            "NA_real_" => T![na_double],
            "NA_complex_" => T![na_complex],
            "NA_character_" => T![na_character],
            _ => return None,
        })
    }

    fn consume_backtick_identifier(&mut self) -> RSyntaxKind {
        self.assert_byte(b'`');
        let open = TextRange::at(self.text_position(), TextSize::from(1));
        self.advance(1);

        while let Some(byte) = self.current_byte() {
            match byte {
                b'`' => {
                    self.advance(1);
                    return IDENT;
                }
                b'\\' => {
                    self.advance(1);
                    if let Some(escaped) = self.current_byte() {
                        self.advance_byte_or_char(escaped);
                    }
                }
                byte => self.advance_byte_or_char(byte),
            }
        }

        let eof = TextRange::empty(self.text_position());
        self.push_diagnostic(unterminated_backtick_identifier(open, eof));
        IDENT
    }

    /// The `byte >= 128` arm, which is also where the BOM is resolved
    fn consume_unicode(&mut self) -> RSyntaxKind {
        // A BOM is only a BOM at offset 0. Elsewhere it falls through as unexpected.
        if self.position == 0
            && let Some((bom, length)) = self.consume_potential_bom(UNICODE_BOM)
        {
            self.unicode_bom_length = length;
            return bom;
        }

        if is_xid_start(self.current_char_unchecked()) {
            self.consume_identifier_or_keyword()
        } else {
            self.consume_unexpected()
        }
    }

    // -----------------------------------------------------------------------
    // Dots

    /// `...`, `..1`, `.5`, and `.foo` all start with a `.`
    ///
    /// R decides between a number and a name on the single character after the
    /// `.` (`typeofnext()` in `src/main/gram.y`), then consumes one whole name
    /// and classifies its text. `...` and `..i` are not tokens of their own
    /// there — `"..."` is a row in R's keyword table, and `..1` is an ordinary
    /// symbol.
    fn consume_dot(&mut self) -> RSyntaxKind {
        self.assert_byte(b'.');

        // A digit after the `.` always makes a number, never a name, otherwise
        // `.1i` and `.1L` lex as identifiers (r-lib/tree-sitter-r#190).
        if matches!(self.peek_byte(), Some(b'0'..=b'9')) {
            return self.consume_number();
        }

        let start = self.position;
        self.advance(1);
        self.consume_identifier_parts();

        Self::dot_kind(&self.source[start..self.position])
    }

    /// The two names the parser has to tell apart from any other identifier
    fn dot_kind(text: &str) -> RSyntaxKind {
        match text.as_bytes() {
            b"..." => T![...],
            [b'.', b'.', digits @ ..]
                if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) =>
            {
                DOTDOTI
            }
            _ => IDENT,
        }
    }

    // -----------------------------------------------------------------------
    // Numbers

    /// `(?:(?:\d+(?:\.\d*)?)|(?:\.\d+))(?:[eE][+-]?\d+)?`
    ///
    /// Both alternatives fall out of one pass, because "digits, then optionally
    /// a dot and more digits" covers `1`, `1.`, `1.5` and `.5` alike. The caller
    /// guarantees at least one digit is present on one side of the dot.
    ///
    /// An `e` commits to an exponent the same way a `0x` commits to a body, so
    /// `1e` and `1e+` are diagnosed. tree-sitter-r's regex ends `[eE][+-]?\d*`
    /// and takes them as whole literals — see `docs/deviations.md`.
    fn consume_number(&mut self) -> RSyntaxKind {
        let start = self.text_position();
        self.consume_digits(u8::is_ascii_digit);

        if self.current_byte() == Some(b'.') {
            self.advance(1);
            self.consume_digits(u8::is_ascii_digit);
        }

        if matches!(self.current_byte(), Some(b'e' | b'E')) && !self.consume_exponent() {
            let range = TextRange::new(start, self.text_position());
            self.push_diagnostic(unterminated_exponent(range));
        }

        self.consume_suffix()
    }

    /// `0[xX](([0-9a-fA-F]+(\.[0-9a-fA-F]*)?)|(\.[0-9a-fA-F]*))([pP][+-]?[0-9]+)?`,
    /// plus a suffix
    ///
    /// `0x.` is a whole literal — R allows it, surprisingly.
    ///
    /// Once the `0x` is there the rest is required, so what the grammar still
    /// wants is diagnosed rather than un-lexed: `0x`, `0xg`, and `0x1p` are all
    /// unterminated hexadecimals. tree-sitter-r instead re-splits them into a
    /// shorter number and a name — see `docs/deviations.md`.
    fn consume_hexadecimal(&mut self) -> RSyntaxKind {
        let start = self.text_position();
        self.advance(2);

        if !matches!(
            self.current_byte(),
            Some(b'.' | b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
        ) {
            let range = TextRange::new(start, self.text_position());
            self.push_diagnostic(unterminated_hexadecimal(range));
            return self.consume_suffix();
        }

        self.consume_digits(u8::is_ascii_hexdigit);

        if self.current_byte() == Some(b'.') {
            self.advance(1);
            self.consume_digits(u8::is_ascii_hexdigit);
        }

        if matches!(self.current_byte(), Some(b'p' | b'P')) && !self.consume_exponent() {
            let range = TextRange::new(start, self.text_position());
            self.push_diagnostic(unterminated_hexadecimal_binary_exponent(range));
        }

        self.consume_suffix()
    }

    /// The `L` and `i` that turn the literal before them into an integer or a
    /// complex
    ///
    /// tree-sitter-r spells these as tokens of their own rather than as part of
    /// the literal's regex, so an identifier that merely starts with one wins on
    /// length: `1L` is an integer, but `1Li` is the literal `1` followed by the
    /// name `Li`. R splits that one the other way, `1L` then `i` — see
    /// `docs/deviations.md`.
    fn consume_suffix(&mut self) -> RSyntaxKind {
        let suffixed = match self.current_byte() {
            Some(b'L') => R_INTEGER_LITERAL,
            Some(b'i') => R_COMPLEX_LITERAL,
            _ => return R_DOUBLE_LITERAL,
        };

        if self.at_identifier_part(1) {
            return R_DOUBLE_LITERAL;
        }
        self.eat_byte(suffixed)
    }

    /// A marker the caller has already seen, an optional sign, and its digits,
    /// reporting whether the digits were there
    fn consume_exponent(&mut self) -> bool {
        self.advance(1);

        if matches!(self.current_byte(), Some(b'+' | b'-')) {
            self.advance(1);
        }

        self.consume_digits(u8::is_ascii_digit)
    }

    /// Whether any digit was consumed
    fn consume_digits(&mut self, is_digit: fn(&u8) -> bool) -> bool {
        let start = self.position;
        while matches!(self.current_byte(), Some(byte) if is_digit(&byte)) {
            self.advance(1);
        }
        self.position != start
    }

    // -----------------------------------------------------------------------
    // Special operators

    /// `/%[^%\\\n]*%/` — `%%`, `%/%`, `%in%`, `%o%`, and friends.
    ///
    /// Unlike strings and backticked names, a `%…%` may not span a newline. R
    /// ungets the newline before reporting the error, so the statement still
    /// terminates and the next line parses normally; aether stops before the
    /// newline for the same reason, and keeps the partial `%foo` as a `SPECIAL`
    /// carrying the diagnostic.
    ///
    /// The backslash and the end of file are the two places aether follows
    /// tree-sitter-r rather than R, which accepts both. See `docs/deviations.md`.
    fn consume_special(&mut self) -> RSyntaxKind {
        self.assert_byte(b'%');
        let open = TextRange::at(self.text_position(), TextSize::from(1));
        self.advance(1);

        while let Some(byte) = self.current_byte() {
            match byte {
                b'%' => {
                    self.advance(1);
                    return SPECIAL;
                }
                b'\n' | b'\r' | b'\\' => break,
                byte => self.advance_byte_or_char(byte),
            }
        }

        self.push_diagnostic(unterminated_special(open));
        SPECIAL
    }

    // -----------------------------------------------------------------------
    // Strings

    fn consume_string_open(&mut self, delimiter: RStringDelimiter) -> RSyntaxKind {
        self.assert_byte(delimiter.quote());
        self.advance(1);
        self.string_delimiter = Some(delimiter);
        STRING_OPEN
    }

    /// The `r"---[` prefix of a raw string, as a delimiter and its byte length,
    /// or `None` when what follows is not one.
    ///
    /// Answers for any identifier start, not just `r`, so that [`Self::lex_token`]
    /// can ask without first singling out `r` and `R`.
    ///
    /// Ported from `scan_raw_string_open` in tree-sitter-r's `src/scanner.c`,
    /// down to bailing out on the pathological 256th hyphen.
    fn scan_raw_string_open(&self) -> Option<(RStringDelimiter, usize)> {
        if !matches!(self.current_byte(), Some(b'r' | b'R')) {
            return None;
        }

        let quote = match self.byte_at(1) {
            Some(quote @ (b'"' | b'\'')) => quote,
            _ => return None,
        };

        let mut offset = 2;
        let mut hyphens: u8 = 0;
        while self.byte_at(offset) == Some(b'-') {
            hyphens = hyphens.checked_add(1)?;
            offset += 1;
        }

        let bracket = RawStringBracket::from_open(self.byte_at(offset)?)?;

        let delimiter = RStringDelimiter::Raw {
            quote,
            hyphens,
            bracket,
        };
        Some((delimiter, offset + 1))
    }

    fn consume_raw_string_open(&mut self, open: (RStringDelimiter, usize)) -> RSyntaxKind {
        let (delimiter, length) = open;
        self.advance(length);
        self.string_delimiter = Some(delimiter);
        STRING_OPEN
    }

    fn lex_string_content(&mut self, delimiter: RStringDelimiter) -> RSyntaxKind {
        debug_assert!(!self.is_eof());
        debug_assert_eq!(self.string_delimiter, Some(delimiter));

        match delimiter {
            RStringDelimiter::Single | RStringDelimiter::Double => {
                self.consume_quoted_string_content(delimiter.quote());

                // An empty body emits no `STRING_CONTENT` at all: the close
                // follows the open directly, matching tree-sitter-r and keeping
                // `RStringValue`'s optional `content` slot meaningful
                if self.position == usize::from(self.current_start) {
                    self.lex_string_end(delimiter)
                } else {
                    STRING_CONTENT
                }
            }
            RStringDelimiter::Raw {
                quote,
                hyphens,
                bracket,
            } => self.consume_raw_string_content(quote, hyphens, bracket),
        }
    }

    /// Everything up to the closing quote, in one token.
    ///
    /// Escape sequences are not modelled separately — neither `r.ungram` nor
    /// air's tree walker has a node for them — so a backslash simply consumes
    /// the character after it, which is what keeps `"a\"b"` a single string.
    fn consume_quoted_string_content(&mut self, quote: u8) {
        while let Some(byte) = self.current_byte() {
            match byte {
                b'\\' => {
                    self.advance(1);
                    if let Some(escaped) = self.current_byte() {
                        self.advance_byte_or_char(escaped);
                    }
                }
                byte if byte == quote => break,
                byte => self.advance_byte_or_char(byte),
            }
        }
    }

    /// Ported from `scan_raw_string_content_or_close` in tree-sitter-r's
    /// `src/scanner.c`.
    ///
    /// The subtle part is what happens when a closing bracket turns out not to
    /// start the closing sequence: scanning resumes from wherever the attempt
    /// stopped, *not* from the byte after the bracket. Otherwise `r"(())"` and
    /// `r"-())-"` both fail to find their close (their #162).
    fn consume_raw_string_content(
        &mut self,
        quote: u8,
        hyphens: u8,
        bracket: RawStringBracket,
    ) -> RSyntaxKind {
        let close = bracket.close();
        let start = self.position;

        while let Some(byte) = self.current_byte() {
            if byte != close {
                self.advance_byte_or_char(byte);
                continue;
            }

            let content_end = self.position;
            self.advance(1);

            if !self.consume_repeated(b'-', usize::from(hyphens)) {
                continue;
            }
            if self.current_byte() != Some(quote) {
                continue;
            }
            self.advance(1);

            if content_end > start {
                // Rewind to the content and re-consume the closing sequence on
                // the next call, now that it is known to be there
                self.position = content_end;
                return STRING_CONTENT;
            }

            self.string_delimiter = None;
            return STRING_CLOSE;
        }

        // End of file inside the body. The parser reports the unterminated
        // string when its `expect(STRING_CLOSE)` fails.
        STRING_CONTENT
    }

    /// Consume exactly `count` copies of `byte`, stopping at the first mismatch.
    ///
    /// A failed run leaves the position at the mismatch rather than rewinding,
    /// which is what the raw string scan above relies on.
    fn consume_repeated(&mut self, byte: u8, count: usize) -> bool {
        for _ in 0..count {
            if self.current_byte() != Some(byte) {
                return false;
            }
            self.advance(1);
        }
        true
    }

    fn lex_string_end(&mut self, delimiter: RStringDelimiter) -> RSyntaxKind {
        debug_assert!(!self.is_eof());
        debug_assert_eq!(self.string_delimiter, Some(delimiter));

        match delimiter {
            RStringDelimiter::Single | RStringDelimiter::Double => {
                self.assert_byte(delimiter.quote());
                self.advance(1);
            }
            RStringDelimiter::Raw {
                quote,
                hyphens,
                bracket,
            } => {
                // `consume_raw_string_content` already matched the whole
                // sequence, so this only has to re-consume it
                self.assert_byte(bracket.close());
                self.advance(1);
                let matched = self.consume_repeated(b'-', usize::from(hyphens));
                debug_assert!(matched);
                self.assert_byte(quote);
                self.advance(1);
            }
        }

        self.string_delimiter = None;
        STRING_CLOSE
    }

    // -----------------------------------------------------------------------
    // Errors

    /// A character that starts no token.
    ///
    /// Exactly one character is consumed and handed on as `ERROR_TOKEN`, which
    /// is `biome_js_parser`'s treatment of the same case. Losing the character
    /// is not an option — round-tripping the source is an invariant, and folding
    /// it into trivia would make it silently disappear the way semicolons used
    /// to — but neither is calling it an `IDENT`, which is accepted wherever a
    /// name is and would let junk parse into a well-formed tree. `ERROR_TOKEN`
    /// starts nothing, so the parser can only wrap it in a bogus node.
    fn consume_unexpected(&mut self) -> RSyntaxKind {
        let start = self.text_position();
        let character = self.current_char_unchecked();
        self.advance(character.len_utf8());

        let range = TextRange::new(start, self.text_position());
        self.push_diagnostic(unexpected_character(range, character));
        ERROR_TOKEN
    }
}
