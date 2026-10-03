import assert from "node:assert/strict";
import test from "node:test";
import { sermo, routes } from "./index.ts";

const duplicate = <T>(value: T): T => structuredClone(value);
const open = (route: string, opener?: unknown) => sermo.Sermo.open(route, opener, duplicate);
function register(route: string, handler: routes.Handler): void {
    routes.table().set(route, handler);
}

test("echo uses runtime frame and directional carriers, completing and closing once", async () => {
    const conversation = open("runtime:echo", "hello");
    const inbound = conversation.tuus<string>();
    assert.ok(inbound instanceof sermo.Tuus);
    const frame = inbound.accipe()!;
    assert.ok(frame instanceof sermo.Scrinium);
    assert.equal(frame.data, "hello");
    assert.equal(frame.call, "runtime:echo");
    assert.equal(inbound.accipe(), null);
    assert.equal(conversation.state, "finished");
    assert.equal(await conversation.close(), true);
    assert.equal(await conversation.close(), false);
    const outgoing = conversation.meus<number>();
    assert.ok(outgoing instanceof sermo.Meus);
    outgoing.da(1);
    assert.deepEqual(outgoing.fini(), { tag: "done" });
    assert.throws(() => outgoing.da(2), /half-stream is closed/);
});

test("tier one precedes builtins and copies opener and reply", () => {
    let input: { count: number };
    register("runtime:echo", { kind: "sync", reply: true, failable: false, run: (value) => {
        input = value;
        value.count += 1;
        return value;
    } });
    try {
        const original = { count: 1 };
        const conversation = open("runtime:echo", original);
        assert.equal(conversation.tier, "static");
        const result = conversation.materializeValor<{ count: number }>();
        assert.deepEqual(original, { count: 1 });
        assert.deepEqual(result, { count: 2 });
        assert.notEqual(result, input!);
    } finally {
        routes.table().delete("runtime:echo");
    }
});

test("unsupported routes fail closed and device stubs complete", () => {
    assert.throws(() => open("test:missing"), /unsupported TypeScript sermo route/);
    assert.deepEqual(open("gpu:stub").finishInbound(), { tag: "done" });
    assert.deepEqual(open("cuda:stub").finishInbound(), { tag: "done" });
});

test("lazy cursors duplicate items, interleave with producer and cancel once", async () => {
    let steps = 0;
    let releases = 0;
    const item = { n: 1 };
    register("test:cursor", { kind: "cursor", reply: true, failable: false, run: function* () {
        try { steps += 1; yield item; steps += 1; yield item; }
        finally { releases += 1; }
    } });
    const conversation = open("test:cursor");
    assert.equal(steps, 0);
    const frame = conversation.nextInbound<{ n: number }>()!;
    assert.equal(steps, 1);
    assert.notEqual(frame.data, item);
    assert.deepEqual(conversation.cancel(), { tag: "cancel" });
    assert.equal(releases, 1);
    await conversation.close();
    assert.equal(releases, 1);
});

test("cursor completion releases at producer completion, close stays idempotent", async () => {
    let releases = 0;
    register("test:complete", { kind: "cursor", reply: true, failable: false, run: function* () {
        try { yield 1; } finally { releases += 1; }
    } });
    const conversation = open("test:complete");
    assert.deepEqual(conversation.tuus<number>().exhauri(), [1]);
    assert.equal(releases, 1);
    assert.equal(conversation.state, "finished");
    assert.equal(await conversation.close(), true);
    assert.equal(await conversation.close(), false);
    assert.equal(releases, 1);
});

test("sync and cursor failable returns become error terminals", async () => {
    register("test:error", { kind: "sync", reply: true, failable: true, run: () => ({ ok: false, error: "bad" }) });
    const sync = open("test:error");
    assert.throws(() => sync.materializeValor(), /bad/);
    assert.throws(() => sync.close(), /bad/);
    assert.equal(await sync.close(), false);
    register("test:cursor-error", { kind: "cursor", reply: true, failable: true, run: function* () {
        yield 1;
        return { ok: false, error: "cursor bad" };
    } });
    const cursor = open("test:cursor-error");
    assert.equal(cursor.nextInbound<number>()!.data, 1);
    assert.equal(cursor.nextInbound(), null);
    assert.throws(() => cursor.close(), /cursor bad/);
});

