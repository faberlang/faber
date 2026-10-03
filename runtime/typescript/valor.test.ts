import assert from "node:assert/strict";
import test from "node:test";
import { display, inf, valor } from "./index.ts";

test("box and payload keep the existing wire layout", () => {
  const box = valor.box("Numerus", 7n);
  assert.deepEqual(box, { __faberValorTag: "Numerus", __faberValorPayload: 7n });
  assert.equal(valor.payload(box), 7n);
  for (const raw of [null, undefined, 7, "x", [1], {}]) assert.equal(valor.payload(raw), raw);
  assert.equal(display.valor(box), "7");
  assert.equal(inf.fromValor(box), 7n);
});

test("bounded integer tags use i64; inf has no bound", () => {
  for (const value of [0, 5, -5, 0n, 9223372036854775807n, -9223372036854775808n]) {
    for (const carrier of [value, valor.box("Numerus", value)]) {
      assert.equal(valor.isInteger(carrier, true), true);
      assert.equal(valor.isInteger(carrier, false), true);
    }
  }
  for (const value of [9223372036854775808n, -9223372036854775809n, 18446744073709551616n, 2 ** 63]) {
    for (const carrier of [value, valor.box("Numerus", value)]) {
      assert.equal(valor.isInteger(carrier, true), false);
      assert.equal(valor.isInteger(carrier, false), true);
    }
  }
  for (const value of [1.5, NaN, Infinity, "7", true, null, undefined, valor.box("Fractus", 2)]) {
    assert.equal(valor.isInteger(value, true), false);
    assert.equal(valor.isInteger(value, false), false);
  }
});

test("a boxed tag takes precedence over raw carrier shape", () => {
  assert.equal(valor.is(valor.box("Fractus", 2), "Fractus"), true);
  assert.equal(valor.is(2, "Fractus"), false);
  assert.equal(valor.is(1.5, "Fractus"), true);
  assert.equal(valor.is(valor.box("Textus", []), "Lista"), false);
  assert.equal(valor.is(valor.box("Lista", {}), "Lista"), true);
  assert.equal(valor.is(null, "Nihil"), true);
  assert.equal(valor.is(undefined, "Nihil"), true);
  assert.equal(valor.is(true, "Bivalens"), true);
  assert.equal(valor.is("x", "Textus"), true);
  assert.equal(valor.is({ text: () => "t" }, "Instans"), true);
});

test("raw list and map predicates retain shaped-carrier exclusions", () => {
  assert.equal(valor.is([], "Lista"), true);
  assert.equal(valor.is({ planata: () => [] }, "Lista"), true);
  assert.equal(valor.is(new Map(), "Tabula"), true);
  assert.equal(valor.is({}, "Tabula"), true);
  for (const value of [[], null, { planata: () => [] }, { densata: () => [] }, { text: () => "t" }]) {
    assert.equal(valor.is(value, "Tabula"), false);
  }
});
