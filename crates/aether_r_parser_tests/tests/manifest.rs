//! The anti-drop guard: every test imported from another project is still
//! where `MANIFEST.toml` says it is.

use aether_r_parser_tests::manifest::TREE_SITTER;
use aether_r_parser_tests::{Document, Manifest, corpus};
use std::collections::HashMap;
use std::fs;

/// The count tree-sitter-r's corpus had when it was imported. It is a fixed
/// number of tests taken in one commit, so a smaller manifest means one was
/// dropped rather than that upstream moved.
const TREE_SITTER_TESTS: usize = 89;

#[test]
fn every_imported_test_still_exists() {
    let manifest = Manifest::load();

    let from_tree_sitter = manifest
        .imported
        .iter()
        .filter(|entry| entry.source.starts_with(TREE_SITTER))
        .count();

    assert_eq!(
        from_tree_sitter, TREE_SITTER_TESTS,
        "the manifest lost tree-sitter entries"
    );

    for entry in &manifest.imported {
        let path = corpus::dir().join(&entry.file);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let document = Document::parse(&text).unwrap_or_else(|err| panic!("{}: {err}", entry.file));

        assert!(
            document
                .sections
                .iter()
                .any(|section| section.name == entry.name),
            "test {:?} was imported from {} into {} but is no longer there",
            entry.name,
            entry.source,
            entry.file,
        );
    }
}

/// Copying an imported test into a second file forks it silently: one copy then
/// drifts while `every_imported_test_still_exists` stays green.
#[test]
fn no_imported_test_is_duplicated() {
    let manifest = Manifest::load();
    let mut homes: HashMap<&str, &str> = HashMap::new();

    for entry in &manifest.imported {
        if let Some(other) = homes.insert(&entry.name, &entry.file) {
            panic!(
                "{:?} is listed twice in the manifest, in {} and {}",
                entry.name, other, entry.file
            );
        }
    }

    for path in corpus::input_files() {
        let file = corpus::relative(&path);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let document = Document::parse(&text).unwrap_or_else(|err| panic!("{file}: {err}"));

        for section in &document.sections {
            if let Some(home) = homes.get(section.name.as_str()) {
                assert_eq!(
                    *home, file,
                    "imported test {:?} also appears in {file}",
                    section.name
                );
            }
        }
    }
}
