(() => {
  "use strict";

  const API = "https://bgp-api.mehrnet.com";
  const ICON_SPRITE = "icons.svg?v=20260727-0010";
  const form = document.querySelector("#lookup-form");
  const input = document.querySelector("#ip-input");
  const result = document.querySelector("#result");
  const lookupError = document.querySelector("#lookup-error");
  const currentButton = document.querySelector("#current-button");
  const ipv6Button = document.querySelector("#ipv6-button");
  const ipv6Address = document.querySelector("#ipv6-address");
  const toast = document.querySelector("#toast");

  let activeLookup;
  let toastTimer;

  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (character) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;"
    })[character]);
  }

  function hasValue(value) {
    return value !== null && value !== undefined && String(value).trim() !== "";
  }

  function icon(name, className = "") {
    const classes = className ? ` class="${className}"` : "";
    return `<svg${classes} viewBox="0 0 24 24" aria-hidden="true"><use href="${ICON_SPRITE}#icon-${name}"></use></svg>`;
  }

  function countryFlag(countryCode) {
    const code = String(countryCode || "").trim().toLowerCase();
    if (!/^[a-z]{2}$/.test(code)) return "";
    return `<svg class="country-flag" viewBox="0 0 640 480" aria-hidden="true"><use href="${ICON_SPRITE}#flag-${code}"></use></svg>`;
  }

  function isIPv4(value) {
    const parts = value.split(".");
    return parts.length === 4 && parts.every((part) => (
      /^\d{1,3}$/.test(part)
      && Number(part) >= 0
      && Number(part) <= 255
    ));
  }

  function isIPv6(value) {
    if (!/^[0-9a-fA-F:.]+$/.test(value)) return false;
    const halves = value.split("::");
    if (halves.length > 2) return false;

    const left = halves[0] ? halves[0].split(":") : [];
    const right = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
    const groups = [...left, ...right];
    let count = 0;

    for (let index = 0; index < groups.length; index += 1) {
      const group = groups[index];
      if (group.includes(".")) {
        if (index !== groups.length - 1 || !isIPv4(group)) return false;
        count += 2;
      } else {
        if (!/^[0-9a-fA-F]{1,4}$/.test(group)) return false;
        count += 1;
      }
    }

    return halves.length === 2 ? count < 8 : count === 8;
  }

  function isIP(value) {
    if (!value || /\s/.test(value)) return false;
    return value.includes(":") ? isIPv6(value) : isIPv4(value);
  }

  function normalizedASN(network) {
    if (Array.isArray(network.asns) && network.asns.length) {
      return network.asns.join(", ");
    }
    return network.asn || "";
  }

  function joinedLocation(location, fallbackCountry) {
    const parts = [location.city, location.region, location.country_code || fallbackCountry]
      .filter(hasValue)
      .map(String);
    return [...new Set(parts)].join(", ");
  }

  function addressRange(start, end) {
    if (!hasValue(start) || !hasValue(end)) return "";
    return `${start} – ${end}`;
  }

  function setLookupError(message = "") {
    lookupError.textContent = message;
    lookupError.hidden = !message;
  }

  function showToast(message) {
    window.clearTimeout(toastTimer);
    toast.textContent = message;
    toast.hidden = false;
    toastTimer = window.setTimeout(() => {
      toast.hidden = true;
    }, 2200);
  }

  function valueOrUnavailable(value, leadingIcon = "") {
    if (!hasValue(value)) {
      return '<span class="quick-value muted">Not available</span>';
    }
    const className = leadingIcon ? "quick-value location-value" : "quick-value";
    return `<span class="${className}">${leadingIcon}${escapeHtml(value)}</span>`;
  }

  function quickFact(label, value, iconName, leadingIcon = "") {
    return `
      <div class="quick-fact">
        <span class="quick-heading">${icon(iconName)}<span class="record-label">${escapeHtml(label)}</span></span>
        ${valueOrUnavailable(value, leadingIcon)}
      </div>`;
  }

  function recordRows(fields) {
    return fields
      .filter((field) => hasValue(field.value))
      .map((field) => `
        <div class="record-row">
          <dt>${escapeHtml(field.label)}</dt>
          <dd>${escapeHtml(field.value)}</dd>
        </div>`)
      .join("");
  }

  function recordPanel(title, fields, iconName) {
    const rows = fields.filter((field) => hasValue(field.value));
    if (!rows.length) return "";
    return `
      <section class="record-panel">
        <header class="record-panel-header">
          <span class="record-heading">${icon(iconName)}<h3>${escapeHtml(title)}</h3></span>
        </header>
        <dl class="record-list">${recordRows(rows)}</dl>
      </section>`;
  }

  function sourcePanel(sources) {
    const rows = [
      ["RIR allocation", sources.allocation],
      ["BGP route", sources.route],
      ["Geofeed", sources.geofeed]
    ].map(([name, present]) => `
      <div class="source-row" data-present="${Boolean(present)}">
        <span class="source-name">${name}</span>
        <span class="source-state">${present ? "Matched" : "No match"}</span>
      </div>`).join("");

    return `
      <section class="record-panel">
        <header class="record-panel-header">
          <span class="record-heading">${icon("database")}<h3>Data coverage</h3></span>
        </header>
        <div class="source-list">${rows}</div>
      </section>`;
  }

  function renderLoading() {
    result.setAttribute("aria-busy", "true");
    result.innerHTML = `
      <div class="loading-panel">
        <span class="spinner" aria-hidden="true"></span>
        <span>Loading network record</span>
      </div>`;
  }

  function renderError(message) {
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <div class="error-panel">
        ${icon("alert")}
        <strong>Lookup unavailable</strong>
        <p>${escapeHtml(message)}</p>
      </div>`;
  }

  function renderResult(data, kind) {
    const network = data.network || {};
    const allocation = data.allocation || {};
    const location = data.location || {};
    const sources = data.sources || {};
    const asn = normalizedASN(network);
    const country = location.country_code || allocation.country_code || data.country_code || "";
    const registry = data.registry || allocation.registry || "";
    const networkName = network.name || allocation.name || "Unidentified network";
    const locationName = joinedLocation(location, country);
    const protocol = hasValue(data.version) ? `IPv${data.version}` : "";
    const resultLabel = kind === "me" ? "Current connection" : "Lookup result";
    const allocationStatus = data.allocation_status || allocation.status || "";
    const asSubtitle = [asn, network.cidr].filter(hasValue).join(" · ");
    const flag = countryFlag(country);

    const routeFields = [
      { label: "Prefix", value: network.cidr },
      { label: "Origin ASN", value: asn },
      { label: "AS number", value: network.as_number },
      { label: "Network name", value: network.name },
      { label: "Start address", value: network.start_ip },
      { label: "End address", value: network.end_ip },
      { label: "Route status", value: network.status }
    ];

    const allocationFields = [
      { label: "Registry", value: allocation.registry || data.registry },
      { label: "Name", value: allocation.name },
      { label: "Range", value: addressRange(allocation.start_ip, allocation.end_ip) },
      { label: "Country", value: allocation.country_code || allocation.country_raw },
      { label: "Allocated", value: data.allocation_date || allocation.allocation_date },
      { label: "Status", value: allocationStatus }
    ];

    const locationFields = [
      { label: "Country", value: location.country_code },
      { label: "Region", value: location.region },
      { label: "City", value: location.city }
    ];

    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("globe")}${resultLabel}</span>
          <div class="address-line">
            <h2>${escapeHtml(data.ip)}</h2>
            <button class="copy-button" type="button" data-copy="${escapeHtml(data.ip)}" aria-label="Copy IP address" title="Copy IP address">
              ${icon("copy")}
            </button>
          </div>
          <div class="identity-meta">
            ${hasValue(protocol) ? `<span class="meta-chip accent">${escapeHtml(protocol)}</span>` : ""}
            ${hasValue(registry) ? `<span class="meta-chip">${escapeHtml(String(registry).toUpperCase())}</span>` : ""}
            ${hasValue(country) ? `<span class="meta-chip with-flag">${flag}${escapeHtml(String(country).toUpperCase())}</span>` : ""}
            ${hasValue(allocationStatus) ? `<span class="meta-chip">${escapeHtml(allocationStatus)}</span>` : ""}
          </div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Network</span>
          <strong>${escapeHtml(networkName)}</strong>
          ${hasValue(asSubtitle) ? `<span class="network-subtitle">${escapeHtml(asSubtitle)}</span>` : ""}
        </div>
      </section>

      <section class="quick-facts" aria-label="Key network facts">
        ${quickFact("Origin ASN", asn, "network")}
        ${quickFact("Route prefix", network.cidr, "route")}
        ${quickFact("Registry", hasValue(registry) ? String(registry).toUpperCase() : "", "registry")}
        ${quickFact("Location", locationName, "map-pin", flag)}
      </section>

      <div class="record-grid">
        ${recordPanel("BGP route", routeFields, "route")}
        ${recordPanel("Allocation", allocationFields, "registry")}
        ${recordPanel("Geolocation", locationFields, "map-pin")}
        ${sourcePanel(sources)}
      </div>`;

    result.querySelector("[data-copy]").addEventListener("click", async (event) => {
      try {
        await navigator.clipboard.writeText(event.currentTarget.dataset.copy);
        showToast("IP address copied");
      } catch {
        showToast("Unable to copy IP address");
      }
    });
  }

  function metadataFields(object) {
    return [
      { label: "Registry", value: object.registry },
      { label: "Source", value: object.source },
      { label: "Maintainers", value: object.maintainers },
      { label: "Organization", value: object.organization },
      { label: "Description", value: object.description },
      { label: "Created", value: object.created },
      { label: "Last modified", value: object.last_modified }
    ];
  }

  function objectList(title, objects, iconName, type) {
    if (!Array.isArray(objects) || !objects.length) return "";
    const rows = objects.map((object) => {
      const primary = type === "route" ? object.prefix : addressRange(object.start_ip, object.end_ip);
      const details = type === "route"
        ? [object.origin_asn, object.relation, object.registry].filter(hasValue).join(" · ")
        : [object.name, object.country_code || object.country_raw, object.status].filter(hasValue).join(" · ");
      return `
        <article class="object-row">
          <strong>${escapeHtml(primary || "Unspecified range")}</strong>
          ${hasValue(details) ? `<span>${escapeHtml(details)}</span>` : ""}
        </article>`;
    }).join("");
    return `
      <section class="record-panel object-panel">
        <header class="record-panel-header">
          <span class="record-heading">${icon(iconName)}<h3>${escapeHtml(title)}</h3></span>
          <span class="object-count">${objects.length}</span>
        </header>
        <div class="object-list">${rows}</div>
      </section>`;
  }

  function enrichmentPanels(enrichment) {
    if (!enrichment) return "";
    const panels = [];
    if (enrichment.rdap) {
      panels.push(recordPanel("RDAP registration", [
        { label: "Name", value: enrichment.rdap.name },
        { label: "Handle", value: enrichment.rdap.handle },
        { label: "Type", value: enrichment.rdap.type },
        { label: "Range", value: addressRange(enrichment.rdap.start_ip, enrichment.rdap.end_ip) },
        { label: "Country", value: enrichment.rdap.country_code },
        { label: "Created", value: enrichment.rdap.created },
        { label: "Last changed", value: enrichment.rdap.last_changed },
        { label: "Status", value: Array.isArray(enrichment.rdap.status) ? enrichment.rdap.status.join(", ") : "" }
      ], "registry"));
    }
    if (enrichment.routing_status) {
      panels.push(recordPanel("Routing status", [
        { label: "Origins", value: Array.isArray(enrichment.routing_status.origins) ? enrichment.routing_status.origins.join(", ") : "" },
        { label: "First seen", value: enrichment.routing_status.first_seen },
        { label: "Last seen", value: enrichment.routing_status.last_seen },
        { label: "Holder", value: enrichment.routing_status.holder },
        { label: "Announced", value: enrichment.routing_status.announced === true ? "Yes" : enrichment.routing_status.announced === false ? "No" : "" }
      ], "route"));
    }
    return panels.join("");
  }

  function paginationControl(endpoint, cursor) {
    if (!hasValue(cursor)) return "";
    const url = new URL(endpoint, API);
    url.searchParams.set("cursor", cursor);
    return `<button class="more-button" type="button" data-next-url="${escapeHtml(`${url.pathname}${url.search}`)}">More results ${icon("arrow-right")}</button>`;
  }

  function renderPrefixResult(data, endpoint) {
    const prefix = data.prefix || {};
    const allocation = data.allocation || {};
    const routes = data.routes || { items: [] };
    const country = allocation.country_code || "";
    const flag = countryFlag(country);
    const allocationFields = [
      { label: "Range", value: addressRange(allocation.start_ip, allocation.end_ip) },
      { label: "Registry", value: allocation.registry },
      { label: "Network name", value: allocation.name },
      { label: "Country", value: allocation.country_code || allocation.country_raw },
      { label: "Allocation date", value: allocation.allocation_date },
      { label: "Status", value: allocation.status },
      ...metadataFields(allocation)
    ];

    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("route")}Prefix lookup</span>
          <div class="address-line"><h2>${escapeHtml(prefix.cidr || "CIDR")}</h2></div>
          <div class="identity-meta">
            ${hasValue(prefix.version) ? `<span class="meta-chip accent">IPv${escapeHtml(prefix.version)}</span>` : ""}
            ${hasValue(prefix.prefix_length) ? `<span class="meta-chip">/${escapeHtml(prefix.prefix_length)}</span>` : ""}
            ${hasValue(country) ? `<span class="meta-chip with-flag">${flag}${escapeHtml(country)}</span>` : ""}
          </div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Address space</span>
          <strong>${escapeHtml(prefix.address_count || "")}</strong>
          <span class="network-subtitle">${escapeHtml(addressRange(prefix.start_ip, prefix.end_ip))}</span>
        </div>
      </section>
      <div class="record-grid">
        ${recordPanel("Allocation", allocationFields, "registry")}
        ${objectList("Registered route objects", routes.items, "route", "route")}
        ${enrichmentPanels(data.enrichment)}
      </div>
      ${paginationControl(endpoint, routes.next_cursor)}`;
    bindResultControls();
  }

  function renderRangeResult(data, endpoint) {
    const rangeValue = data.range || {};
    const kind = data.kind || "allocations";
    const objects = kind === "routes" ? data.routes : data.allocations;
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("route")}Range lookup</span>
          <div class="address-line"><h2>${escapeHtml(addressRange(rangeValue.start_ip, rangeValue.end_ip))}</h2></div>
          <div class="identity-meta">${hasValue(rangeValue.version) ? `<span class="meta-chip accent">IPv${escapeHtml(rangeValue.version)}</span>` : ""}</div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Address space</span>
          <strong>${escapeHtml(rangeValue.address_count || "")}</strong>
        </div>
      </section>
      <div class="record-grid">
        ${objectList(kind === "routes" ? "Registered route objects" : "Allocation records", objects, kind === "routes" ? "route" : "registry", kind === "routes" ? "route" : "allocation")}
      </div>
      ${paginationControl(endpoint, data.next_cursor)}`;
    bindResultControls();
  }

  function renderASNResult(data, endpoint) {
    const autnum = data.autnum || {};
    const routes = data.routes || { items: [] };
    const autnumFields = [
      { label: "Name", value: autnum.name },
      { label: "Registry", value: autnum.registry },
      { label: "Country", value: autnum.country_code || autnum.country_raw },
      { label: "Organization", value: autnum.organization },
      { label: "Status", value: autnum.status },
      ...metadataFields(autnum)
    ];
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("network")}Autonomous system</span>
          <div class="address-line"><h2>${escapeHtml(data.asn || "ASN")}</h2></div>
          <div class="identity-meta"><span class="meta-chip accent">AS${escapeHtml(data.as_number || "")}</span></div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("registry")}Registered identity</span>
          <strong>${escapeHtml(autnum.name || autnum.organization || "No aut-num record")}</strong>
          ${hasValue(autnum.country_code) ? `<span class="network-subtitle">${countryFlag(autnum.country_code)}${escapeHtml(autnum.country_code)}</span>` : ""}
        </div>
      </section>
      <div class="record-grid">
        ${recordPanel("Aut-num record", autnumFields, "registry")}
        ${objectList("Registered route objects", routes.items, "route", "route")}
        ${enrichmentPanels(data.enrichment)}
      </div>
      ${paginationControl(endpoint, routes.next_cursor)}`;
    bindResultControls();
  }

  function bindResultControls() {
    result.querySelectorAll("[data-next-url]").forEach((button) => {
      button.addEventListener("click", () => lookupEndpoint(button.dataset.nextUrl, "resource"));
    });
  }

  function renderResourceResult(data, endpoint) {
    if (data.prefix) {
      renderPrefixResult(data, endpoint);
    } else if (data.range) {
      renderRangeResult(data, endpoint);
    } else if (data.asn) {
      renderASNResult(data, endpoint);
    } else {
      renderError("The API returned an unsupported lookup response.");
    }
  }

  function classifyQuery(value) {
    if (isIP(value)) return { kind: "ip", endpoint: `/v1/ip/${encodeURIComponent(value)}` };
    if (/^[0-9a-fA-F:.]+\/[0-9]{1,3}$/.test(value)) return { kind: "prefix", endpoint: `/v1/prefix?prefix=${encodeURIComponent(value)}` };
    if (/^(?:AS)?[1-9][0-9]*$/i.test(value)) return { kind: "asn", endpoint: `/v1/asn/${encodeURIComponent(value)}` };
    const match = value.match(/^\s*([^\s]+)\s+-\s+([^\s]+)\s*$/);
    if (match && isIP(match[1]) && isIP(match[2])) {
      return { kind: "range", endpoint: `/v1/range?start=${encodeURIComponent(match[1])}&end=${encodeURIComponent(match[2])}` };
    }
    return null;
  }

  async function lookup(kind, ip = "") {
	const endpoint = kind === "me" ? "/v1/me" : `/v1/ip/${encodeURIComponent(ip)}`;
	return lookupEndpoint(endpoint, kind);
  }

  async function lookupEndpoint(endpoint, kind) {
    if (activeLookup) activeLookup.abort();
    const controller = new AbortController();
    activeLookup = controller;
    const timeout = window.setTimeout(() => controller.abort(), 10000);

    setLookupError();
    renderLoading();

    try {
      const response = await fetch(API + endpoint, {
        signal: controller.signal,
        headers: { Accept: "application/json" }
      });
      if (!response.ok) {
        throw new Error(response.status === 404
          ? "No network record was found for this address."
          : `The API returned HTTP ${response.status}.`);
      }

      const data = await response.json();
      if (!data) {
        throw new Error("The API returned an empty response.");
      }
      if (kind === "me" || kind === "ip") {
        if (!isIP(data.ip)) throw new Error("The API returned an invalid address record.");
        input.value = data.ip;
        renderResult(data, kind);
      } else {
        renderResourceResult(data, endpoint);
      }
    } catch (error) {
      if (controller.signal.aborted && activeLookup !== controller) return;
      const message = error.name === "AbortError"
        ? "The lookup took too long. Try again."
        : error.message || "The lookup could not be completed.";
      renderError(message);
    } finally {
      window.clearTimeout(timeout);
      if (activeLookup === controller) activeLookup = null;
    }
  }

  async function detectIPv6() {
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 6000);

    try {
      const response = await fetch("https://api6.ipify.org?format=json", {
        signal: controller.signal,
        headers: { Accept: "application/json" }
      });
      if (!response.ok) return;
      const data = await response.json();
      if (!data || !isIPv6(data.ip)) return;
      ipv6Address.textContent = data.ip;
      ipv6Button.dataset.ip = data.ip;
      ipv6Button.hidden = false;
    } catch {
      // An IPv4-only client cannot connect to the IPv6-only endpoint.
    } finally {
      window.clearTimeout(timeout);
    }
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const query = input.value.trim();
    const lookupQuery = classifyQuery(query);
    if (!lookupQuery) {
      setLookupError("Enter an IP address, CIDR, range, or AS number");
      input.focus();
      return;
    }
    lookupEndpoint(lookupQuery.endpoint, lookupQuery.kind);
  });

  currentButton.addEventListener("click", () => lookup("me"));
  ipv6Button.addEventListener("click", () => lookup("ip", ipv6Button.dataset.ip));

  lookup("me");
  detectIPv6();
})();
