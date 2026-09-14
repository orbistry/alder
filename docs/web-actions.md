# Page actions and text forms

The M6 action contract is independent of the future M8 schema language.
`+page.server.ald` can export a closed record of named actions:

```alder
async fn save(input: { name: String }) Result[{ id: Number }, [:invalid_name(String)]] {
    if input.name == "" { Err(:invalid_name("required")) } else { Ok({ id: 42 }) }
}

pub let actions = { save: save }
```

Each action takes exactly one closed input record and returns
`Task[Result[Output, Errors]]`. The compiler generates argument and result wire
validators from the solved types. Inputs, outputs, and errors must be supported
wire values; opaque host handles, callback values, and unresolved type variables
are not allowed. The browser-facing action record retains the same solved type,
so `actions.save({ name: "Ada" }).await` has the exact declared Result type.
HTTP dispatch stays on the current page URL, preserving its route parameters
and request hooks. A route identity accompanies the action name, and the server
validates both before invocation.

Use normal HTML forms with `html.submit(handle)` as the `onSubmit` callback.
The helper prevents native navigation and captures fields synchronously before
starting the lazy Task returned by `handle`. This timing matters: browsers clear
`currentTarget` after event dispatch, and an async Alder function does not start
running until the Task scheduler runs it.

```alder
import html
import map

async fn submitted(fields: Result[Map[String, Array[String]], [:invalid_form(String) | :file_field(String)]]) {
    // Match fields, read map.get(values, "name"), and validate/coerce user input.
    // Then await actions.save({ name: validatedName }) and handle its Result.
    ()
}

pub component Form() {
    <form onSubmit={html.submit(submitted)}>
        <input name="name" />
        <button type="submit" name="intent" value="save">Save</button>
    </form>
```

`html.formValues(event)` returns all text values as
`Result[Map[String, Array[String]], [:invalid_form(String) | :file_field(String)]]`.
Repeated names remain arrays, empty fields remain empty strings, and the actual
submit button is included. Files produce `:file_field(fieldName)`; they are not
silently converted to text. `html.preventDefault(event)` and `formValues` can
also be called directly inside a synchronous handler before its first await.

For native server-side form reads, `http.readForm(request)` is a lazy Task
returning the same text/multivalue map, with `:body_error(String)` for malformed
or already-consumed bodies and `:file_field(String)` for files. These primitives
do not infer number/boolean/date conversions, schema constraints, or a field
component system. Application validation returns its own typed Result errors;
schema-driven Form/Field components remain part of M8.

Action functions are ordinary typed Task producers: a component can store their
Result in state or refresh an existing resource after a successful mutation.
The action transport does not automatically invent a resource or submit it on
component initialization.
