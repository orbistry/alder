# Control-flow checking

`alder-ast::flow` summarizes whether an expression or statement can continue
normally and whether it can return, break, or continue. Sequential composition
ignores unreachable successors; alternative branches combine possible exits.
Loops consume their own break and continue exits. Literal Boolean conditions
are understood; other conditions conservatively admit both branches. Calls and
deferred DSL constructs conservatively admit normal continuation.

The active solver checks every inferred body with normal fallthrough against
the function result. A block that cannot continue produces a fresh unconstrained
value type for expression composition: there is no runtime value to constrain.
Explicit return expressions are still checked against their enclosing function
result. Creating a lambda does not execute its body or return from its creator.
An empty while/for body, or merely containing a return inside one, does not prove
that the containing function returns a value.

Codegen evaluates loop-body tail expressions and discards their values, not
their effects. Break payloads are evaluated even when the surrounding statement
loop has no result slot. While/for bodies isolate themselves from enclosing
loop-expression result slots.

This is an initial control-flow checkpoint, not the entire loop implementation:
valued-break inference, further unreachable-path constraints, function-boundary
loop targets, and additional expression-order interactions remain in the
hardening acceptance matrix. No interprocedural termination analysis is claimed.
