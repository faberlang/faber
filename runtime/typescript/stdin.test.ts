import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { readLineFd } from "./stdin.ts";

function source(text: string) {
  const data = new TextEncoder().encode(text);
  let at = 0;
  return (_fd: number, buffer: Uint8Array, offset: number): number => {
    if (at >= data.length) return 0;
    buffer[offset] = data[at++];
    return 1;
  };
}

test("readLineFd returns lines then null at end of input", () => {
  const read = source("one\r\ntwo\nlast");
  assert.equal(readLineFd(read, 0), "one");
  assert.equal(readLineFd(read, 0), "two");
  assert.equal(readLineFd(read, 0), "last");
  assert.equal(readLineFd(read, 0), null);
});

test("readLineFd returns null on empty input and keeps an empty line", () => {
  assert.equal(readLineFd(source(""), 0), null);
  assert.equal(readLineFd(source("\n"), 0), "");
});

test("readLine is null for a closed stdin under node", () => {
  const code = `import { readLine } from ${JSON.stringify(new URL("./stdin.ts", import.meta.url).href)}; console.log(String(readLine()));`;
  const out = spawnSync(process.execPath, ["--input-type=module", "-e", code], { input: "", encoding: "utf8" });
  assert.equal(out.stdout.trim(), "null");
  const some = spawnSync(process.execPath, ["--input-type=module", "-e", code], { input: "hi\nthere\n", encoding: "utf8" });
  assert.equal(some.stdout.trim(), "hi");
});
