(() => {
  "use strict";

  const API = "https://bgp-api.mehrnet.com";
  const ICON_SPRITE = "/icons.svg?v=20260727-0500";
  const homeView = document.querySelector("#home-view");
  const apiView = document.querySelector("#api-view");
  const apiLink = document.querySelector(".api-link");
  const codeCopyTemplate = document.querySelector("#code-copy-template");
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
  let ipv6Initialized = false;

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
    return networkASNs(network).join(", ");
  }

  function networkASNs(network) {
    const values = Array.isArray(network.asns) && network.asns.length
      ? network.asns
      : [network.asn];
    return [...new Set(values
      .flatMap((value) => String(value || "").split(","))
      .map((value) => value.trim())
      .filter((value) => /^(?:AS)?[1-9][0-9]*$/i.test(value)))];
  }

  function addressRange(start, end) {
    if (!hasValue(start) || !hasValue(end)) return "";
    return `${start} - ${end}`;
  }

  function queryButton(label, endpoint, kind = "resource") {
    if (!hasValue(label) || !hasValue(endpoint)) return "";
    return `<button class="query-link" type="button" data-lookup-endpoint="${escapeHtml(endpoint)}" data-lookup-kind="${escapeHtml(kind)}">${escapeHtml(label)}</button>`;
  }

  function prefixQuery(cidr) {
    if (!hasValue(cidr)) return "";
    return queryButton(cidr, `/v1/prefix?prefix=${encodeURIComponent(cidr)}`);
  }

  function asnQuery(asn) {
    if (!hasValue(asn)) return "";
    const query = String(asn).trim();
    return queryButton(query, `/v1/asn?query=${encodeURIComponent(query)}&page=1`);
  }

  function asnQueries(asns) {
    const values = Array.isArray(asns)
      ? asns
      : String(asns || "").split(",");
    return values
      .map((value) => String(value).trim())
      .filter((value) => /^(?:AS)?[1-9][0-9]*$/i.test(value))
      .map(asnQuery)
      .join('<span class="query-separator">, </span>');
  }

  function ipQuery(ip) {
    if (!hasValue(ip) || !isIP(String(ip))) return "";
    return queryButton(ip, `/v1/ip?query=${encodeURIComponent(ip)}`, "ip");
  }

  function rangeQuery(start, end, kind = "allocations") {
    const label = addressRange(start, end);
    if (!hasValue(label)) return "";
    const endpoint = `/v1/range?start=${encodeURIComponent(start)}&end=${encodeURIComponent(end)}${kind === "routes" ? "&kind=routes" : ""}`;
    return queryButton(label, endpoint);
  }

  function routePath(endpoint, kind = "resource") {
    if (kind === "ip" || endpoint.startsWith("/v1/ip")) {
      const address = new URL(endpoint, API).searchParams.get("query") || "";
      return isIP(address) ? `/ip/${encodeURIComponent(address)}` : "/my-ip";
    }
    if (endpoint.startsWith("/v1/asn")) {
      const asn = new URL(endpoint, API).searchParams.get("query") || "";
      return /^(?:AS)?[1-9][0-9]*$/i.test(asn) ? `/asn/${encodeURIComponent(asn)}` : "/my-ip";
    }
    if (endpoint.startsWith("/v1/prefix")) {
      const url = new URL(endpoint, API);
      const prefix = url.searchParams.get("prefix") || "";
      return prefix ? `/cidr/${encodeURIComponent(prefix)}` : "/my-ip";
    }
    if (endpoint.startsWith("/v1/range")) {
      const url = new URL(endpoint, API);
      const params = new URLSearchParams();
      ["start", "end", "kind"].forEach((key) => {
        const value = url.searchParams.get(key);
        if (value) params.set(key, value);
      });
      return params.has("start") && params.has("end") ? `/range?${params.toString()}` : "/my-ip";
    }
    return "/my-ip";
  }

  function navigateToPath(path) {
    const currentPath = `${window.location.pathname}${window.location.search}`;
    if (currentPath === path) {
      renderRoute();
      return;
    }
    window.history.pushState(null, "", path);
    renderRoute();
  }

  function navigateToEndpoint(endpoint, kind = "resource") {
    navigateToPath(routePath(endpoint, kind));
  }

  function decodePathSegment(value) {
    try {
      return decodeURIComponent(value);
    } catch {
      return "";
    }
  }

  function parseRoutePath() {
    const path = window.location.pathname;
    const segments = path.split("/").filter(Boolean);
    if (path === "/" || path === "/home") return { redirect: "/my-ip" };
    if (path === "/api") return { page: "api" };
    if (segments.length === 2 && segments[0] === "api" && /^(ip|prefix|range|asn)$/.test(segments[1])) {
      return { page: "api", anchor: `api-${segments[1]}` };
    }
    if (path === "/my-ip") return { page: "lookup", kind: "self", endpoint: "", inputValue: "" };

    if (segments.length === 2 && segments[0] === "ip") {
      const address = decodePathSegment(segments[1]);
      return isIP(address)
        ? { page: "lookup", kind: "ip", endpoint: `/v1/ip?query=${encodeURIComponent(address)}`, inputValue: address }
        : { redirect: "/my-ip" };
    }

    if (path.startsWith("/cidr/")) {
      // Browsers may normalize an encoded CIDR slash, so accept both forms.
      const cidr = decodePathSegment(path.slice("/cidr/".length));
      return /^[0-9a-fA-F:.]+\/[0-9]{1,3}$/.test(cidr)
        ? { page: "lookup", kind: "prefix", endpoint: `/v1/prefix?prefix=${encodeURIComponent(cidr)}`, inputValue: cidr }
        : { redirect: "/my-ip" };
    }

    if (segments.length === 2 && segments[0] === "asn") {
      const asn = decodePathSegment(segments[1]);
      return /^(?:AS)?[1-9][0-9]*$/i.test(asn)
        ? { page: "lookup", kind: "asn", endpoint: `/v1/asn?query=${encodeURIComponent(asn)}&page=1`, inputValue: asn }
        : { redirect: "/my-ip" };
    }

    if (path === "/range") {
      const params = new URLSearchParams(window.location.search);
      const start = params.get("start") || "";
      const end = params.get("end") || "";
      const kind = params.get("kind") === "routes" ? "routes" : "";
      if (isIP(start) && isIP(end)) {
        const endpoint = `/v1/range?start=${encodeURIComponent(start)}&end=${encodeURIComponent(end)}${kind ? "&kind=routes" : ""}`;
        return { page: "lookup", kind: "range", endpoint, inputValue: addressRange(start, end) };
      }
      return { redirect: "/my-ip" };
    }

    return { redirect: "/my-ip" };
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

  function renderRoute() {
    const route = parseRoutePath();
    if (route.redirect) {
      window.history.replaceState(null, "", route.redirect);
      renderRoute();
      return;
    }

    const apiRoute = route.page === "api";
    homeView.hidden = apiRoute;
    apiView.hidden = !apiRoute;
    document.body.classList.toggle("api-active", apiRoute);
    document.title = apiRoute ? "MehrNet BGP API" : "MehrNet BGP";

    if (apiRoute) {
      if (activeLookup) {
        activeLookup.abort();
        activeLookup = null;
      }
      apiLink.setAttribute("aria-current", "page");
      if (route.anchor) {
        window.requestAnimationFrame(() => {
          document.getElementById(route.anchor)?.scrollIntoView({ block: "start" });
        });
      }
      return;
    }

    apiLink.removeAttribute("aria-current");
    if (!ipv6Initialized) {
      ipv6Initialized = true;
      detectIPv6();
    }
    if (route.page === "lookup") {
      if (route.kind === "self") {
        lookupCurrentIPv4();
      } else {
        lookupEndpoint(route.endpoint, route.kind, route.inputValue);
      }
    }
  }

  function bindCodeCopyButtons() {
    document.querySelectorAll(".api-code-panel").forEach((panel) => {
      const code = panel.querySelector(".api-code code");
      const codeRegion = panel.querySelector(".api-code");
      const actions = panel.querySelector(".api-code-actions");
      if (!code || !actions) return;
      const button = codeCopyTemplate.content.firstElementChild.cloneNode(true);
      const label = button.querySelector("span");
      const language = panel.dataset.language || "code";
      const service = panel.closest(".api-example")?.querySelector("h3")?.textContent.trim() || "API";
      const sampleType = panel.classList.contains("api-response-panel") ? "JSON response" : "request";
      let resetTimer;

      const panelLabel = document.createElement("span");
      panelLabel.className = "api-code-label";
      panelLabel.textContent = panel.classList.contains("api-response-panel") ? "200 response" : "Request";

      const panelType = document.createElement("span");
      panelType.className = "api-code-type";
      panelType.textContent = panel.classList.contains("api-response-panel") ? "application/json" : language;

      if (codeRegion && !codeRegion.hasAttribute("aria-label")) {
        codeRegion.tabIndex = 0;
        codeRegion.setAttribute("aria-label", `${service} ${language} ${sampleType} example`);
      }

      button.setAttribute("aria-label", `Copy ${language} ${sampleType}`);
      button.title = `Copy ${language} ${sampleType}`;
      button.addEventListener("click", async () => {
        window.clearTimeout(resetTimer);
        try {
          await navigator.clipboard.writeText(code.textContent.trim());
          label.textContent = "Copied";
          button.dataset.copyState = "copied";
        } catch {
          label.textContent = "Copy failed";
          button.dataset.copyState = "failed";
        }
        resetTimer = window.setTimeout(() => {
          label.textContent = "Copy";
          delete button.dataset.copyState;
        }, 1800);
      });
      actions.append(panelLabel, panelType, button);
    });
  }

  function activateLanguage(language) {
    const availableLanguages = new Set(
      Array.from(document.querySelectorAll("[data-language-tab]"), (button) => button.dataset.languageTab),
    );
    const activeLanguage = availableLanguages.has(language) ? language : "curl";
    document.querySelectorAll("[data-language-tab]").forEach((button) => {
      button.setAttribute("aria-pressed", String(button.dataset.languageTab === activeLanguage));
    });
    document.querySelectorAll("[data-language-panel]").forEach((panel) => {
      panel.hidden = panel.dataset.languagePanel !== activeLanguage;
    });
    return activeLanguage;
  }

  function bindApiExampleTabs() {
    const selectedLanguage = localStorage.getItem("bgp_api_language") || "curl";
    const activeLanguage = activateLanguage(selectedLanguage);
    if (activeLanguage !== selectedLanguage) localStorage.setItem("bgp_api_language", activeLanguage);
    document.querySelectorAll("[data-language-tab]").forEach((button) => {
      button.addEventListener("click", () => {
        localStorage.setItem("bgp_api_language", button.dataset.languageTab);
        activateLanguage(button.dataset.languageTab);
      });
    });
  }

  function recordRows(fields) {
    return fields
      .filter((field) => hasValue(field.value))
      .map((field) => `
        <div class="record-row">
          <dt>${escapeHtml(field.label)}</dt>
          <dd>${field.html || escapeHtml(field.value)}</dd>
        </div>`)
      .join("");
  }

  function recordPanel(title, fields, iconName) {
    const seenLabels = new Set();
    const rows = fields.filter((field) => {
      const label = String(field.label).toLowerCase();
      if (!hasValue(field.value) || seenLabels.has(label)) return false;
      seenLabels.add(label);
      return true;
    });
    if (!rows.length) return "";
    return `
      <section class="record-panel">
        <header class="record-panel-header">
          <span class="record-heading">${icon(iconName)}<h3>${escapeHtml(title)}</h3></span>
        </header>
        <dl class="record-list">${recordRows(rows)}</dl>
      </section>`;
  }

  function recordGrid(...panels) {
    const content = panels.filter(hasValue);
    if (!content.length) return "";
    const className = content.length === 1 ? "record-grid record-grid-single" : "record-grid";
    return `<div class="${className}">${content.join("")}</div>`;
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
    const asn = normalizedASN(network);
    const country = location.country_code || allocation.country_code || data.country_code || "";
    const registry = data.registry || allocation.registry || "";
    const networkName = network.name || allocation.name || "Unidentified network";
    const protocol = hasValue(data.version) ? `IPv${data.version}` : "";
    const resultLabel = kind === "self" ? "Current connection" : "Lookup result";
    const allocationStatus = data.allocation_status || allocation.status || "";
    const subtitleParts = [
      hasValue(asn) ? asnQueries(networkASNs(network)) : "",
      hasValue(network.cidr) ? prefixQuery(network.cidr) : ""
    ].filter(hasValue);
    const asSubtitle = subtitleParts.join('<span class="query-separator"> - </span>');
    const flag = countryFlag(country);

    const routeFields = [
      { label: "Route status", value: network.status },
      { label: "Abuse contact", value: network.abuse_contact },
      ...metadataFields(network, ["Registry"])
    ];

    const allocationFields = [
      { label: "Allocation range", value: addressRange(allocation.start_ip, allocation.end_ip), html: rangeQuery(allocation.start_ip, allocation.end_ip) },
      { label: "Registered country", value: allocation.country_code || allocation.country_raw },
      { label: "Allocated", value: data.allocation_date || allocation.allocation_date },
      { label: "Abuse contact", value: allocation.abuse_contact },
      ...metadataFields(allocation, ["Registry"])
    ];

    const locationFields = [
      { label: "Region", value: location.region },
      { label: "City", value: location.city }
    ];

    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("globe")}${resultLabel}</span>
          <div class="address-line">
            <h2>${ipQuery(data.ip) || escapeHtml(data.ip)}</h2>
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
          ${hasValue(asSubtitle) ? `<span class="network-subtitle">${asSubtitle}</span>` : ""}
        </div>
      </section>

      ${recordGrid(
        recordPanel("BGP route", routeFields, "route"),
        recordPanel("Allocation", allocationFields, "registry"),
        recordPanel("Geolocation", locationFields, "map-pin")
      )}`;

    result.querySelector("[data-copy]").addEventListener("click", async (event) => {
      try {
        await navigator.clipboard.writeText(event.currentTarget.dataset.copy);
        showToast("IP address copied");
      } catch {
        showToast("Unable to copy IP address");
      }
    });
    bindResultControls();
  }

  function metadataFields(object, omittedLabels = []) {
    const omitted = new Set(omittedLabels.map((label) => String(label).toLowerCase()));
    return [
      { label: "Registry", value: object.registry },
      { label: "Source", value: object.source },
      { label: "Maintainers", value: object.maintainers },
      { label: "Organization", value: object.organization },
      { label: "Description", value: object.description },
      { label: "Created", value: object.created },
      { label: "Last modified", value: object.last_modified }
    ].filter((field) => !omitted.has(field.label.toLowerCase()));
  }

  function objectList(title, objects, iconName, type) {
    if (!Array.isArray(objects) || !objects.length) return "";
    const rows = objects.map((object) => {
      const primary = type === "route" ? object.prefix : addressRange(object.start_ip, object.end_ip);
      const primaryHtml = type === "route"
        ? prefixQuery(object.prefix)
        : rangeQuery(object.start_ip, object.end_ip);
      const details = type === "route"
        ? [object.origin_asn, object.relation, object.registry].filter(hasValue).join(" · ")
        : [object.name, object.country_code || object.country_raw, object.status].filter(hasValue).join(" · ");
      return `
        <article class="object-row">
          <strong>${primaryHtml || escapeHtml(primary || "Unspecified range")}</strong>
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

  function paginationControl(endpoint, cursor) {
    if (!hasValue(cursor)) return "";
    const url = new URL(endpoint, API);
    url.searchParams.set("cursor", cursor);
    return `<button class="more-button" type="button" data-next-url="${escapeHtml(`${url.pathname}${url.search}`)}">More results ${icon("arrow-right")}</button>`;
  }

  function asnPaginationControl(endpoint, routes) {
    const currentPage = Number.parseInt(routes.page, 10);
    const totalPages = Number.parseInt(routes.total_pages, 10);
    const totalItems = Number.parseInt(routes.total_items, 10);
    if (!Number.isInteger(currentPage) || !Number.isInteger(totalPages) || totalPages < 1) return "";

    const page = Math.min(Math.max(currentPage, 1), totalPages);
    const pageUrl = (targetPage) => {
      const url = new URL(endpoint, API);
      url.searchParams.delete("cursor");
      url.searchParams.set("page", targetPage);
      return escapeHtml(`${url.pathname}${url.search}`);
    };
    const pageButton = (targetPage) => `
      <button class="pagination-page" type="button" data-page-url="${pageUrl(targetPage)}" aria-label="Page ${targetPage}">${targetPage}</button>`;
    const controls = [];

    if (page > 1) {
      controls.push(`
        <button class="pagination-control pagination-control-first" type="button" data-page-url="${pageUrl(1)}" aria-label="First page" title="First page">
          <span class="pagination-double-icon" aria-hidden="true">${icon("arrow-right")}${icon("arrow-right")}</span>
        </button>
        <button class="pagination-control pagination-control-previous" type="button" data-page-url="${pageUrl(page - 1)}" aria-label="Previous page" title="Previous page">
          ${icon("arrow-right")}
        </button>`);
    }

    const firstPage = Math.max(1, page - 2);
    const lastPage = Math.min(totalPages, page + 2);
    for (let targetPage = firstPage; targetPage <= lastPage; targetPage += 1) {
      controls.push(targetPage === page
        ? `<span class="pagination-page is-current" aria-current="page" aria-label="Page ${targetPage}, current page">${targetPage}</span>`
        : pageButton(targetPage));
    }

    if (page < totalPages) {
      controls.push(`
        <button class="pagination-control pagination-control-next" type="button" data-page-url="${pageUrl(page + 1)}" aria-label="Next page" title="Next page">
          ${icon("arrow-right")}
        </button>
        <button class="pagination-control pagination-control-last" type="button" data-page-url="${pageUrl(totalPages)}" aria-label="Last page" title="Last page">
          <span class="pagination-double-icon" aria-hidden="true">${icon("arrow-right")}${icon("arrow-right")}</span>
        </button>`);
    }

    const routeLabel = Number.isInteger(totalItems) && totalItems >= 0
      ? `${totalItems} registered route${totalItems === 1 ? "" : "s"}`
      : "Registered routes";
    const itemSummary = `${routeLabel} | Page ${page} of ${totalPages}`;
    return `
      <nav class="route-pagination" aria-label="Registered route pages">
        <span class="pagination-summary">${escapeHtml(itemSummary)}</span>
        <span class="pagination-controls">${controls.join("")}</span>
      </nav>`;
  }

  function renderPrefixResult(data, endpoint) {
    const prefix = data.prefix || {};
    const allocation = data.allocation || {};
    const routes = data.routes || { items: [] };
    const queryRange = addressRange(prefix.start_ip, prefix.end_ip);
    const allocationRange = addressRange(allocation.start_ip, allocation.end_ip);
    const allocationFields = [
      { label: "Allocation range", value: allocationRange !== queryRange ? allocationRange : "", html: rangeQuery(allocation.start_ip, allocation.end_ip) },
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
          <div class="address-line"><h2>${prefixQuery(prefix.cidr) || escapeHtml(prefix.cidr || "CIDR")}</h2></div>
          <div class="identity-meta">
            ${hasValue(prefix.version) ? `<span class="meta-chip accent">IPv${escapeHtml(prefix.version)}</span>` : ""}
            ${hasValue(prefix.prefix_length) ? `<span class="meta-chip">/${escapeHtml(prefix.prefix_length)}</span>` : ""}
          </div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Address space</span>
          <strong>${escapeHtml(prefix.address_count || "")}</strong>
          <span class="network-subtitle">${rangeQuery(prefix.start_ip, prefix.end_ip) || escapeHtml(addressRange(prefix.start_ip, prefix.end_ip))}</span>
        </div>
      </section>
      ${recordGrid(
        recordPanel("Allocation", allocationFields, "registry"),
        objectList("Registered route objects", routes.items, "route", "route")
      )}
      ${paginationControl(endpoint, routes.next_cursor)}`;
    bindResultControls();
  }

  function renderRangeResult(data, endpoint) {
    const rangeValue = data.range || {};
    const kind = data.kind || "allocations";
    if (data.mode === "summary" && data.summary) {
      renderRangeSummary(data);
      return;
    }
    const objects = kind === "routes" ? data.routes : data.allocations;
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("route")}Range lookup</span>
          <div class="address-line"><h2>${rangeQuery(rangeValue.start_ip, rangeValue.end_ip, kind) || escapeHtml(addressRange(rangeValue.start_ip, rangeValue.end_ip))}</h2></div>
          <div class="identity-meta">${hasValue(rangeValue.version) ? `<span class="meta-chip accent">IPv${escapeHtml(rangeValue.version)}</span>` : ""}</div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Address space</span>
          <strong>${escapeHtml(rangeValue.address_count || "")}</strong>
        </div>
      </section>
      ${recordGrid(objectList(kind === "routes" ? "Registered route objects" : "Allocation records", objects, kind === "routes" ? "route" : "registry", kind === "routes" ? "route" : "allocation"))}
      ${paginationControl(endpoint, data.next_cursor)}`;
    bindResultControls();
  }

  function summaryFacetList(title, facets, iconName, type, sourceRecordCount, sourceLabel) {
    if (!Array.isArray(facets) || !facets.length) return "";
    const rows = facets.map((facet) => {
      const value = String(facet.value || "");
      const country = type === "country" ? countryFlag(value) : "";
      const primary = type === "asn" ? asnQuery(value) : `${country}${escapeHtml(value)}`;
      return `<article class="object-row"><strong>${primary}</strong><span>${escapeHtml(facet.record_count)} source records</span></article>`;
    }).join("");
    return `
      <section class="record-panel object-panel">
        <header class="record-panel-header">
          <span class="record-heading">${icon(iconName)}<h3>${escapeHtml(title)}</h3></span>
          <span class="panel-support">${escapeHtml(sourceRecordCount || 0)} ${escapeHtml(sourceLabel)} records</span>
        </header>
        <div class="object-list">${rows}</div>
      </section>`;
  }

  function renderRangeSummary(data) {
    const rangeValue = data.range || {};
    const summary = data.summary || {};
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("route")}Range summary</span>
          <div class="address-line"><h2>${rangeQuery(rangeValue.start_ip, rangeValue.end_ip) || escapeHtml(addressRange(rangeValue.start_ip, rangeValue.end_ip))}</h2></div>
          <div class="identity-meta">${hasValue(rangeValue.version) ? `<span class="meta-chip accent">IPv${escapeHtml(rangeValue.version)}</span>` : ""}</div>
        </div>
        <div class="network-block">
          <span class="section-label">${icon("network")}Address space</span>
          <strong>${escapeHtml(rangeValue.address_count || "")}</strong>
          <span class="network-subtitle">Precomputed from ${escapeHtml(summary.buckets || 0)} /${escapeHtml(summary.bucket_prefix_length || 0)} buckets</span>
        </div>
      </section>
      ${recordGrid(
        summaryFacetList("Top countries", summary.countries, "map-pin", "country", summary.allocation_records, "allocation"),
        summaryFacetList("Top origin ASNs", summary.asns, "network", "asn", summary.route_records, "route")
      )}`;
    bindResultControls();
  }

  function renderASNResult(data, endpoint) {
    const autnum = data.autnum || {};
    const routes = data.routes || { items: [] };
    const registeredName = autnum.name || autnum.organization || "No aut-num record";
    const registeredOrganization = autnum.organization && autnum.organization !== autnum.name
      ? autnum.organization
      : "";
    const autnumFields = [
      { label: "Registry", value: autnum.registry },
      { label: "Country", value: autnum.country_code || autnum.country_raw },
      { label: "Status", value: autnum.status },
      { label: "Abuse contact", value: autnum.abuse_contact },
      ...metadataFields(autnum, ["Registry", "Organization"])
    ];
    const autnumRows = recordRows(autnumFields);
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <section class="result-identity">
        <div class="address-block">
          <span class="section-label">${icon("network")}Autonomous system</span>
          <div class="address-line"><h2>${asnQuery(data.asn) || escapeHtml(data.asn || "ASN")}</h2></div>
        </div>
        <div class="network-block asn-identity-block">
          <span class="section-label">${icon("registry")}Registered identity</span>
          <strong>${escapeHtml(registeredName)}</strong>
          ${hasValue(registeredOrganization) ? `<span class="network-subtitle">${escapeHtml(registeredOrganization)}</span>` : ""}
          ${autnumRows ? `<dl class="asn-detail-list">${autnumRows}</dl>` : ""}
        </div>
      </section>
      ${recordGrid(objectList("Registered route objects", routes.items, "route", "route"))}
      ${asnPaginationControl(endpoint, routes) || paginationControl(endpoint, routes.next_cursor)}`;
    bindResultControls();
  }

  function bindResultControls() {
    result.querySelectorAll("[data-lookup-endpoint]").forEach((button) => {
      button.addEventListener("click", () => navigateToEndpoint(button.dataset.lookupEndpoint, button.dataset.lookupKind || "resource"));
    });
    result.querySelectorAll("[data-next-url]").forEach((button) => {
      button.addEventListener("click", () => lookupEndpoint(button.dataset.nextUrl, "resource"));
    });
    result.querySelectorAll("[data-page-url]").forEach((button) => {
      button.addEventListener("click", () => lookupEndpoint(button.dataset.pageUrl, "resource"));
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
    if (isIP(value)) return { kind: "ip", endpoint: `/v1/ip?query=${encodeURIComponent(value)}` };
    if (/^[0-9a-fA-F:.]+\/[0-9]{1,3}$/.test(value)) return { kind: "prefix", endpoint: `/v1/prefix?prefix=${encodeURIComponent(value)}` };
    if (/^(?:AS)?[1-9][0-9]*$/i.test(value)) return { kind: "asn", endpoint: `/v1/asn?query=${encodeURIComponent(value)}&page=1` };
    const match = value.match(/^\s*([^\s]+)\s+-\s+([^\s]+)\s*$/);
    if (match && isIP(match[1]) && isIP(match[2])) {
      return { kind: "range", endpoint: `/v1/range?start=${encodeURIComponent(match[1])}&end=${encodeURIComponent(match[2])}` };
    }
    return null;
  }

  async function raceAddressProviders(providers, validate, parentSignal) {
    const attempts = providers.map(({ url, parse }) => {
      const controller = new AbortController();
      const cancel = () => controller.abort();
      parentSignal.addEventListener("abort", cancel, { once: true });

      const promise = fetch(url, {
        signal: controller.signal,
        cache: "no-store"
      })
        .then((response) => {
          if (!response.ok) throw new Error(`Address provider returned HTTP ${response.status}.`);
          return parse(response);
        })
        .then((address) => {
          if (!validate(address)) throw new Error("Address provider returned an invalid address.");
          return address;
        })
        .finally(() => parentSignal.removeEventListener("abort", cancel));

      return { controller, promise };
    });

    try {
      const address = await Promise.any(attempts.map(({ promise }) => promise));
      attempts.forEach(({ controller }) => controller.abort());
      return address;
    } catch {
      attempts.forEach(({ controller }) => controller.abort());
      throw new Error("Unable to determine the current address.");
    }
  }

  async function lookupCurrentIPv4() {
    if (activeLookup) activeLookup.abort();
    const controller = new AbortController();
    activeLookup = controller;
    const timeout = window.setTimeout(() => controller.abort(), 10000);

    setLookupError();
    input.value = "";
    renderLoading();

    try {
      const address = await raceAddressProviders([
        {
          url: "https://ipv4.mehrnet.com/",
          parse: async (response) => (await response.text()).trim()
        },
        {
          url: "https://api4.ipify.org?format=json",
          parse: async (response) => (await response.json()).ip
        }
      ], isIPv4, controller.signal);
      await lookupEndpoint(`/v1/ip?query=${encodeURIComponent(address)}`, "self", address, controller);
    } catch (error) {
      if (controller.signal.aborted && activeLookup !== controller) return;
      renderError("Unable to determine your IPv4 address. Try again.");
    } finally {
      window.clearTimeout(timeout);
      if (activeLookup === controller) activeLookup = null;
    }
  }

  async function lookupEndpoint(endpoint, kind, inputValue = "", sharedController = null) {
    if (activeLookup && activeLookup !== sharedController) activeLookup.abort();
    const controller = sharedController || new AbortController();
    activeLookup = controller;
    const timeout = window.setTimeout(() => controller.abort(), 10000);

    setLookupError();
    if (hasValue(inputValue)) input.value = inputValue;
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
      if (!data) throw new Error("The API returned an empty response.");
      if (kind === "self" || kind === "ip") {
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
      const address = await raceAddressProviders([
        {
          url: "https://ipv6.mehrnet.com/",
          parse: async (response) => (await response.text()).trim()
        },
        {
          url: "https://api6.ipify.org?format=json",
          parse: async (response) => (await response.json()).ip
        }
      ], isIPv6, controller.signal);
      ipv6Address.textContent = address;
      ipv6Button.dataset.ip = address;
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
    navigateToEndpoint(lookupQuery.endpoint, lookupQuery.kind);
  });

  document.addEventListener("click", (event) => {
    const link = event.target.closest("a[data-route]");
    if (!link || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    navigateToPath(link.getAttribute("href"));
  });

  currentButton.addEventListener("click", () => navigateToPath("/my-ip"));
  ipv6Button.addEventListener("click", () => {
    if (ipv6Button.dataset.ip) {
      navigateToEndpoint(`/v1/ip?query=${encodeURIComponent(ipv6Button.dataset.ip)}`, "ip");
    }
  });

  bindCodeCopyButtons();
  bindApiExampleTabs();
  window.addEventListener("popstate", renderRoute);
  renderRoute();
})();
