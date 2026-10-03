import assert from "node:assert/strict";
import test from "node:test";
import { routes } from "./index.ts";
import { table } from "./routes.ts";
import { readFileSync } from "node:fs";

test("package and direct imports share the typed global table", () => {
    assert.equal(routes.table(), table());
    const handler: routes.Handler = { kind: "sync", reply: true, failable: false, run: () => 7 };
    table().set("test:registry", handler);
    assert.equal(routes.table().get("test:registry"), handler);
    assert.equal(routes.table().get("test:registry")!.run(), 7);
    table().delete("test:registry");
});

test("fresh module instances share registrations through globalThis", async () => {
    const fresh = await import(new URL("./routes.ts?other-module", import.meta.url).href);
    assert.equal(fresh.table(), table());
});

test("fixed package and typecheck lists include both carriers", () => {
    const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
    const config = JSON.parse(readFileSync(new URL("./tsconfig.json", import.meta.url), "utf8"));
    for (const name of ["sermo", "routes"]) {
        assert.ok(pkg.files.includes(`${name}.ts`));
        assert.ok(config.include.includes(`${name}.ts`));
        assert.ok(pkg.scripts.test.includes(`${name}.test.ts`));
    }
});
