// HS2-MVF6X3: the plain-Node scripts cannot import `src/machine-home.ts`, so this twin of its
// `nonBlank` gives them the same blank-as-unset rule for path-valued HOTSHEET_* variables. A failed
// `$(...)` substitution leaves an empty or whitespace-only value, which must fall back to the
// script's default rather than resolve against the working directory. `machine-home.test.ts` runs
// both functions on the same inputs, and `eslint-policy.test.ts` requires each read to go through it.

/**
 * `value`, or `undefined` when it is absent, empty, or whitespace-only.
 * @param {string | undefined} value
 * @returns {string | undefined}
 */
export function nonBlank(value) {
  return value === undefined || value.trim() === '' ? undefined : value;
}
