# Formatter preservation contract

The current formatter changes horizontal indentation and trailing layout spaces,
not token spelling or internal line breaks. It parses input before formatting
and parses output before returning it.

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

Before returning output it compares comment payloads, every verbatim source
slice, and each physical line's non-boundary-whitespace contents. Internal
blank lines remain in place. Together with reparsing, these checks enforce the
current restricted transformation contract; they are not a general-purpose AST
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
