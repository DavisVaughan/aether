use super::*;

fn document(text: &str) -> Document {
    Document::parse(text).expect("the document parses")
}

fn error(text: &str) -> String {
    Document::parse(text)
        .expect_err("the document is rejected")
        .message
}

const INPUT: &str = "\
## Numbers

```r
1
```

## Dollar [pending, error]

Why this test exists.

```r
a$
```

```r
b$
```
";

#[test]
fn parses_headings_markers_and_blocks() {
    let document = document(INPUT);

    assert_eq!(
        document.shape(),
        vec![
            ("Numbers", Markers::empty(), 1),
            ("Dollar", Markers::PENDING | Markers::ERROR, 2),
        ]
    );

    let sources: Vec<_> = document.tests[1]
        .blocks()
        .map(|block| block.source.as_str())
        .collect();

    assert_eq!(sources, vec!["a$\n", "b$\n"]);
}

/// Prose keeps its place in the element sequence, so rendering can put each
/// block's cells beside it
#[test]
fn prose_interleaves_with_blocks() {
    let document = document(INPUT);

    assert!(matches!(
        document.tests[1].elements.as_slice(),
        [
            Element::Prose(prose),
            Element::Block(_),
            Element::Block(_)
        ] if prose == "Why this test exists."
    ));
}

/// Textual idempotence is the contract, since CI regenerates and asserts a
/// clean tree. A cell whose content does not end its line gains a newline, so
/// the structural round-trip holds only for content that already ends in one.
#[test]
fn round_trips_through_a_snapshot() {
    let mut document = document(INPUT);
    document.tests[0].blocks_mut().next().unwrap().generated =
        vec![Cell::Cst("RRoot { }\n".to_string())];
    document.tests[1]
        .blocks_mut()
        .for_each(|block| block.generated = vec![Cell::Pending]);

    let rendered = document.render_snapshot("literals/numbers.md");
    assert_eq!(Document::parse(&rendered).unwrap(), document);
    assert_eq!(
        Document::parse(&rendered)
            .unwrap()
            .render_snapshot("literals/numbers.md"),
        rendered
    );
}

/// Loosely spaced input must converge after one pass rather than gaining a
/// blank line on every one
#[test]
fn rendering_is_idempotent_over_prose_spacing() {
    let loose = "## A\n\n\n\nSpaced out.\n\n\n\n```r\n1\n```\n";
    let rendered = document(loose).render_snapshot("a.md");

    assert_eq!(document(&rendered).render_snapshot("a.md"), rendered);
}

#[test]
fn an_empty_block_survives() {
    let document = document("## A\n\n```r\n```\n");
    assert_eq!(document.tests[0].blocks().next().unwrap().source, "");

    let rendered = document.render_snapshot("a.md");
    assert_eq!(Document::parse(&rendered).unwrap(), document);
}

/// A name may hold brackets of its own; only a trailing group is markers
#[test]
fn markers_are_the_trailing_bracket_group() {
    let document = document("## Subset2 With `]]` (#177) [error]\n\n```r\n1\n```\n");

    assert_eq!(
        document.shape(),
        vec![("Subset2 With `]]` (#177)", Markers::ERROR, 1)]
    );
}

#[test]
fn rejects_content_before_the_first_heading() {
    assert!(error("Stray prose.\n\n## A\n\n```r\n1\n```\n").contains("before the first"));
}

#[test]
fn rejects_a_heading_with_no_block() {
    assert!(error("## A\n\nJust prose.\n").contains("no ```r block"));
}

#[test]
fn rejects_a_duplicate_name() {
    let text = "## A\n\n```r\n1\n```\n\n## A\n\n```r\n2\n```\n";
    assert!(error(text).contains("duplicate test name"));
}

#[test]
fn rejects_an_unknown_marker() {
    assert!(error("## A [wip]\n\n```r\n1\n```\n").contains("unknown marker"));
}

#[test]
fn rejects_an_unterminated_cell() {
    assert!(error("## A\n\n```r\n1\n").contains("unterminated"));
}
