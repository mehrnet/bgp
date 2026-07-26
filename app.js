(() => {
  "use strict";

  const API = "https://bgp-api.mehrnet.com";
  const form = document.querySelector("#lookup-form");
  const input = document.querySelector("#ip-input");
  const result = document.querySelector("#result");
  const requestStatus = document.querySelector("#request-status");
  const serviceStatus = document.querySelector("#service-status");
  const serviceStatusText = document.querySelector("#service-status-text");
  const currentButton = document.querySelector("#current-button");
  const ipv6Button = document.querySelector("#ipv6-button");
  const ipv6Address = document.querySelector("#ipv6-address");
  const toast = document.querySelector("#toast");

  let activeLookup;
  let toastTimer;

  const icons = {
    copy: '<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="8" width="11" height="11" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>',
    error: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v6M12 17h.01"/></svg>'
  };

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

  function statusMessage(message, tone = "") {
    requestStatus.textContent = message;
    requestStatus.dataset.tone = tone;
  }

  function showToast(message) {
    window.clearTimeout(toastTimer);
    toast.textContent = message;
    toast.hidden = false;
    toastTimer = window.setTimeout(() => {
      toast.hidden = true;
    }, 2200);
  }

  function valueOrUnavailable(value) {
    if (!hasValue(value)) {
      return '<span class="quick-value muted">Not available</span>';
    }
    return `<span class="quick-value">${escapeHtml(value)}</span>`;
  }

  function quickFact(label, value) {
    return `<div class="quick-fact"><span class="record-label">${escapeHtml(label)}</span>${valueOrUnavailable(value)}</div>`;
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

  function recordPanel(title, fields) {
    const rows = fields.filter((field) => hasValue(field.value));
    if (!rows.length) return "";
    return `
      <section class="record-panel">
        <header class="record-panel-header">
          <h3>${escapeHtml(title)}</h3>
          <span class="record-count">${rows.length} fields</span>
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
          <h3>Data coverage</h3>
          <span class="record-count">3 sources</span>
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
        ${icons.error}
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
          <span class="section-label">${resultLabel}</span>
          <div class="address-line">
            <h2>${escapeHtml(data.ip)}</h2>
            <button class="copy-button" type="button" data-copy="${escapeHtml(data.ip)}" aria-label="Copy IP address" title="Copy IP address">
              ${icons.copy}
            </button>
          </div>
          <div class="identity-meta">
            ${hasValue(protocol) ? `<span class="meta-chip accent">${escapeHtml(protocol)}</span>` : ""}
            ${hasValue(registry) ? `<span class="meta-chip">${escapeHtml(String(registry).toUpperCase())}</span>` : ""}
            ${hasValue(country) ? `<span class="meta-chip">${escapeHtml(String(country).toUpperCase())}</span>` : ""}
            ${hasValue(allocationStatus) ? `<span class="meta-chip">${escapeHtml(allocationStatus)}</span>` : ""}
          </div>
        </div>
        <div class="network-block">
          <span class="section-label">Network</span>
          <strong>${escapeHtml(networkName)}</strong>
          ${hasValue(asSubtitle) ? `<span class="network-subtitle">${escapeHtml(asSubtitle)}</span>` : ""}
        </div>
      </section>

      <section class="quick-facts" aria-label="Key network facts">
        ${quickFact("Origin ASN", asn)}
        ${quickFact("Route prefix", network.cidr)}
        ${quickFact("Registry", hasValue(registry) ? String(registry).toUpperCase() : "")}
        ${quickFact("Location", locationName)}
      </section>

      <div class="record-grid">
        ${recordPanel("BGP route", routeFields)}
        ${recordPanel("Allocation", allocationFields)}
        ${recordPanel("Geolocation", locationFields)}
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

  async function lookup(kind, ip = "") {
    if (activeLookup) activeLookup.abort();
    const controller = new AbortController();
    activeLookup = controller;
    const endpoint = kind === "me" ? "/v1/me" : `/v1/ip/${encodeURIComponent(ip)}`;
    const timeout = window.setTimeout(() => controller.abort(), 10000);

    statusMessage(kind === "me" ? "Resolving your current connection" : `Looking up ${ip}`);
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
      if (!data || !isIP(data.ip)) {
        throw new Error("The API returned an invalid address record.");
      }

      input.value = data.ip;
      statusMessage("Network record loaded", "success");
      renderResult(data, kind);
    } catch (error) {
      if (controller.signal.aborted && activeLookup !== controller) return;
      const message = error.name === "AbortError"
        ? "The lookup took too long. Try again."
        : error.message || "The lookup could not be completed.";
      statusMessage(message, "error");
      renderError(message);
    } finally {
      window.clearTimeout(timeout);
      if (activeLookup === controller) activeLookup = null;
    }
  }

  async function checkHealth() {
    try {
      const response = await fetch(`${API}/v1/health`, {
        headers: { Accept: "application/json" }
      });
      if (!response.ok) throw new Error("API unavailable");
      serviceStatus.dataset.state = "online";
      serviceStatusText.textContent = "API online";
    } catch {
      serviceStatus.dataset.state = "offline";
      serviceStatusText.textContent = "API offline";
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
    const ip = input.value.trim();
    if (!isIP(ip)) {
      statusMessage("Enter a valid IPv4 or IPv6 address", "error");
      input.focus();
      return;
    }
    lookup("ip", ip);
  });

  currentButton.addEventListener("click", () => lookup("me"));
  ipv6Button.addEventListener("click", () => lookup("ip", ipv6Button.dataset.ip));

  lookup("me");
  checkHealth();
  detectIPv6();
})();
