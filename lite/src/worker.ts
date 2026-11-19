import dbBuffer from "../bgp-lite.mnl";
import { handleRequest } from "./handler.ts";
import { LiteDB } from "./mnl1.ts";
import type { Env } from "./types.ts";

const db = new LiteDB(dbBuffer);

const worker: ExportedHandler<Env> = {
  fetch(request, env, ctx) {
    return handleRequest(db, request as unknown as globalThis.Request, env, ctx);
  },
};

export default worker;
