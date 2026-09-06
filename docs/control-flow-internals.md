# Control-flow checking

`alder-ast::flow` summarizes whether an expression or statement can continue
normally and whether it can return, break, or continue. Sequential composition
ignores unreachable successors; alternative branches combine possible exits.
Loops consume their own break and continue exits. Literal Boolean conditions
are understood; other conditions conservatively admit both branches. Calls and
deferred DSL constructs conservatively admit normal continuation.

Pattern flow distinguishes matching, rejection, and pin-expression exits.
Each alternative applies the arm guard with its own bindings: a false guard
retries the next alternative, not just the next arm. Guard exits stop that
path, while pattern rejection skips the guard. Both structural summaries and
solver reachability use this same per-alternative guard transition.

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

Generated loops carry explicit labels, and source break/continue statements
refer to their lexical loop label. A while condition is lowered before entering
that while body's target scope: its exits belong to an enclosing source loop,
even when condition setup is physically emitted inside the generated while.
This distinction also applies when the condition suspends via await.

Each loop expression starts with a fresh result variable. Reachable break
payloads join with the accumulated result; this unifies payload types while
retaining optional record presence from any exit. The final frame result,
rather than the first break's shape, becomes the loop type. A bare break
supplies unit. While/for bodies use a
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
and subject to ordinary type checks. Aggregate elements, record fields and
spreads, tag payloads, template interpolations, and index expressions carry
reachability forward in evaluation order, including contextually checked
initializers. Assignment indices precede later indices and the assigned value.
An exit in an earlier operand prevents later break payloads from joining the
enclosing loop result. Further unreachable-path constraints
(including pattern selection and general expression evaluation order) remain
in the hardening acceptance matrix. No interprocedural termination analysis
is claimed.
