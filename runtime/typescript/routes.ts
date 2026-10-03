/** Shared typed tier-one route registry. All emitted modules use this table. */
type Kind = "sync" | "async" | "cursor" | "async-cursor";
export type Handler = {
    kind: Kind;
    reply: boolean;
    failable: boolean;
    run: (opener?: any) => any;
};
export type Registry = Map<string, Handler>;

export function table(): Registry {
    const host = globalThis as unknown as { __faberRoutes?: Registry };
    return (host.__faberRoutes ??= new Map<string, Handler>());
}
