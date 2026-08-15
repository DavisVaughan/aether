mod diagnostics;
mod lexer;
mod options;
mod parse;
mod parser;
mod syntax;
mod token_source;

pub use crate::options::RParserOptions;
pub use crate::parse::{Parse, parse, parse_with_cache};

use aether_r_factory::RSyntaxFactory;
use aether_r_syntax::RLanguage;
use biome_parser::tree_sink::LosslessTreeSink;

pub(crate) type RLosslessTreeSink<'source> = LosslessTreeSink<'source, RLanguage, RSyntaxFactory>;
