//! The anti-gap guard.
//!
//! "Every token kind has lexer coverage" is the acceptance criterion for this
//! phase, and as prose it is unfalsifiable — nobody re-derives it by hand across
//! sixteen files. `RSyntaxKind` is `#[repr(u16)]` and `SyntaxKind` gives a safe
//! `from_raw`, so the inventory is enumerable and the claim is a test.
//!
//! The failure mode this is built for is not "somebody forgot a test today". It
//! is a grammar edit landing a new token kind in `r_kinds_src.rs` months from
//! now with nothing lexing it — otherwise discovered by the parser, as a
//! confusing missing-slot bug several phases downstream.

use super::*;
use biome_rowan::RawSyntaxKind;
use std::collections::HashSet;

/// Sources that between them must produce every lexable kind. The test names the
/// gaps for you.
const SOURCES: &[&str] = &[
    "\u{FEFF}",
    ";,{}()?~$@\\+/^",
    "[ ] [[ ]]",
    "<- <<- := -> ->> - = == != < <= > >=",
    "| || & && |> * ** : :: ::: !",
    "function for in while repeat if else next break",
    "TRUE FALSE NULL Inf NaN NA NA_integer_ NA_real_ NA_complex_ NA_character_",
    "foo ... ..1 1 1L 1i %in%\n# comment\n",
    "\"a\"",
];

/// Every kind before `R_ROOT` in `RSyntaxKind` is a token; `R_ROOT` and
/// everything after it is a node. So adding a node kind to `r.ungram` does not
/// fail this test, but adding a token kind does, until something lexes it.
fn is_lexable(kind: RSyntaxKind) -> bool {
    !matches!(kind, TOMBSTONE | EOF | __LAST) && (kind as u16) < (R_ROOT as u16)
}

#[test]
fn every_lexable_kind_is_produced() {
    let mut produced = HashSet::new();
    for source in SOURCES {
        let (tokens, diagnostics) = lex(source);
        assert!(
            diagnostics.is_empty(),
            "coverage sources are well-formed R; {source:?} lexed with diagnostics",
        );
        produced.extend(tokens.into_iter().map(|token| token.kind));
    }

    let missing: Vec<_> = (0..RSyntaxKind::__LAST as u16)
        .map(|raw| RSyntaxKind::from_raw(RawSyntaxKind(raw)))
        .filter(|kind| is_lexable(*kind) && !produced.contains(kind))
        .collect();

    assert!(missing.is_empty(), "no test lexes: {missing:#?}");
}
