use super::KERNEL_JS;

async fn run(source: &str) {
    let code = format!("{KERNEL_JS}\n{source}");
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        alder_runtime::execute(code, vec![]),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn http_responses_preserve_headers_status_and_native_streams() {
    run(r#"
        const headers = new Map([["x-result", "created"]]);
        const response = $httpText("Hello & goodbye", {status: 201, headers});
        $assert(response instanceof Response && $httpStatus(response) === 201);
        $assert($httpResponseHeader(response, "x-result") === "created");
        $assert($httpResponseHeader(response, "missing") === null);
        $assert($httpResponseHeader(response, "content-type") === "text/plain; charset=utf-8");
        const modified = $httpWithHeader(response, "x-extra", "yes");
        $assert(response.headers.get("x-extra") === null);
        $assert(modified.headers.get("x-extra") === "yes");
        $assert((await $runTask($httpResponseText(modified)))._0 === "Hello & goodbye");
        $assert(response.bodyUsed);
        $assert($httpEmpty().status === 204);
        $assert($httpRedirect("/next").headers.get("location") === "/next");
        $assert($httpRedirect("/next", 307).status === 307);
        const json = $httpJson({encode: value => JSON.stringify({value})}, 42);
        $assert(await json.text() === '{"value":42}');
        $assert(json.headers.get("content-type") === "application/json; charset=utf-8");
    "#)
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn http_request_reads_are_lazy_and_io_errors_are_results() {
    run(r#"
        const request = $httpRequest("https://example.test/users?id=42", {
            method: "POST", body: "42", headers: new Map([["content-type", "application/json"]])
        });
        $assert($httpMethod(request) === "POST");
        $assert($httpRequestUrl(request) === "https://example.test/users?id=42");
        $assert($httpRequestHeader(request, "content-type") === "application/json");
        const url = new URL(request.url);
        $assert($httpPathname(url) === "/users" && $httpSearchParam(url, "id") === "42");
        $assert($httpSearchParam(url, "missing") === null);
        const body = $httpReadText(request);
        $assert(!request.bodyUsed);
        $assert((await $runTask(body))._0 === "42");
        const repeated = await $runTask(body);
        $assert(repeated.$ === "Err" && repeated._0.$ === ":body_error");
        const dictionary = {decode: value => $jsonDecodePrimitive(value, "number")};
        const valid = $httpRequest("https://example.test", {method: "POST", body: "42", headers: null});
        $assert((await $runTask($httpReadJson(dictionary, valid)))._0 === 42);
        const invalid = $httpRequest("https://example.test", {method: "POST", body: "invalid", headers: null});
        const result = await $runTask($httpReadJson(dictionary, invalid));
        $assert(result.$ === "Err" && result._0.$ === ":invalid_json");
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn http_fetch_is_lazy_cancellable_and_has_typed_network_failures() {
    run(r#"
        const original = globalThis.fetch;
        let started = 0;
        let aborted = false;
        globalThis.fetch = (request, options) => {
            started++;
            return new Promise((resolve, reject) => {
                options.signal.addEventListener("abort", () => { aborted = true; reject(new Error("aborted")); });
            });
        };
        try {
            const task = $httpFetch($httpRequest("https://example.test"));
            $assert(started === 0);
            await $runTask($task(function* () {
                const fiber = yield* $fiberFork(task);
                yield* $taskSleep(1);
                yield* $fiberInterrupt(fiber);
            }));
            $assert(started === 1 && aborted);
            globalThis.fetch = async () => { throw new Error("offline"); };
            const failed = await $runTask($httpFetch($httpRequest("https://example.test")));
            $assert(failed.$ === "Err" && failed._0.$ === ":network_error" && failed._0._0 === "offline");
        } finally { globalThis.fetch = original; }
    "#).await;
}
