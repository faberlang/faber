/** Conversation, frame and lifecycle carriers for generated TypeScript. */
// @ts-ignore TS5097: Node requires the source extension.
import * as routes from "./routes.ts";

type Duplicate = <T>(value: T) => T;

export type Status =
    { tag: "request" }
    | { tag: "item" }
    | { tag: "byte" }
    | { tag: "bulk" }
    | { tag: "done" }
    | { tag: "error" }
    | { tag: "cancel" };
export class Scrinium<T> {
    id: string | null;
    parent_id: string | null;
    call: string;
    status: Status;
    data: T;
    created_ms: number;
    from: string | null;
    trace: any;
    constructor(init: { id?: string | null; parent_id?: string | null; call?: string; status: Status; data: T; created_ms?: number; from?: string | null; trace?: any }) {
        this.id = init.id ?? null;
        this.parent_id = init.parent_id ?? null;
        this.call = init.call ?? "runtime:echo";
        this.status = init.status;
        this.data = init.data;
        this.created_ms = init.created_ms ?? Date.now();
        this.from = init.from ?? null;
        this.trace = init.trace ?? null;
    }
}
export class Meus<T> {
    private readonly source: Sermo;
    constructor(source: Sermo) { this.source = source; }
    da(value: T): void {
        this.source.pushOutgoing(value);
    }
    fini(): Status {
        this.source.finishOutgoing();
        return { tag: "done" };
    }
}
export class Tuus<T> {
    private readonly source: Sermo;
    constructor(source: Sermo) { this.source = source; }
    accipe(): Scrinium<T> | null {
        return this.source.nextInbound<T>();
    }
    cursor(): Iterable<Scrinium<T>> & AsyncIterable<Scrinium<T>> {
        return this.source.cursor<T>();
    }
    exhauri(): T[] {
        return Array.from(this.cursor(), (frame) => frame.data);
    }
    fini(): Status {
        return this.source.finishInbound();
    }
}

type FaberSermoLifecycle = {
    task: unknown;
    cancelled: boolean;
    released: boolean;
    closed: boolean;
};

type FaberSermoFinalizationRegistry = {
    register(target: object, heldValue: FaberSermoLifecycle, unregisterToken?: object): void;
    unregister(unregisterToken: object): void;
};

type FaberSermoFinalizationRegistryConstructor = new <T>(cleanup: (heldValue: T) => void) => FaberSermoFinalizationRegistry;

const FaberFinalizationRegistry = (globalThis as any).FinalizationRegistry as FaberSermoFinalizationRegistryConstructor | undefined;

function __faberReleaseSermo(lifecycle: FaberSermoLifecycle): void {
    if (lifecycle.released) { return; }
    lifecycle.released = true;
    lifecycle.task = null;
}

function __faberAbandonSermo(lifecycle: FaberSermoLifecycle): void {
    if (lifecycle.closed) { return; }
    lifecycle.closed = true;
    lifecycle.cancelled = true;
    const task = lifecycle.task as { return?: (value?: unknown) => unknown } | null;
    if (task !== null && typeof task.return === "function") { void task.return(undefined); }
    __faberReleaseSermo(lifecycle);
}

type FaberRouteTier = "static" | "builtin" | "none";

