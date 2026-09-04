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

Each loop expression owns a fresh result variable. Reachable break payloads
unify with that variable; a bare break supplies unit. While/for bodies use a
unit target instead, and nested loops do not constrain an enclosing target.
Lambda inference saves and clears the target stack, restoring it afterward.
A loop with no structurally reachable exit remains divergent rather than
producing unit.

Inference tracks reachability across block statements, literal conditional
branches, Boolean short circuits, and match guards. A break after another
unconditional exit, inside `if false`, in a skipped Boolean operand, or behind
a literal false guard does not determine a live loop's result. The structural
flow summary also excludes these exits when deciding whether a loop diverges.
Unknown Boolean values remain conservative. Unreachable code is still inferred
and subject to ordinary type checks. Further unreachable-path constraints
(including pattern selection and general expression evaluation order) remain
in the hardening acceptance matrix. No interprocedural termination analysis
is claimed.
