# Hook, endpoint, and error contracts

`hooks.server.ald` recognizes these solved contracts:

- `handle(event: RequestEvent[p], resolve: fn(RequestEvent[p]) Task[Response])`
  returns `Response` or `Task[Response]`.
- `handleFetch(event: RequestEvent[p], request: Request, next: Fetch)` returns
  `Task[Result[Response, [:network_error(String)]]]`.
- `handleError(err: Error, event: RequestEvent[p])` returns Unit or Task[Unit].

Here the types are imported from `http`. A global hook must work for every
route's parameters: `p` can remain generic, but a global hook cannot assume that
all requests have an `id` parameter. The compiler also accepts inferred generic
handlers such as `pub fn handle(event, resolve) { resolve(event) }`.

`hooks.client.ald` recognizes `init()` and `handleError(err: Error)`, both returning
Unit or Task[Unit]. Error hooks report unexpected errors; they do not transform
expected Result errors or return arbitrary HTTP responses.

Client reporting covers init failures, initial component setup/render failures,
navigation failures, synchronous/Task event defects, and unexpected resource
defects. Expected Resource/Result failures remain typed values. Reports belong
to the render owner/session, so late failures from disposed event owners do not
reach a replacement session after navigation or hot update. Initial render
failures are reported and still fail startup; reporting does not silently mount
an incomplete application.

`provide Session = ...` and `use Session` use the existing typed provider/fiber
context: value fields are checked, and bindings survive awaited tasks within a
request. They do not prove transitive provider availability statically; a
missing runtime provider still fails explicitly. Service/layer composition and
the stronger dependency-injection guarantees remain in
`plans/dependency-injection.md`.

HTTP methods exported by `+server.ald` take their route's RequestEvent and return
a native `http.Response` or Task[Response]. An ordinary record with a `body`
field is not a Response, and Result[Response, e] is not silently unwrapped.
The method names are get, head, post, put, patch, delete, and options.

## Reporting versus public errors

`http.Error` is `{name: String, message: String, stack: Option[String]}` for
unexpected-error reporting. Stack details are not part of a page payload.
`http.PublicError` is `{status: Number, message: String}`; unexpected production
failures expose a generic message. Development may expose the error message,
but not a server stack in the serialized page data.

`http.PageError[expected]` is an enum:

```alder
pub enum PageError[expected] {
    Expected(expected),
    Unexpected(PublicError),
}
```

Each `+error.ald` receives a generated concrete PageError alias whose expected
payload is the union of tagged loader errors for routes using that boundary.
The same error tag must have a compatible payload everywhere in that union.
The component may be static or accept `{error: PageError}`. For example:

```alder
import http.{PageError as ErrorKind}

pub component error(props: { error: PageError }) {
    @match props.error {
        ErrorKind::Expected(_) => <p>The requested page could not be loaded.</p>,
        ErrorKind::Unexpected(problem) => <p>{problem.message}</p>,
    }
}
```

The implicit `PageError` is the boundary's concrete type alias, not an enum
constructor namespace. Import the enum under a separate name such as
`ErrorKind` to match its `Expected` and `Unexpected` constructors.

Error props deliberately do not promise PageData: a loader can fail before any
or all data fields exist. Reading `{data: PageData}` in an error component is
therefore rejected. Expected loader failures keep their typed tagged payload;
unexpected exceptions are wrapped separately rather than masquerading as a
tagged application error.
