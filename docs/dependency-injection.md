# Services and dependency injection

Status: agreed architectural direction, deferred implementation. This document
supersedes the earlier `use`/scoped-`provide` design as the intended public DI
model. No new syntax or compile-time DI guarantees are implemented by this
decision. Implementation planning lives in `plans/dependency-injection.md`.

## Direction

Use Effect-style services and layers with Alder-native syntax and compiler
integration: typed service identities, dependency-aware provider factories,
composition roots, scoped resource ownership, and statically checked service
requirements. This is more than implicitly passing context values. The system
must construct and own the dependency graph, not require users to manually wire
every node or nest their application in `provide` blocks.

Effect already tracks service requirements statically; that guarantee is not an
Alder invention. Its layers separate construction dependencies from the service
interface consumed by callers. Alder should preserve that distinction while
generating wiring for statically declared graphs. See Effect's
[services](https://effect.website/docs/v3/requirements-management/services) and
[layers](https://effect.website/docs/v3/requirements-management/layers) documentation.
These references explain the model, not a commitment to a particular Effect API
version or runtime implementation.

## Service contracts and requirements

- A service has a nominal identity and a typed interface. It is distinct from a
  trait: traits describe type capabilities; services identify runtime dependencies.
- Match requirements by service identity, not local parameter names. In
  `#[using(db: Database)]`, `db` is only a local binding. Distinct identities such
  as `PrimaryDatabase` and `ReplicaDatabase` distinguish same-shaped services.
- Retain the agreed annotation placement above functions:
  `#[using(db: Database, cache: Cache)]`. This is compiler syntax, not a macro,
  and does not depend on M5.
- Initially require explicit service requirements on named functions, including
  forwarding callers. Calls check requirements using function contracts rather
  than requiring whole-program call-graph reconstruction.
- Function values, callbacks, async tasks, and exported interfaces must retain
  the relevant requirements or captured service bindings. An annotation is not
  merely declaration metadata that can disappear when a function is passed around.
- A constructed service captures its implementation dependencies. A caller of
  `Users` should not need to declare `Database` just because one Users provider
  uses a database internally.

## Providers and composition

A provider constructs a service from declared dependencies. It may return a
plain value, perform asynchronous initialization, or fail with an explicit
Result. Resource providers additionally establish cleanup ownership.

A composition root selects providers and exposes a validated graph to an
application entry point, a request boundary, or a test. Construction order follows
dependencies, not textual registration order. Only services reachable from the
selected roots need construction. Share instances within their declared lifetime.

The following is a syntax sketch, **not accepted grammar or executable Alder**:

```text
service Users {
    async fn find(id: UserId) Result[Option[User]]
    async fn save(user: User) Result[()]
}

#[provider]
#[using(db: Database, log: Logger)]
fn postgresUsers() Users {
    // Construct Users operations that capture db and log.
}

dependencies App {
    application {
        Logger = consoleLogger
        Database = postgresDatabase
        Users = postgresUsers
    }
    request {
        CurrentUser = authenticateRequest
    }
}

#[inject(App)]
#[using(users: Users)]
pub async fn main() Result[()] {
    // Use users; the root owns initialization and cleanup.
}
```

`service`, `#[provider]`, `dependencies`, and `#[inject]` are provisional
spellings. The architecture and annotation placement are the agreed direction;
the exact service-value construction syntax and type representation still need
a compiler contract. Requirements should not further crowd return types and
`where` clauses in ordinary function declarations.

## Lifetimes and resource ownership

The proposed initial lifetime categories are application and request. These are
Alder-specific design choices, not guarantees inherited automatically from Effect.

- Application services live within one application root, not a process-global
  mutable registry. Request services live within an explicitly created request
  scope; an ordinary function call does not silently create that scope.
- Request services may depend on application services. Application services must
  not retain request services. Nested lifetime dependencies must be validated.
- Prefer lexical service capture: constructed implementations, closures, and
  tasks retain supplied values rather than performing execution-time lookups in
  whichever fiber happens to run them.
- Scope-bound services must not escape through closures, returned values, or
  detached tasks. This requires a concrete tracking/restriction design before
  implementation can promise safety. Unsupported escapes must be rejected;
  dependency graph validation alone is insufficient.
- Acquire dependencies first; register finalizers immediately after acquisition.
  On startup failure, clean up already acquired resources. On shutdown, finish
  or cancel scope-owned work before releasing services, and release dependents
  before their dependencies. Cancellation and cleanup failures need explicit rules.
- Begin with deterministic sequential initialization. Parallel initialization,
  transient lifetimes, and dynamic registration are not initial requirements.

The compiler can prove a graph is satisfiable; it cannot prove a database will
connect successfully. Acquisition failures remain ordinary runtime errors, and
foreign code remains a trust boundary.

## Tests and overrides

Tests should reuse application composition and replace selected provider bindings,
without modifying consumers or reconstructing the entire graph. For example,
`dependencies TestApp extends App` could replace Database with an in-memory
provider. The resulting graph must be validated again. Each test owns a fresh
root and cleanup scope; replacements must not leak between tests.

## Required static checks

Missing service bindings, duplicate/ambiguous bindings, provider output type
mismatches, dependency cycles, invalid lifetime dependencies, unsatisfied call or
entry-point requirements, and unsupported scope escapes must be compile errors.
Diagnostics should identify both the consumer requirement and the relevant
provider/composition declaration. Compiler-generated hidden arguments or typed
environments are implementation options, not a user-accessible service locator.

## Existing implementation and migration

The compiler currently has top-level value dependency groups for inference and
recursion, not a complete higher-order call graph. Provider requirement inference
is absent: `Stmt::Use` is ignored by the solver and provider references currently
infer as `Any`. Existing runtime code carries fiber-local context and scoped
restoration/inheritance. That is infrastructure, not the proposed DI system.

The current parser accepts `use` and a value-producing `provide` expression.
Do not remove or change that runtime behavior as part of documenting this design.
When implementation resumes, define its migration explicitly and test it against
the new lexical-capture and resource-lifetime contracts. The language reference
must distinguish existing behavior from the new guarantees until acceptance.
