export function answer(value, signal) {
  if (signal === undefined) throw new Error("missing AbortSignal");
  return Promise.resolve(value);
}
