import { parseIP, parsePrefix } from "./ip.js";

const DEFAULT_ALLOWED_ORIGINS = ["https://bgp.mehrnet.com"];

function parseAllowedOrigins(env) {
  if (env && env.CORS_ALLOWED_ORIGINS_JSON) {
    try {
      const list = JSON.parse(env.CORS_ALLOWED_ORIGINS_JSON);
      if (Array.isArray(list)) return new Set(list.map((s) => String(s).trim()).filter(Boolean));
    } catch {
      // fall back to default
    }
  }
  return new Set(DEFAULT_ALLOWED_ORIGINS);
}

function corsHeaders(request, env) {
  const origin = request.headers.get("Origin") || "";
  const allowed = parseAllowedOrigins(env);
  const headers = {
    "Access-Control-Allow-Methods": "GET, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
    Vary: "Origin",
  };
  if (origin && (allowed.has("*") || allowed.has(origin))) {
    headers["Access-Control-Allow-Origin"] = origin;
  }
  return headers;
}

function jsonResponse(status, body, extraHeaders) {
  return new Response(JSON.stringify(body) + "\n", {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      ...extraHeaders,
    },
  });
}

function errorResponse(status, code, message, extraHeaders) {
  return jsonResponse(status, { error: { code, message } }, extraHeaders);
}

function datasetMeta(db) {
  return {
    release_tag: db.meta.release_tag || null,
    built_at: db.meta.built_at || null,
    activated_at: db.meta.built_at || null,
    source_commit: db.meta.source_commit || null,
  };
}

function parsePage(url) {
  const limitRaw = url.searchParams.get("limit");
  let limit = 50;
  if (limitRaw !== null && limitRaw !== "") {
    if (!/^\d+$/.test(limitRaw)) {
      return { error: ["INVALID_LIMIT", "limit must be between 1 and 100"] };
    }
    limit = Number(limitRaw);
    if (limit < 1 || limit > 100) {
      return { error: ["INVALID_LIMIT", "limit must be between 1 and 100"] };
    }
  }
  const cursorRaw = url.searchParams.get("cursor");
  let cursor = 0;
  if (cursorRaw !== null && cursorRaw !== "") {
    if (!/^\d+$/.test(cursorRaw)) {
      return { error: ["INVALID_CURSOR", "cursor must be a non-negative record cursor"] };
    }
    cursor = Number(cursorRaw);
  }
  return { limit, cursor };
}

function parseASNPage(url) {
  const base = parsePage(url);
  if (base.error) return base;
  const pageRaw = url.searchParams.get("page");
  if (pageRaw === null || pageRaw === "") {
    return { ...base, numbered: false, page: 0 };
  }
  if ((url.searchParams.get("cursor") || "") !== "") {
    return { error: ["INVALID_PAGINATION", "page and cursor cannot be used together"] };
  }
  if (!/^\d+$/.test(pageRaw)) {
    return { error: ["INVALID_PAGE", "page must be between 1 and 100000"] };
  }
  const page = Number(pageRaw);
  if (page < 1 || page > 100000) {
    return { error: ["INVALID_PAGE", "page must be between 1 and 100000"] };
  }
  return { ...base, numbered: true, page };
}

function parseASN(raw) {
  const s = (raw || "").trim().toUpperCase().replace(/^AS/, "");
  if (!/^[1-9]\d*$/.test(s)) return null;
  const n = Number(s);
  if (!Number.isSafeInteger(n) || n < 1 || n > 0xffffffff) return null;
  return n;
}

