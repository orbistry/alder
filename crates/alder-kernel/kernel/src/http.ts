// Typed Alder facade over the host's WHATWG Fetch objects. Body reads and fetch
// remain lazy Tasks; rejected I/O becomes an explicit error-row Result.
function httpOptions(options) {
    if (options === null || options === undefined) return {};
    const value = optionValue(options);
    const result = {};
    for (const key of ["status", "method", "body"]) {
        if (value[key] !== null && value[key] !== undefined) result[key] = optionValue(value[key]);
    }
    if (value.headers !== null && value.headers !== undefined) {
        result.headers = new Headers(optionValue(value.headers));
    }
    return result;
}

export function $httpText(body, options = null) {
    const init = httpOptions(options);
    init.headers ??= new Headers();
    if (!init.headers.has("content-type")) init.headers.set("content-type", "text/plain; charset=utf-8");
    return new Response(body, init);
}

export function $httpJson(dictionary, body, options = null) {
    const init = httpOptions(options);
    init.headers ??= new Headers();
    if (!init.headers.has("content-type")) init.headers.set("content-type", "application/json; charset=utf-8");
    return new Response(dictionary.encode(body), init);
}

export function $httpEmpty(status = null) {
    return new Response(null, {status: status === null ? 204 : optionValue(status)});
}

export function $httpRedirect(location, status = null) {
    const code = status === null ? 303 : optionValue(status);
    if (![301, 302, 303, 307, 308].includes(code)) throw new RangeError("redirect status must be 301, 302, 303, 307, or 308");
    return new Response(null, {status: code, headers: {location}});
}

export function $httpStatus(response) { return response.status; }
export function $httpResponseHeader(response, name) { return response.headers.get(name); }
export function $httpRequestHeader(request, name) { return request.headers.get(name); }
export function $httpMethod(request) { return request.method; }
export function $httpRequestUrl(request) { return request.url; }
export function $httpPathname(url) { return url.pathname; }
export function $httpSearchParam(url, name) { return url.searchParams.get(name); }
export function $httpRequest(url, options = null) { return new Request(url, httpOptions(options)); }

export function $httpWithHeader(response, name, value) {
    const headers = new Headers(response.headers);
    headers.set(name, value);
    // Retain the stream. Consuming either response consumes this same body.
    return new Response(response.body, {status: response.status, statusText: response.statusText, headers});
}

function httpFailure(tag, error) {
    return {$: "Err", _0: {$: tag, _0: error instanceof Error ? error.message : String(error)}};
}

export function $httpReadText(request) {
    return $tryPromise(async () => $resultOk(await request.text()), false,
        "http.readText", error => httpFailure(":body_error", error));
}

export function $httpResponseText(response) {
    return $tryPromise(async () => $resultOk(await response.text()), false,
        "http.responseText", error => httpFailure(":body_error", error));
}

export function $httpReadJson(dictionary, request) {
    return $task(function* () {
        const body = yield* $httpReadText(request);
        return body.$ === "Ok" ? dictionary.decode(body._0) : body;
    });
}

const httpFetchHookKey = Symbol.for("alder/httpFetchHook/v1");

function httpNativeFetch(request) {
    return $tryPromise(async signal => $resultOk(await fetch(request, {signal})), true,
        "http.fetch", error => httpFailure(":network_error", error));
}

export function $httpFetch(request) {
    return $task(function* () {
        const hooks = (currentFiber?.context ?? synchronousProviderContext).get(httpFetchHookKey);
        const hook = hooks?.[hooks.length - 1];
        return yield* (hook ? hook(request) : httpNativeFetch(request));
    });
}
