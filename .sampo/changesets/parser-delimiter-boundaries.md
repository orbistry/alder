---
cargo/alder-parse: minor
cargo/alder-driver: patch
---

Retain pre-whitespace/comment boundaries in shared delimiter errors, including
parser backtracking. Report cross-line missing punctuation at the insertion
boundary and label the actual opening punctuation, not the enclosing declaration.
Keep detection evidence internal rather than labeling valid following code, retain
actual mismatched closer locations, and distinguish required separators from list
entries in CLI/editor messages. Delimiter error variants now carry ExpectedEnd
instead of a row/column pair; error-row extension and non-select query closers have
separate variants so their messages do not offer invalid continuations.