export function handleRequest(db, request, env = {}) {
  const headers = corsHeaders(request, env);
  if (request.method === "OPTIONS") {
    return new Response(null, {
      status: 204,
      headers: { ...headers, "Access-Control-Max-Age": "86400" },
    });
  }
  if (request.method !== "GET") {
    return errorResponse(404, "NOT_FOUND", "route not found", headers);
  }

  const url = new URL(request.url);
  const path = url.pathname;
  const meta = datasetMeta(db);

  if (path === "/") {
    return jsonResponse(200, { ok: true, service: "bgp-api", mode: "lite", version: 1 }, headers);
  }

  if (path === "/v1/health") {
    return jsonResponse(
      200,
      {
        ok: true,
        service: "bgp-api",
        mode: "lite",
        version: 1,
        database: "lite",
        build: {
          version: meta.release_tag,
          commit: meta.source_commit,
          built_at: meta.built_at,
        },
        dataset: meta,
      },
      headers
    );
  }

  if (path === "/v1/country") {
    const raw = url.searchParams.get("query") || request.headers.get("CF-Connecting-IP") || "";
    const ip = parseIP(raw);
    if (!ip) {
      return errorResponse(400, "INVALID_IP", "query must be a valid IPv4 or IPv6 address", headers);
    }
    const result = db.lookupIP(ip);
    const cc = result && result.location ? result.location.country_code : null;
    if (!cc) {
      return errorResponse(404, "IP_NOT_FOUND", "no country code matched this IP", headers);
    }
    if (url.searchParams.get("format") === "json") {
      return jsonResponse(
        200,
        { meta: { dataset: meta }, ip: ip.canonical, version: ip.isV6 ? 6 : 4, country_code: cc },
        headers
      );
    }
    return new Response(cc + "\n", {
      status: 200,
      headers: { "Content-Type": "text/plain; charset=utf-8", ...headers },
    });
  }

  if (path === "/v1/ip") {
    const ip = parseIP(url.searchParams.get("query") || "");
    if (!ip) {
      return errorResponse(400, "INVALID_IP", "query must be a valid IPv4 or IPv6 address", headers);
    }
    const details = (url.searchParams.get("details") || "").trim();
    if (details !== "" && details !== "full") {
      return errorResponse(400, "INVALID_DETAILS", "details must be full when present", headers);
    }
    const result = db.lookupIP(ip);
    if (!result) {
      return errorResponse(
        404,
        "IP_NOT_FOUND",
        "no RIR allocation, route, or geofeed record matched this IP",
        headers
      );
    }
    if (url.searchParams.get("format") === "text") {
      const cc = result.location.country_code;
      if (!cc) {
        return errorResponse(404, "IP_NOT_FOUND", "no country code matched this IP", headers);
      }
      return new Response(cc + "\n", {
        status: 200,
        headers: { "Content-Type": "text/plain; charset=utf-8", ...headers },
      });
    }
    return jsonResponse(200, { meta: { dataset: meta }, ...result }, headers);
  }

  if (path === "/v1/asn") {
    const asNumber = parseASN(url.searchParams.get("query") || "");
    if (asNumber === null) {
      return errorResponse(400, "INVALID_ASN", "query must be a valid ASN such as AS13335 or 13335", headers);
    }
    const page = parseASNPage(url);
    if (page.error) {
      return errorResponse(400, page.error[0], page.error[1], headers);
    }
    const offset = page.numbered ? (page.page - 1) * page.limit : page.cursor;
    const res = db.lookupASN(asNumber, offset, page.limit);
    if (!res) {
      return errorResponse(404, "ASN_NOT_FOUND", "no aut-num or route record matched this ASN", headers);
    }
    const routes = { items: res.items, next_cursor: null };
    if (page.numbered) {
      routes.page = page.page;
      routes.total_items = res.totalItems;
      routes.total_pages = res.totalItems === 0 ? 0 : Math.ceil(res.totalItems / page.limit);
    } else if (offset + res.items.length < res.totalItems) {
      routes.next_cursor = String(offset + res.items.length);
    }
    return jsonResponse(
      200,
      { meta: { dataset: meta }, asn: res.asn, as_number: res.as_number, autnum: res.autnum, routes },
      headers
    );
  }

  if (path === "/v1/prefix") {
    const prefix = parsePrefix(url.searchParams.get("prefix") || "");
    if (!prefix) {
      return errorResponse(400, "INVALID_PREFIX", "prefix must be a valid IPv4 or IPv6 CIDR", headers);
    }
    const page = parsePage(url);
    if (page.error) {
      return errorResponse(400, page.error[0], page.error[1], headers);
    }
    const res = db.lookupPrefix(prefix, page.cursor, page.limit);
    if (res && res.tooBroad) {
      return errorResponse(
        422,
        "QUERY_TOO_BROAD",
        "prefix matches too many source records; request a narrower prefix",
        headers
      );
    }
    if (!res) {
      return errorResponse(404, "PREFIX_NOT_FOUND", "no allocation or route object matched this prefix", headers);
    }
    return jsonResponse(
      200,
      {
        meta: { dataset: meta },
        prefix: {
          cidr: prefix.cidr,
          version: prefix.version,
          prefix_length: prefix.bits,
          start_ip: prefix.startIP,
          end_ip: prefix.endIP,
          address_count: prefix.addressCount,
        },
        allocation: res.allocation,
        routes: { items: res.routes, next_cursor: res.nextCursor },
      },
      headers
    );
  }

  return errorResponse(404, "NOT_FOUND", "route not found", headers);
}
