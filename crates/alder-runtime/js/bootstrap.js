// The Deno extension crates expose standards implementations as internal
// modules. Alder installs its deliberate standalone global surface here rather
// than inheriting the Deno CLI bootstrap.
const load = Deno.core.loadExtScript;
// Fetch integrates with Deno telemetry when the CLI installs it. Alder keeps
// tracing disabled until its own observability contract lands.
__bootstrap.internals.__telemetry = {
  TRACING_ENABLED: false,
  PROPAGATORS: [],
  builtinTracer: undefined,
  ContextManager: undefined,
  enterSpan: undefined,
  restoreSnapshot: undefined,
};
__bootstrap.internals.__telemetryUtil = {
  updateSpanFromClientResponse: undefined,
  updateSpanFromError: undefined,
  updateSpanFromRequest: undefined,
};
const domException = load("ext:deno_web/01_dom_exception.js");
const event = load("ext:deno_web/02_event.js");
const abortSignal = load("ext:deno_web/03_abort_signal.js");
const streams = load("ext:deno_web/06_streams.js");
const encoding = load("ext:deno_web/08_text_encoding.js");
const file = load("ext:deno_web/09_file.js");
const url = load("ext:deno_web/00_url.js");
const headers = load("ext:deno_fetch/20_headers.js");
const formData = load("ext:deno_fetch/21_formdata.js");
const request = load("ext:deno_fetch/23_request.js");
const response = load("ext:deno_fetch/23_response.js");
const fetchApi = load("ext:deno_fetch/26_fetch.js");
const cryptoApi = load("ext:deno_crypto/00_crypto.js");

Deno.core.setWasmStreamingCallback(fetchApi.handleWasmStreaming);
Object.assign(globalThis, {
  DOMException: domException.DOMException,
  Event: event.Event,
  EventTarget: event.EventTarget,
  CustomEvent: event.CustomEvent,
  AbortController: abortSignal.AbortController,
  AbortSignal: abortSignal.AbortSignal,
  ReadableStream: streams.ReadableStream,
  WritableStream: streams.WritableStream,
  TransformStream: streams.TransformStream,
  TextEncoder: encoding.TextEncoder,
  TextDecoder: encoding.TextDecoder,
  Blob: file.Blob,
  File: file.File,
  URL: url.URL,
  URLSearchParams: url.URLSearchParams,
  Headers: headers.Headers,
  FormData: formData.FormData,
  Request: request.Request,
  Response: response.Response,
  fetch: fetchApi.fetch,
  Crypto: cryptoApi.Crypto,
  CryptoKey: cryptoApi.CryptoKey,
  SubtleCrypto: cryptoApi.SubtleCrypto,
  crypto: cryptoApi.crypto,
});
