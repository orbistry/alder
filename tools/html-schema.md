# Reproducible HTML schema

`html-schema-source.html.gz` is an unchanged, gzip-compressed snapshot of the
[WHATWG HTML Living Standard indices](https://html.spec.whatwg.org/multipage/indices.html),
retrieved 2026-09-14. WHATWG and its contributors publish the source under the
[WHATWG copyright policy](https://whatwg.org/ipr-policy#copyright).
The generator records the uncompressed source SHA-256 in the generated Rust
header; gzip metadata uses a fixed timestamp.

Regenerate or verify without network access (Python 3 and rustfmt required):

```sh
python3 tools/generate-html-schema.py
python3 tools/generate-html-schema.py --check
```

Only `--refresh` fetches the Living Standard and replaces the pinned snapshot.
Review both the source digest and generated table changes when refreshing.
`--source PATH` accepts a different offline HTML or `.gz` input and `--output`
selects another output. Ordinary Cargo builds use the checked-in Rust table and
never run the generator.

The current table has 115 elements and 31 global attributes. Element-specific
attributes and their String/Number/Bool policy are derived from the index; a
small explicit override list resolves overloaded attribute descriptions.
ARIA vocabulary, readable event records, custom-element rules, and structural
nesting checks are maintained separately in the compiler. This is not a full
validator for all WHATWG conditional content rules or cross-component markup.