test("successful failable replies unwrap and void replies send done only", () => {
    register("test:ok", { kind: "sync", reply: true, failable: true, run: () => ({ ok: true, value: 42 }) });
    assert.equal(open("test:ok").materializeNumber(), 42);
    register("test:void", { kind: "sync", reply: false, failable: false, run: () => 42 });
    assert.equal(open("test:void").nextInbound(), null);
});

test("async handlers reject sync reads and admit async replies", async () => {
    register("test:async", { kind: "async", reply: true, failable: false, run: async () => "async" });
    const conversation = open("test:async");
    assert.throws(() => conversation.nextInbound(), /cannot synchronously read async handler/);
    assert.equal((await conversation.nextInboundAsync<string>())!.data, "async");
    assert.equal(await conversation.nextInboundAsync(), null);
    assert.equal(await conversation.close(), true);
});

test("async cursor close keeps the synchronous signature and releases independently", async () => {
    let releases = 0;
    register("test:async-cursor", { kind: "async-cursor", reply: true, failable: false, run: async function* () {
        try { yield 1; yield 2; } finally { releases += 1; }
    } });
    const conversation = open("test:async-cursor");
    assert.equal((await conversation.nextInboundAsync<number>())!.data, 1);
    assert.equal(conversation.close(), true);
    assert.equal(conversation.state, "finished");
    assert.equal(conversation.close(), false);
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.equal(releases, 1);
});

test("materialization keeps fallbacks and delegates instant parsing without an inline dependency", () => {
    assert.equal(open("runtime:echo", "12").materializeInstans((value) => Number(value)), 12);
    assert.equal(open("runtime:echo", "nope").materializeInstans(() => { throw Error("bad"); }, 7), 7);
    assert.equal(open("runtime:echo", "nope").materializeNumber(8), 8);
});

test("last-reference finalization cancels task with independent lifecycle state", async () => {
    const NativeRegistry = globalThis.FinalizationRegistry;
    let cleanup: (state: any) => void;
    const held: any[] = [];
    const targets: object[] = [];
    class Registry {
        constructor(callback: (state: any) => void) { cleanup = callback; }
        register(target: object, state: any): void { targets.push(target); held.push(state); }
        unregister(): void {}
    }
    globalThis.FinalizationRegistry = Registry as any;
    try {
        const fresh = await import(new URL("./sermo.ts?finalization-test", import.meta.url).href);
        let released = 0;
        register("test:abandon", { kind: "cursor", reply: true, failable: false, run: function* () {
            try { yield 1; } finally { released += 1; }
        } });
        const conversation = fresh.Sermo.open("test:abandon", undefined, duplicate);
        conversation.nextInbound();
        assert.notEqual(targets[0], conversation);
        assert.deepEqual(Object.keys(held[0]).sort(), ["cancelled", "closed", "released", "task"]);
        cleanup!(held[0]);
        cleanup!(held[0]);
        assert.equal(released, 1);
        assert.equal(held[0].task, null);
        assert.equal(held[0].released, true);
        assert.equal(held[0].closed, true);
    } finally {
        globalThis.FinalizationRegistry = NativeRegistry;
    }
});

// Synchronous close is the compiler's semantic signature (radix 49fbee5f8).
test("close throws synchronous cleanup failures but still releases once", () => {
    let releases = 0;
    register("test:cleanup-error", { kind: "cursor", reply: true, failable: false, run: function* () {
        try { yield 1; } finally { releases += 1; throw new Error("cleanup rejected"); }
    } });
    const conversation = open("test:cleanup-error");
    conversation.nextInbound();
    assert.throws(() => conversation.close(), /cleanup rejected/);
    assert.equal(conversation.state, "finished");
    assert.equal(conversation.close(), false);
    assert.equal(releases, 1);
});
