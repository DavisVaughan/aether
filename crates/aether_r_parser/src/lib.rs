// The lexer, and the diagnostics it raises, are the crate's only content until
// `RTokenSource` and `RParser` land. Nothing outside `#[cfg(test)]` drives them
// yet, so every item in both modules reads as dead.
#[allow(dead_code, reason = "driven by `RTokenSource` in Phase 5")]
mod diagnostics;
#[allow(dead_code, reason = "driven by `RTokenSource` in Phase 5")]
mod lexer;
