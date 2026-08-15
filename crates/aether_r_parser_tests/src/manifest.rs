//! `MANIFEST.toml`: every test imported from another project, and where it
//! landed

use crate::corpus;
use serde::Deserialize;

/// A `source` prefix, so the count of imported tree-sitter tests can be
/// asserted on its own
pub const TREE_SITTER: &str = "tree-sitter-r/";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub imported: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    /// Where the test came from, rooted at the upstream project
    pub source: String,
    /// The `##` heading it now lives under
    pub name: String,
    /// Its corpus file, relative to the corpus root
    pub file: String,
}

impl Manifest {
    pub fn load() -> Manifest {
        let path = corpus::dir().join("MANIFEST.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));

        toml::from_str(&text).unwrap_or_else(|err| panic!("cannot parse MANIFEST.toml: {err}"))
    }
}
