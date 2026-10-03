/**
 * Faber TypeScript runtime package (`@faber/runtime`).
 *
 * Each runtime namespace is a module of this package, re-exported here under
 * its namespace name: generated TypeScript imports the namespaces it uses
 * (`import { display } from "@faber/runtime"`) and calls them qualified
 * (`display.value(x, hint)`).
 *
 * Node loads this file as TypeScript source (type stripping) and resolves a
 * relative import only with its real `.ts` extension; a consumer's `tsc`
 * (strict, no `allowImportingTsExtensions`) rejects that extension on an
 * imported package file (TS5097). The package is the one place that needs both,
 * so the diagnostic is suppressed on each re-export rather than in every
 * consumer's flags.
 */

// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as display from "./display.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as exact from "./exact.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as dec from "./dec.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as inf from "./inf.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as format from "./format.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as modulus from "./modulus.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as tensor from "./tensor.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as sparsa from "./sparsa.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as matrix from "./matrix.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as vector from "./vector.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as intervallum from "./intervallum.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as instans from "./instans.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as order from "./order.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as dup from "./dup.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as json from "./json.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as valor from "./valor.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as variant from "./variant.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as tuple from "./tuple.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as atomic from "./atomic.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as result from "./result.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as sermo from "./sermo.ts";
// @ts-ignore TS5097: the `.ts` extension is required by Node, not by tsc.
export * as routes from "./routes.ts";
