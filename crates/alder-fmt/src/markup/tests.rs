use crate::{Options, format_source, format_with};

fn checked(source: &str) -> String {
    let formatted = format_source(source).unwrap();
    assert_eq!(
        super::signature(source).unwrap(),
        super::signature(&formatted).unwrap()
    );
    assert_eq!(format_source(&formatted).unwrap(), formatted);
    formatted
}

#[test]
fn nested_about_markup_expands() {
    let source = "pub component page() {\n<main><h2>About</h2><p>First paragraph.</p><p>Second paragraph.</p></main>\n}\n";
    assert_eq!(
        checked(source),
        "pub component page() {\n    <main>\n        <h2>About</h2>\n        <p>First paragraph.</p>\n        <p>Second paragraph.</p>\n    </main>\n}\n"
    );
}

#[test]
fn whitespace_matrix_preserves_semantics() {
    for markup in [
        "<main>\n  <p>A</p>\n  <p>B</p>\n</main>",
        "<p>\n  Hello\n  world\n</p>",
        "<p>Hello <strong>Ada</strong>!</p>",
        "<p>Hello\n  <strong>Ada</strong>!</p>",
        "<p>Hello{\" \"}\n  <strong>Ada</strong>!</p>",
        "<p>Hello {name}!</p>",
        "<p>{name}\n  !</p>",
        "<p>A  B\tC</p>",
        "<p>{\"  A\\n B  \"}</p>",
        "<pre>\n  A\n\n B  </pre>",
        "<textarea>\n  A\n B  </textarea>",
        "<pre> \n <b> x\n  y </b> \n</pre>",
        "<p>Region {{\"Position {\"}} é😀</p>",
        "<p>\n  @iffy</p>",
        "<p><b>x</b>\n  @iffy</p>",
    ] {
        checked(&format!("pub component View() {{\n    {markup}\n}}\n"));
    }
}

#[test]
fn long_prose_wraps_without_splitting_repeated_spaces() {
    let source = "pub component View() {\n<p>First words followed by a long  repeated space then more words and more words here.</p>\n}\n";
    let options = Options {
        max_width: 48,
        ..Options::default()
    };
    let formatted = format_with(source, options).unwrap();
    assert!(formatted.contains("    <p>\n"));
    assert!(formatted.contains("long  repeated"));
    assert_eq!(format_with(&formatted, options).unwrap(), formatted);
    assert_eq!(
        super::signature(source).unwrap(),
        super::signature(&formatted).unwrap()
    );
}

#[test]
fn attributes_and_event_blocks_are_indented() {
    let formatted = checked(
        "pub component View() {\nlet count = state(0)\n<button id=\"counter\" onClick={() -> async { count += 1\nmatch count { 1 => { count += 1\ncount += 2 }, _ => {} } }}>Click</button>\n}\n",
    );
    assert!(
        formatted.contains("\n        onClick={() -> async {\n            count += 1"),
        "{formatted}"
    );
    assert!(formatted.contains("1 => {\n"), "{formatted}");
}

#[test]
fn directives_and_fragments_expand() {
    checked(
        "pub component View() {\n<><div>@if enabled { <span>Yes</span> } @else { <span>No</span> }@for item in items; key item.id { <b>{item.name}</b> } @empty { <i>Empty</i> }@match choice { Some(x) => <p>{x}</p>, None => { <p>None</p> } }</div></>\n}\n",
    );
}

#[test]
fn comments_between_attributes_and_directive_parts_survive() {
    checked(
        "pub component View() {\n<div // element\n id=\"x\" // id\n>@match choice { // arms\nSome(x) => <b>{x}</b>, // next\nNone => {\n// setup\nlet x = 1\n<b>{x}</b>\n} }</div>\n}\n",
    );
}

#[test]
fn raw_macro_and_template_payloads_in_handlers_are_preserved() {
    checked(
        "pub component View() {\n<button onClick={() -> { log(\"a \\\" { b }\")\ncall!( { raw } )\nlog(`a {b}`) }}>Go</button>\n}\n",
    );
}

#[test]
fn comments_in_embedded_code_survive() {
    checked(
        "pub component View() {\n<button onClick={() -> { // why\nrun() // done\n}}>Run</button>\n}\n",
    );
}

#[test]
fn comments_in_directive_headers_and_holes_survive() {
    checked(
        "pub component View() {\n<div>@if // condition\nenabled // body\n{ <b>yes</b> } @else { <b>no</b> }@for item in // iterable\nitems; key item.id { <b>{ // value\nitem.name // end\n}</b> }@match // select\nchoice { Some(x) // pattern\nif x > 0 => // arm\n<b>{x}</b> }</div>\n}\n",
    );
}

#[test]
fn comments_inside_attribute_delimiters_survive() {
    checked(
        "pub component View() {\n<button onClick // assignment\n= { // callback\n() -> run() // tail\n} id // name\n= \"x\">Run</button // close\n>\n}\n",
    );
}
