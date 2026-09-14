// Preserve repeated fields without guessing application validation/coercions.
function webFormEntries(data) {
    const values = new Map();
    for (const [name, value] of data.entries()) {
        if (typeof value !== "string") return $resultErr({$: ":file_field", _0: name});
        const entries = values.get(name);
        if (entries) entries.push(value);
        else values.set(name, [value]);
    }
    return $resultOk(values);
}

export function $webPreventDefault(event) {
    event.preventDefault();
}

export function $webFormValues(event) {
    try {
        const form = event.currentTarget;
        if (!form || form.tagName?.toLowerCase() !== "form") {
            return $resultErr({$: ":invalid_form", _0: "formValues requires a form submit event"});
        }
        return webFormEntries(new FormData(form, event.submitter || undefined));
    } catch (error) {
        return httpFailure(":invalid_form", error);
    }
}

export function $webSubmit(handle) {
    return event => {
        // These must happen before scheduling the lazy Alder handler Task:
        // browsers clear currentTarget and run native submission afterwards.
        event.preventDefault();
        return handle($webFormValues(event));
    };
}

export function $httpReadForm(request) {
    return $tryPromise(async () => webFormEntries(await request.formData()), false,
        "http.readForm", error => httpFailure(":body_error", error));
}
