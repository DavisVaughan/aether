//! `gen_corpus_tests!` reads the corpus at expansion time, and a proc-macro
//! records no file dependencies of its own. Without this, adding a corpus file
//! generates no test until something else forces a rebuild.

fn main() {
    println!("cargo::rerun-if-changed=tests/corpus");
}
