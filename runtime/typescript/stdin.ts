/**
 * Line input for the `lege` / `read` builtin.
 *
 * Returns one line without its terminator, or `null` at end of input (Faber
 * `none`). In a browser the line comes from `prompt` (cancel is `null`); under
 * Node it comes from file descriptor 0, read a byte at a time so input past
 * the line stays unread for the next call.
 */
export function readLine(): string | null {
  const host: any = globalThis;
  if (typeof host.prompt === "function") {
    return host.prompt("") as string | null;
  }
  const fs = host.process?.getBuiltinModule?.("node:fs");
  if (!fs) {
    return null;
  }
  return readLineFd(fs.readSync, 0);
}

/** Read one line from `fd` through a `readSync`-shaped function. */
export function readLineFd(
  readSync: (fd: number, buffer: Uint8Array, offset: number, length: number, position: null) => number,
  fd: number,
): string | null {
  const bytes: number[] = [];
  const one = new Uint8Array(1);
  for (;;) {
    let n = 0;
    try {
      n = readSync(fd, one, 0, 1, null);
    } catch (error: any) {
      if (error?.code === "EAGAIN") {
        continue;
      }
      if (error?.code !== "EOF") {
        throw error;
      }
    }
    if (n === 0) {
      return bytes.length === 0 ? null : decode(bytes);
    }
    if (one[0] === 10) {
      return decode(bytes);
    }
    bytes.push(one[0]);
  }
}

function decode(bytes: number[]): string {
  const text = new TextDecoder().decode(Uint8Array.from(bytes));
  return text.endsWith("\r") ? text.slice(0, -1) : text;
}
