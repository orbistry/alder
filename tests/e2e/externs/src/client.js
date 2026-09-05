export function answer() { return Promise.resolve(42); }
export function boundedIdentity(value) {
    if (arguments.length !== 1) throw new Error("hidden dictionaries leaked to JS");
    return value;
}
export function boundedAsync(value, signal) {
    if (arguments.length !== 2 || !(signal instanceof AbortSignal)) {
        throw new Error("incorrect bounded async extern arguments");
    }
    return Promise.resolve(value);
}
export function fulfilled(value) { return Promise.resolve(value); }

let aborted = 0;
export function pending(signal) {
    signal.addEventListener("abort", () => { aborted += 1; }, { once: true });
    return new Promise(() => {});
}
export function abortCount() { return aborted; }
export function throws() { throw new Error("sync wrapper failure"); }
export function rejects() { return Promise.reject(new Error("rejected wrapper failure")); }