export class Sermo {
    private static nextId = 0;
    private static readonly finalizers: FaberSermoFinalizationRegistry | null = FaberFinalizationRegistry === undefined ? null : new FaberFinalizationRegistry((lifecycle: FaberSermoLifecycle) => __faberAbandonSermo(lifecycle));
    readonly id: number;
    readonly route: string;
    readonly tier: FaberRouteTier;
    state: "open" | "finished" = "open";
    private readonly lifecycle: FaberSermoLifecycle;
    private readonly callerToken: object = {};
    private readonly duplicate: Duplicate;
    private readonly inbound: Scrinium<any>[];
    private pull: (() => Scrinium<any>) | null = null;
    private pullAsync: (() => Promise<Scrinium<any>>) | null = null;
    private readonly outgoing: Scrinium<any>[] = [];
    private inboundTerminal: Status | null = null;
    private inboundError: unknown = null;
    private producerTerminal: Status | null = null;
    private producerData: unknown = null;
    private outgoingClosed = false;
    private constructor(route: string, tier: FaberRouteTier, inbound: Scrinium<any>[], duplicate: Duplicate) {
        this.duplicate = duplicate;
        this.id = Sermo.nextId++;
        this.route = route;
        this.tier = tier;
        this.inbound = inbound;
        this.lifecycle = { task: null, cancelled: false, released: false, closed: false };
        Sermo.finalizers?.register(this.callerToken, this.lifecycle, this.callerToken);
    }
    static open(route: string, opener: unknown, duplicate: Duplicate): Sermo {
        const handler = routes.table().get(route);
        if (handler !== undefined) {
            return Sermo.serve(route, handler, opener, duplicate);
        }
        if (route === "runtime:echo") {
            const conversation = new Sermo(route, "builtin", [new Scrinium<any>({ status: { tag: "item" }, data: opener, call: route }), new Scrinium<any>({ status: { tag: "done" }, data: null, call: route })], duplicate);
            conversation.completeProducer(conversation.inbound[conversation.inbound.length - 1]);
            return conversation;
        }
        if (route.startsWith("cuda:") || route.startsWith("gpu:")) {
            const conversation = new Sermo(route, "builtin", [new Scrinium<any>({ status: { tag: "done" }, data: null, call: route })], duplicate);
            conversation.completeProducer(conversation.inbound[0]);
            return conversation;
        }
        throw new Error(`unsupported TypeScript sermo route: ${route}`);
    }
    private static serve(route: string, handler: routes.Handler, opener: unknown, duplicate: Duplicate): Sermo {
        const conversation = new Sermo(route, "static", [], duplicate);
        const input = duplicate(opener);
        switch (handler.kind) {
            case "sync": {
                try {
                    const frames = conversation.replyFrames(handler, handler.run(input));
                    conversation.inbound.push(...frames);
                    conversation.completeProducer(frames[frames.length - 1]);
                } catch (err) {
                    const frame = conversation.frame("error", err);
                    conversation.inbound.push(frame);
                    conversation.completeProducer(frame);
                }
                break;
            }
            case "cursor": {
                const task = handler.run(input) as Iterator<unknown, unknown>;
                conversation.lifecycle.task = task;
                conversation.pull = () => {
                    if (conversation.lifecycle.cancelled) { return conversation.frame("cancel", null); }
                    try {
                        return conversation.cursorFrame(handler, task.next());
                    } catch (err) {
                        const frame = conversation.frame("error", err);
                        conversation.completeProducer(frame);
                        return frame;
                    }
                };
                break;
            }
            case "async": {
                let task: Promise<Scrinium<any>[]>;
                let immediateError: Scrinium<any> | null = null;
                try {
                    task = Promise.resolve(handler.run(input)).then(
                        (result) => {
                            const frames = conversation.replyFrames(handler, result);
                            conversation.completeProducer(frames[frames.length - 1]);
                            return frames;
                        },
                        (err) => {
                            const frame = conversation.frame("error", err);
                            conversation.completeProducer(frame);
                            return [frame];
                        },
                    );
                } catch (err) {
                    immediateError = conversation.frame("error", err);
                    task = Promise.resolve([immediateError]);
                }
                if (immediateError !== null) { conversation.completeProducer(immediateError); }
                let buffered: Scrinium<any>[] | null = null;
                conversation.pullAsync = async () => {
                    if (buffered === null) { buffered = await task; }
                    const frame = buffered.shift() ?? conversation.frame("done", null);
                    if (buffered.length === 0) { conversation.pullAsync = null; }
                    return frame;
                };
                break;
            }
            case "async-cursor": {
                const task = handler.run(input) as AsyncIterator<unknown, unknown>;
                conversation.lifecycle.task = task;
                conversation.pullAsync = async () => {
                    if (conversation.lifecycle.cancelled) { return conversation.frame("cancel", null); }
                    try {
                        return conversation.cursorFrame(handler, await task.next());
                    } catch (err) {
                        const frame = conversation.frame("error", err);
                        conversation.completeProducer(frame);
                        return frame;
                    }
                };
                break;
            }
        }
        return conversation;
    }
    private frame(tag: "item" | "done" | "error" | "cancel", data: unknown): Scrinium<any> {
        return new Scrinium<any>({ status: { tag }, data, call: this.route });
    }
    private completeProducer(frame: Scrinium<any>): void {
        if (this.producerTerminal !== null) { return; }
        this.producerTerminal = frame.status;
        this.producerData = frame.data;
        this.release();
    }
    private replyFrames(handler: routes.Handler, result: unknown): Scrinium<any>[] {
        let value = result;
        if (handler.failable) {
            const outcome = result as { ok: boolean; value?: unknown; error?: unknown };
            if (!outcome.ok) { return [this.frame("error", outcome.error)]; }
            value = outcome.value;
        }
        const done = this.frame("done", null);
        return handler.reply ? [this.frame("item", this.duplicate(value)), done] : [done];
    }
    private cursorFrame(handler: routes.Handler, step: IteratorResult<unknown, unknown>): Scrinium<any> {
        if (!step.done) { return this.frame("item", this.duplicate(step.value)); }
        const outcome = step.value as { ok?: boolean; error?: unknown } | null | undefined;
        const frame = handler.failable && outcome !== null && outcome !== undefined && outcome.ok === false
            ? this.frame("error", outcome.error)
            : this.frame("done", null);
        this.completeProducer(frame);
        return frame;
    }
    private shift(): Scrinium<any> | null {
        const queued = this.inbound.shift();
        if (queued !== undefined) { return queued; }
        if (this.pull !== null) { return this.pull(); }
        if (this.pullAsync !== null) { throw new Error(`TypeScript cannot synchronously read async handler route ${this.route}`); }
        return null;
    }
    private admit<T>(frame: Scrinium<any>): Scrinium<T> | null {
        switch (frame.status.tag) {
            case "item":
            case "byte":
            case "bulk":
                return frame as Scrinium<T>;
            case "done":
            case "error":
            case "cancel":
                this.finish(frame.status, frame.data);
                return null;
            default:
                throw new Error("inbound request frame is invalid");
        }
    }
    private finish(terminal: Status, data: unknown = null): void {
        if (this.inboundTerminal !== null) { return; }
        this.inboundTerminal = terminal;
        if (terminal.tag === "error") { this.inboundError = data; }
        this.release();
        this.pull = null;
        this.pullAsync = null;
    }
    private release(): void {
        this.state = "finished";
        __faberReleaseSermo(this.lifecycle);
    }
    close(): boolean {
        if (this.lifecycle.closed) { return false; }
        this.lifecycle.closed = true;
        Sermo.finalizers?.unregister(this.callerToken);
        try {
            if (this.inboundTerminal === null) {
                this.inbound.length = 0;
                if (this.producerTerminal === null) {
                    this.lifecycle.cancelled = true;
                    const task = this.lifecycle.task as { return?: (value?: unknown) => unknown } | null;
                    if (task !== null && typeof task.return === "function") { void task.return(undefined); }
                    this.finish({ tag: "cancel" });
                } else {
                    this.finish(this.producerTerminal, this.producerData);
                }
            }
            if (this.inboundTerminal?.tag === "error") {
                throw new Error(String(this.inboundError));
            }
            return true;
        } finally {
            this.pull = null;
            this.pullAsync = null;
            this.release();
        }
    }
    cancel(): Status {
        if (this.inboundTerminal !== null) { return this.inboundTerminal; }
        this.lifecycle.cancelled = true;
        const task = this.lifecycle.task as { return?: (value?: unknown) => unknown } | null;
        if (task !== null && typeof task.return === "function") { void task.return(undefined); }
        const terminal: Status = { tag: "cancel" };
        this.finish(terminal);
        return terminal;
    }
    meus<T>(): Meus<T> {
        return new Meus<T>(this);
    }
    tuus<T>(): Tuus<T> {
        return new Tuus<T>(this);
    }
    pushOutgoing<T>(value: T): void {
        if (this.outgoingClosed) { throw new Error("meus half-stream is closed"); }
        this.outgoing.push(new Scrinium<T>({ status: { tag: "item" }, data: value }));
    }
    finishOutgoing(): void {
        if (this.outgoingClosed) { return; }
        this.outgoing.push(new Scrinium<any>({ status: { tag: "done" }, data: null }));
        this.outgoingClosed = true;
    }
    nextInbound<T>(): Scrinium<T> | null {
        while (this.inboundTerminal === null) {
            const frame = this.shift();
            if (frame === null) { return null; }
            const admitted = this.admit<T>(frame);
            if (admitted !== null) { return admitted; }
        }
        return null;
    }
    async nextInboundAsync<T>(): Promise<Scrinium<T> | null> {
        while (this.inboundTerminal === null) {
            const frame = this.inbound.length === 0 && this.pullAsync !== null ? await this.pullAsync() : this.shift();
            if (frame === null) { return null; }
            const admitted = this.admit<T>(frame);
            if (admitted !== null) { return admitted; }
        }
        return null;
    }
    cursor<T>(): Iterable<Scrinium<T>> & AsyncIterable<Scrinium<T>> {
        return {
            [Symbol.iterator]: (): Iterator<Scrinium<T>> => ({
                next: (): IteratorResult<Scrinium<T>> => {
                    const frame = this.nextInbound<T>();
                    return frame === null ? { done: true, value: undefined } : { done: false, value: frame };
                },
            }),
            [Symbol.asyncIterator]: (): AsyncIterator<Scrinium<T>> => ({
                next: async (): Promise<IteratorResult<Scrinium<T>>> => {
                    const frame = await this.nextInboundAsync<T>();
                    return frame === null ? { done: true, value: undefined } : { done: false, value: frame };
                },
            }),
        };
    }
    drain(): void {
        const terminal = this.finishInbound();
        if (terminal.tag === "error") { throw new Error("sermo materialization terminal error"); }
        if (terminal.tag === "cancel") { throw new Error("sermo materialization cancelled"); }
    }
    finishInbound(): Status {
        while (this.inboundTerminal === null) {
            const frame = this.shift();
            if (frame === null) {
                this.finish(this.producerTerminal ?? { tag: "done" }, this.producerData);
                break;
            }
            this.admit(frame);
        }
        return this.inboundTerminal ?? { tag: "done" };
    }
    materializeValor<T = any>(fallback?: T): T {
        return this.materialize((frames) => (frames.length === 0 ? null : frames[0].data) as T, fallback);
    }
    materializeTextus(fallback?: string): string {
        return this.materialize((frames) => { if (frames.some((frame) => frame.status.tag !== "item")) { throw new Error("sermo textus materialization requires item frames"); } return frames.map((frame) => String(frame.data)).join(""); }, fallback);
    }
    materializeNumber(fallback?: number): number {
        return this.materialize((frames) => { if (frames.length !== 1 || frames[0].status.tag !== "item") { throw new Error("sermo numerus materialization requires one item frame"); } const n = Number(frames[0].data); if (Number.isNaN(n)) { throw new Error("sermo numerus materialization failed"); } return n; }, fallback);
    }
    materializeBoolean(fallback?: boolean): boolean {
        return this.materialize((frames) => { if (frames.length !== 1 || frames[0].status.tag !== "item") { throw new Error("sermo bivalens materialization requires one item frame"); } return Boolean(frames[0].data); }, fallback);
    }
    materializeOcteti(fallback?: number[]): number[] {
        return this.materialize((frames) => frames.flatMap((frame) => Array.isArray(frame.data) ? frame.data.map(Number) : Array.from(String(frame.data), (ch) => ch.charCodeAt(0))), fallback);
    }
    materializeInstans<T>(parse: (value: unknown) => T, fallback?: T): T {
        return this.materialize((frames) => { if (frames.length !== 1 || frames[0].status.tag !== "item") { throw new Error("sermo instans materialization requires one item frame"); } return parse(frames[0].data); }, fallback);
    }
    materializeList<T>(convert: (value: any) => T, fallback?: T[]): T[] {
        return this.materialize((frames) => { if (frames.some((frame) => frame.status.tag !== "item")) { throw new Error("sermo lista materialization requires item frames"); } return frames.map((frame) => convert(frame.data)); }, fallback);
    }
    private materialize<T>(convert: (frames: Scrinium<any>[]) => T, fallback?: T): T {
        try {
            return convert(this.readInboundFrames());
        } catch (_err) {
            if (fallback !== undefined) { return fallback; }
            throw _err;
        }
    }
    private readInboundFrames(): Scrinium<any>[] {
        const frames: Scrinium<any>[] = [];
        while (this.inboundTerminal === null) {
            const frame = this.shift();
            if (frame === null) {
                this.finish(this.producerTerminal ?? { tag: "done" }, this.producerData);
                break;
            }
            switch (frame.status.tag) {
                case "item":
                case "byte":
                case "bulk":
                    frames.push(frame);
                    break;
                case "done":
                    this.finish(frame.status, frame.data);
                    break;
                case "error":
                    this.finish(frame.status, frame.data);
                    throw new Error(`sermo materialization terminal error: ${String(frame.data)}`);
                case "cancel":
                    this.finish(frame.status, frame.data);
                    throw new Error("sermo materialization cancelled");
                default:
                    throw new Error("inbound request frame is invalid");
            }
        }
        return frames;
    }
}
