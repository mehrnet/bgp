(() => {
  "use strict";

  const API = "https://bgp-api.mehrnet.com";
  const form = document.querySelector("#lookup-form");
  const input = document.querySelector("#ip-input");
  const heading = document.querySelector("#ip-heading");
  const state = document.querySelector("#lookup-state");
  const result = document.querySelector("#result");
  const availability = document.querySelector("#availability");
  const currentButton = document.querySelector("#current-button");
  const ipv6Connection = document.querySelector("#connection-ipv6");
  let lookupController;

  const copyIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="square" aria-hidden="true"><rect x="8" y="8" width="11" height="11" rx="1"/><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"/></svg>';

  function escapeHtml(value) {
    return String(value).replace(/[&<>\"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
  }

  function meaningful(value) {
    return value !== null && value !== undefined && String(value).trim() !== "";
  }

  function isIPv4(value) {
    const blocks = value.split(".");
    return blocks.length === 4 && blocks.every((block) => /^\d{1,3}$/.test(block) && Number(block) <= 255);
  }

  function isIPv6(value) {
    if (!/^[0-9a-fA-F:.]+$/.test(value)) return false;

    const halves = value.split("::");
    if (halves.length > 2) return false;

    const left = halves[0] ? halves[0].split(":") : [];
    const right = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
    const groups = [...left, ...right];
    let groupCount = 0;

    for (let index = 0; index < groups.length; index += 1) {
      const group = groups[index];
      if (group.includes(".")) {
        if (index !== groups.length - 1 || !isIPv4(group)) return false;
        groupCount += 2;
      } else {
        if (!/^[0-9a-fA-F]{1,4}$/.test(group)) return false;
        groupCount += 1;
      }
    }

    return halves.length === 2 ? groupCount < 8 : groupCount === 8;
  }

  function isIP(value) {
    if (!value || /\s/.test(value)) return false;
    return value.includes(":") ? isIPv6(value) : isIPv4(value);
  }

  function setState(message, tone = "") {
    state.textContent = message;
    state.dataset.tone = tone;
  }

  function field(label, value) {
    if (!meaningful(value)) return "";
    return `<div class="datum"><span class="label">${label}</span><div class="datum-value">${escapeHtml(value)}</div></div>`;
  }

  function band(name, className, fields) {
    const content = fields.filter(Boolean).join("");
    if (!content) return "";
    return `<section class="detail-band ${className}"><div class="band-title">${name}</div><div class="data-grid">${content}</div></section>`;
  }

  function source(name, present) {
    return `<span class="source" data-present="${Boolean(present)}">${name}</span>`;
  }

  function range(start, end) {
    return meaningful(start) && meaningful(end) ? `${start} - ${end}` : "";
  }

  function renderResult(data) {
    const network = data.network || {};
    const allocation = data.allocation || {};
    const location = data.location || {};
    const sources = data.sources || {};
    const asns = Array.isArray(network.asns) && network.asns.length ? network.asns.join(", ") : network.asn;
    const country = location.country_code || allocation.country_code || data.country_code;
    const observed = data.ip;
    const copy = meaningful(observed) ? `<button class="copy-button" type="button" data-copy="${escapeHtml(observed)}" aria-label="Copy IP address" title="Copy IP address">${copyIcon}</button>` : "";
    const protocol = meaningful(data.version) ? `IPv${data.version}` : "";

    result.classList.remove("skeleton");
    result.removeAttribute("aria-busy");
    result.innerHTML = `
      <div class="result-summary">
        <div class="summary-ip"><span class="label">Observed address</span><strong>${escapeHtml(observed)}${copy}</strong></div>
        <div class="summary-meta">
          <div class="summary-metric"><span class="label">Protocol</span><span class="metric-value accent">${escapeHtml(protocol)}</span></div>
          <div class="summary-metric"><span class="label">Registry</span><span class="metric-value">${escapeHtml(data.registry || allocation.registry || "")}</span></div>
          <div class="summary-metric"><span class="label">Country</span><span class="metric-value">${escapeHtml(country || "")}</span></div>
        </div>
      </div>
      ${band("Network route", "route", [
        field("CIDR", network.cidr), field("ASN", asns), field("AS number", network.as_number),
        field("Network", network.name), field("Route coverage", range(network.start_ip, network.end_ip)), field("Route status", network.status)
      ])}
      ${band("Allocation", "allocation", [
        field("Registry", allocation.registry || data.registry), field("Allocation range", range(allocation.start_ip, allocation.end_ip)),
        field("Country", allocation.country_code || allocation.country_raw), field("Allocation name", allocation.name),
        field("Allocation date", data.allocation_date || allocation.allocation_date), field("Allocation status", data.allocation_status || allocation.status)
      ])}
      ${band("Location", "location", [field("Country", location.country_code), field("Region", location.region), field("City", location.city)])}
      <div class="source-row"><div class="source-list">${source("allocation", sources.allocation)}${source("route", sources.route)}${source("geofeed", sources.geofeed)}</div><span class="source-note">record sources</span></div>`;

    const copyButton = result.querySelector("[data-copy]");
    if (copyButton) copyButton.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(copyButton.dataset.copy);
        setState("Address copied to clipboard.", "ok");
      } catch {
        setState("Unable to copy this address.", "error");
      }
    });
  }

  function showError(message) {
    result.classList.remove("skeleton");
    result.removeAttribute("aria-busy");
    result.innerHTML = `<div class="empty"><strong>Lookup unavailable</strong>${escapeHtml(message)}</div>`;
  }

  function setIPv6Connection(ip) {
    if (!isIP(ip) || !ip.includes(":")) return;
    ipv6Connection.hidden = false;
    ipv6Connection.disabled = false;
    ipv6Connection.dataset.state = "ready";
    ipv6Connection.dataset.ip = ip;
    ipv6Connection.querySelector(".connection-ip").textContent = ip;
    ipv6Connection.querySelector(".connection-status").textContent = "view record";
  }

  async function detectIPv6() {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 6000);
    try {
      const response = await fetch("https://api6.ipify.org?format=json", { signal: controller.signal, headers: { Accept: "application/json" } });
      if (!response.ok) throw new Error("IPv6 lookup failed");
      const data = await response.json();
      setIPv6Connection(data && data.ip);
    } catch {
      // IPv4-only browsers commonly cannot reach this endpoint.
    } finally {
      clearTimeout(timeout);
    }
  }

  async function checkHealth() {
    try {
      const response = await fetch(`${API}/v1/health`, { headers: { Accept: "application/json" } });
      if (!response.ok) throw new Error("unhealthy");
      availability.dataset.state = "ok";
      availability.textContent = "online";
    } catch {
      availability.dataset.state = "error";
      availability.textContent = "offline";
    }
  }

  async function lookup(kind, ip = "") {
    if (lookupController) lookupController.abort();
    lookupController = new AbortController();
    const endpoint = kind === "me" ? "/v1/me" : `/v1/ip/${encodeURIComponent(ip)}`;
    heading.textContent = kind === "me" ? "Resolving your address" : ip;
    setState(kind === "me" ? "Querying the network record for this connection." : `Querying network records for ${ip}.`);
    result.classList.add("skeleton");
    result.setAttribute("aria-busy", "true");

    const timeout = setTimeout(() => lookupController.abort(), 10000);
    try {
      const response = await fetch(API + endpoint, { signal: lookupController.signal, headers: { Accept: "application/json" } });
      if (!response.ok) throw new Error(response.status === 404 ? "No record was found for this address." : `The API returned HTTP ${response.status}.`);
      const data = await response.json();
      if (!data || !isIP(data.ip)) throw new Error("The API response did not contain an IP address.");
      input.value = data.ip;
      heading.textContent = data.ip;
      setState("Record resolved successfully.", "ok");
      renderResult(data);
    } catch (error) {
      if (error.name === "AbortError") return;
      const message = error.message || "The lookup could not be completed.";
      setState(message, "error");
      showError(message);
    } finally {
      clearTimeout(timeout);
    }
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const ip = input.value.trim();
    if (!isIP(ip)) {
      setState("Enter a valid IPv4 or IPv6 address.", "error");
      input.focus();
      return;
    }
    lookup("ip", ip);
  });
  currentButton.addEventListener("click", () => lookup("me"));
  ipv6Connection.addEventListener("click", () => lookup("ip", ipv6Connection.dataset.ip));

  lookup("me");
  checkHealth();
  detectIPv6();
})();
