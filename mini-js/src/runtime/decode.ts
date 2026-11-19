import { inflateRaw } from "../../../lite/src/binary.ts";

declare const __PAYLOAD_B64__: string;

const textDecoder = new TextDecoder();

function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    out[i] = bin.charCodeAt(i);
  }
  return out;
}

export function readMetaJson(
  bytes: Uint8Array,
  view: DataView,
  pos: number
): [Record<string, string | null>, number] {
  const len = view.getUint16(pos, true);
  const start = pos + 2;
  const meta =
    len > 0
      ? (JSON.parse(textDecoder.decode(bytes.subarray(start, start + len))) as Record<string, string | null>)
      : {};
  return [meta, start + len];
}

export function createLazyDb<T>(parse: (bytes: Uint8Array, view: DataView) => T): () => Promise<T> {
  let cached: Promise<T> | null = null;
  return () => {
    if (!cached) {
      cached = inflateRaw(decodeBase64(__PAYLOAD_B64__)).then(({ bytes, view }) => parse(bytes, view));
    }
    return cached;
  };
}
