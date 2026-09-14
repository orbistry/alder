# Formatter preservation contract

The formatter canonicalizes import runs, lays out parsed markup, then changes
horizontal indentation and trailing layout spaces elsewhere. It parses input
before formatting and parses output before returning it.

`imports.rs` flattens grouped entries with their source regions, retains
visibility and import tails, and sorts each consecutive same-visibility run by
root (bundled, local, external) and module path. Aliases do not affect sorting.
One entry stays ungrouped; larger runs have one entry per line, trailing commas,
and exactly one blank line between nonempty root sections. Declarations and
visibility changes are boundaries. Adjacent leading and trailing comments
travel with an entry. Blank-separated comments outside a run stay in place;
standalone comments inside a run become a blank-separated preamble in their
original order. Comments inside selections move before the owning import.
Import and comment multisets are checked after this rewrite, including exact
comment spelling and public visibility.

`alder-parse::parse_module_with_verbatim` also returns byte ranges for templates
(including interpolation), markup expressions, raw macro bodies/calls, and
comments. These ranges are recorded by the relevant parser productions, not a
second approximation of the language grammar. Parser backtracking truncates
the range side table together with the comment side table. The ordinary source
AST is unchanged.

`markup.rs` formats outer markup spans using parsed elements, attributes,
children, and directive blocks. Literal text has already undergone the folding
rules in [markup-whitespace.md](markup-whitespace.md). Structural children are
indented on separate lines; prose wraps at single separating spaces. Mixed
inline children stay adjacent, with optional line breaks around the whole
sequence only when its boundary whitespace is unaffected. Long unbreakable
tokens and semantically significant inline sequences may exceed the width.
Embedded code blocks expand at brace boundaries while preserving existing
statement newlines, comments, literal/template payloads, and raw macros.
Whitespace-sensitive `<pre>`/`<textarea>` subtrees remain verbatim.

The markup pass compares complete parsed Debug structures before and after,
excluding only `Region`/`Position` metadata. Quoted payloads are scanned intact,
so location-shaped text is not erased. This checks expression structure, child
order, normalized text, exact literal values, and comments; a parse or semantic
mismatch fails before any files are written. It does not equate arbitrary AST
rewrites, merge text nodes, or replace text with expression holes.

After the markup pass, the indentation pass preserves every physical line intersecting a verbatim range,
including its original line ending. It masks those ranges when counting code
delimiters so braces/backticks in payloads cannot affect surrounding indentation.
This leaves the already formatted markup and literal/template/raw macro payloads alone.
CRLF and literal carriage returns are not globally normalized.

After markup layout, it compares comment payloads, every verbatim source
slice, and each physical line's non-boundary-whitespace contents. Internal
blank lines remain in place. Together with reparsing, these checks enforce the
restricted indentation transformation contract; the markup pass has its own
structural-equivalence guard described above.

Tests separately check cooked literal values, byte preservation, and idempotence.
They cover whitespace-only lines, trailing spaces, nested interpolation/escapes,
CRLF, literal CR, raw macros, markup, comments, and parser lookahead rollback.
The CLI computes and validates all outputs before writing any file, with a
regression confirming that an invalid file leaves earlier valid files untouched.
An end-to-end CLI regression formats a temporary application, checks formatting
idempotence, bundles it, and executes assertions on the multiline template's
exact value and length. The test also requires a real indentation change.
Import tests check root/path sorting, comment attachment, visibility boundaries,
single-entry collapse, nested paths, CRLF, and idempotence. An execution test
compares pre-format and post-format bundles byte-for-byte and asserts effects,
shared identities, namespace re-export chains, and exactly-once initialization.
