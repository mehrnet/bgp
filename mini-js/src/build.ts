import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";
import * as esbuild from "esbuild";
import { LiteDB } from "../../lite/src/mnl1.ts";
import { buildMna1, buildMnm1 } from "./pack.ts";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");

export interface BuildMiniJsOptions {
  countryPath: string;
  litePath: string;
  outDir: string;
}

export interface BundleStats {
  name: string;
  rawPayloadBytes: number;
  compressedPayloadBytes: number;
  bundleBytes: number;
}

const BANNER = `/**
 * MehrBGP static browser lookup bundle (https://github.com/mehrnet/bgp)
 * Code license: MIT. Compiled routing/RIR data subject to upstream RIR terms & RFC 8805.
 */`;

const MAX_JSDELIVR_BRANCH_BYTES = 45 * 1024 * 1024;

async function emitBundle(
  entryFile: string,
  outFile: string,
  payload: Uint8Array
): Promise<BundleStats> {
  const compressed = zlib.deflateRawSync(payload, { level: 9 });
  const b64 = compressed.toString("base64");
  await esbuild.build({
    entryPoints: [path.join(here, "runtime", entryFile)],
    outfile: outFile,
    bundle: true,
    minify: true,
    format: "esm",
    target: "es2022",
    banner: { js: BANNER },
    define: {
      __PAYLOAD_B64__: JSON.stringify(b64),
    },
  });
  const stat = fs.statSync(outFile);
  return {
    name: path.basename(outFile),
    rawPayloadBytes: payload.length,
    compressedPayloadBytes: compressed.length,
    bundleBytes: stat.size,
  };
}

export async function buildMiniJsBundles(opts: BuildMiniJsOptions): Promise<BundleStats[]> {
  fs.mkdirSync(opts.outDir, { recursive: true });

  const countryBuf = fs.readFileSync(opts.countryPath);
  const countryBytes = new Uint8Array(
    countryBuf.buffer,
    countryBuf.byteOffset,
    countryBuf.byteLength
  );

  const liteBuf = fs.readFileSync(opts.litePath);
  const liteAb = liteBuf.buffer.slice(liteBuf.byteOffset, liteBuf.byteOffset + liteBuf.byteLength);
  const liteBytes = new Uint8Array(liteAb);
  const db = new LiteDB(liteAb);

  const mna1 = buildMna1(liteBytes, db);
  const mnm1 = await buildMnm1(liteBytes, db);

  const stats = await Promise.all([
    emitBundle("country.ts", path.join(opts.outDir, "country.min.js"), countryBytes),
    emitBundle("asn.ts", path.join(opts.outDir, "asn.min.js"), mna1),
    emitBundle("asn-meta.ts", path.join(opts.outDir, "asn-meta.min.js"), mnm1),
  ]);

  let totalBytes = 0;
  for (const s of stats) totalBytes += s.bundleBytes;
  if (totalBytes > MAX_JSDELIVR_BRANCH_BYTES) {
    throw new Error(`Total mini-js bundle size ${totalBytes} exceeds safety limit ${MAX_JSDELIVR_BRANCH_BYTES}`);
  }
  return stats;
}

function parseCliArgs(argv: string[]): BuildMiniJsOptions {
  const baseDir = process.env.INIT_CWD ? path.resolve(process.env.INIT_CWD) : repoRoot;
  let countryPath = path.join(repoRoot, "country.mncc");
  let litePath = path.join(repoRoot, "lite", "bgp-lite.mnl");
  let outDir = path.join(repoRoot, "mini-js", "dist");
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const next = argv[i + 1];
    if (arg === "--country" && next) {
      countryPath = path.resolve(baseDir, next);
      i++;
    } else if (arg === "--lite" && next) {
      litePath = path.resolve(baseDir, next);
      i++;
    } else if (arg === "--out-dir" && next) {
      outDir = path.resolve(baseDir, next);
      i++;
    }
  }
  return { countryPath, litePath, outDir };
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const opts = parseCliArgs(process.argv.slice(2));
  const stats = await buildMiniJsBundles(opts);
  for (const s of stats) {
    console.log(
      `${s.name}: raw=${(s.rawPayloadBytes / 1024).toFixed(1)} KiB deflate=${(s.compressedPayloadBytes / 1024).toFixed(1)} KiB bundle=${(s.bundleBytes / 1024).toFixed(1)} KiB`
    );
  }
}
