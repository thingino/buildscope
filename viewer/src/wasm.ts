// Loader and typed wrapper for the buildscope WASM core. The boundary is a
// plain C ABI (see wasm/src/lib.rs): byte buffers and integers only.

export const KIND = {
  ROOT: 1,
  CONFIG: 2,
  PFL: 3,
  BUILD_TIME_LOG: 4,
  ETC_MODULES: 5,
  MODULES_BUILTIN: 6,
  ENV_TEXT: 7,
  GENIMAGE: 8,
  OS_RELEASE: 9,
} as const;

interface Exports {
  memory: WebAssembly.Memory;
  bs_alloc(len: number): number;
  bs_free(ptr: number, len: number): void;
  bs_new(): number;
  bs_drop(handle: number): void;
  bs_set_text(
    h: number,
    kind: number,
    namePtr: number,
    nameLen: number,
    textPtr: number,
    textLen: number
  ): number;
  bs_add_targets(h: number, ptr: number, len: number): number;
  bs_add_removed(h: number, ptr: number, len: number): number;
  bs_add_image(
    h: number,
    namePtr: number,
    nameLen: number,
    size: bigint,
    bytesPtr: number,
    bytesLen: number
  ): number;
  bs_set_images_mtime(h: number, unixSeconds: bigint): number;
  bs_analyze(h: number): number;
  bs_carve(namePtr: number, nameLen: number, bytesPtr: number, bytesLen: number): number;
  bs_schema(): number;
}

const enc = new TextEncoder();
const dec = new TextDecoder();

let loading: Promise<Exports> | null = null;

/** Fetch and instantiate the module (once). Same-origin, no fallbacks. */
export function loadWasm(): Promise<Exports> {
  if (!loading) {
    loading = (async () => {
      const url = new URL("buildscope.wasm", document.baseURI).href;
      const res = await fetch(url);
      if (!res.ok) throw new Error(`cannot load scanner (${res.status})`);
      const { instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {});
      return instance.exports as unknown as Exports;
    })().catch((e) => {
      loading = null;
      throw e;
    });
  }
  return loading;
}

/** A buffer written into wasm memory, released with `free()`. */
class Buf {
  constructor(
    readonly x: Exports,
    readonly ptr: number,
    readonly len: number
  ) {}
  static bytes(x: Exports, data: Uint8Array): Buf {
    if (data.length === 0) return new Buf(x, 0, 0);
    const ptr = x.bs_alloc(data.length);
    new Uint8Array(x.memory.buffer, ptr, data.length).set(data);
    return new Buf(x, ptr, data.length);
  }
  static text(x: Exports, s: string): Buf {
    return Buf.bytes(x, enc.encode(s));
  }
  free() {
    if (this.len > 0) this.x.bs_free(this.ptr, this.len);
  }
}

/** Read a length-prefixed result buffer and release it. */
function takeJson(x: Exports, ptr: number): unknown {
  const len = new DataView(x.memory.buffer).getUint32(ptr, true);
  const text = dec.decode(new Uint8Array(x.memory.buffer, ptr + 4, len));
  x.bs_free(ptr, 4 + len);
  const parsed = JSON.parse(text) as { error?: string };
  if (parsed && typeof parsed.error === "string") throw new Error(parsed.error);
  return parsed;
}

/** Incremental snapshot builder mirroring the native walker. */
export class TreeScan {
  private constructor(
    private readonly x: Exports,
    private readonly h: number
  ) {}

  static async open(): Promise<TreeScan> {
    const x = await loadWasm();
    const h = x.bs_new();
    if (h === 0) throw new Error("scanner unavailable");
    return new TreeScan(x, h);
  }

  setText(kind: number, name: string, text: string) {
    const n = Buf.text(this.x, name);
    const t = Buf.text(this.x, text);
    this.x.bs_set_text(this.h, kind, n.ptr, n.len, t.ptr, t.len);
    n.free();
    t.free();
  }

  /** `size\tflags\tpath` records; flags bit 1 = symlink. */
  addTargets(blob: string): number {
    const b = Buf.text(this.x, blob);
    const n = this.x.bs_add_targets(this.h, b.ptr, b.len);
    b.free();
    return n;
  }

  /** `source_bytes\tpackage\tpath` records. */
  addRemoved(blob: string): number {
    const b = Buf.text(this.x, blob);
    const n = this.x.bs_add_removed(this.h, b.ptr, b.len);
    b.free();
    return n;
  }

  addImage(name: string, size: number, bytes: Uint8Array | null) {
    const n = Buf.text(this.x, name);
    const d = bytes ? Buf.bytes(this.x, bytes) : new Buf(this.x, 0, 0);
    this.x.bs_add_image(this.h, n.ptr, n.len, BigInt(size), d.ptr, d.len);
    n.free();
    d.free();
  }

  setImagesMtime(unixSeconds: number) {
    this.x.bs_set_images_mtime(this.h, BigInt(Math.floor(unixSeconds)));
  }

  analyze(): unknown {
    const ptr = this.x.bs_analyze(this.h);
    try {
      return takeJson(this.x, ptr);
    } finally {
      this.x.bs_drop(this.h);
    }
  }

  abandon() {
    this.x.bs_drop(this.h);
  }
}

/** Analyze one bare firmware artifact. */
export async function carveBytes(name: string, bytes: Uint8Array): Promise<unknown> {
  const x = await loadWasm();
  const n = Buf.text(x, name);
  const d = Buf.bytes(x, bytes);
  try {
    return takeJson(x, x.bs_carve(n.ptr, n.len, d.ptr, d.len));
  } finally {
    n.free();
    d.free();
  }
}
