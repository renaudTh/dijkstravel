export type Result<T, E> =
  { readonly ok: true; readonly value: T } | { readonly ok: false; readonly errors: readonly E[] };
