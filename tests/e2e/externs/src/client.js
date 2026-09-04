export function answer() { return Promise.resolve(42); }
export function fulfilled(value) { return Promise.resolve(value); }

let aborted = 0;
export function pending(signal) {
    signal.addEventListener("abort", () => { aborted += 1; }, { once: true });
    return new Promise(() => {});
}
export function abortCount() { return aborted; }
export function throws() { throw new Error("sync wrapper failure"); }
export function rejects() { return Promise.reject(new Error("rejected wrapper failure")); }
