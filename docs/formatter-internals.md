# Formatter preservation contract

The formatter canonicalizes import runs, then changes horizontal indentation
and trailing layout spaces elsewhere. It parses input before formatting and
parses output before returning it.

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

The formatter preserves every physical line intersecting a verbatim range,
including its original line ending. It masks those ranges when counting code
delimiters so braces/backticks in payloads cannot affect surrounding indentation.
This deliberately leaves layout inside templates, markup, and raw macros alone.
CRLF and literal carriage returns are not globally normalized.

After normalizing imports, it compares comment payloads, every verbatim source
slice, and each physical line's non-boundary-whitespace contents. Internal
blank lines remain in place. Together with reparsing, these checks enforce the
restricted non-import transformation contract; they are not a general-purpose AST
equivalence checker. Future token rewriting or line reflow must add an
appropriate structural-equivalence check before broadening that contract.

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
