import dbBuffer from "../bgp-lite.mnl";
import { LiteDB } from "./mnl1.js";
import { handleRequest } from "./handler.js";

const db = new LiteDB(dbBuffer);

export default {
  fetch(request, env) {
    return handleRequest(db, request, env);
  },
};
