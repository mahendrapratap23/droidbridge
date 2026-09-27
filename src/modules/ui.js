/**
 * DroidBridge — UI rendering module
 *
 * Pure DOM manipulation functions. Each function receives data
 * and updates the relevant section of the DOM. No state is stored
 * here; state lives in main.js.
 */

/* ── Helpers ─────────────────────────────────────────────────── */

function $(selector) {
  return document.querySelector(selector);
}

function formatBytes(bytes) {
  if (bytes === 0 || bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, i);
  return `${val < 10 ? val.toFixed(1) : Math.round(val)} ${units[i]}`;
}

function formatDate(timestamp) {
  if (!timestamp) return "—";
  const d = new Date(timestamp);
  const now = new Date();
  const sameYear = d.getFullYear() === now.getFullYear();
  const opts = { month: "short", day: "numeric" };
  if (!sameYear) opts.year = "numeric";
  return d.toLocaleDateString("en-US", opts);
}

function fileIcon(entry) {
  if (entry.is_folder) return "📁";
  const ext = (entry.name || "").split(".").pop().toLowerCase();
  const map = {
    jpg: "🖼", jpeg: "🖼", png: "🖼", gif: "🖼", webp: "🖼", heic: "🖼",
    mp4: "🎬", mov: "🎬", mkv: "🎬", avi: "🎬", webm: "🎬",
    mp3: "🎵", aac: "🎵", flac: "🎵", wav: "🎵", ogg: "🎵",
    pdf: "📄", doc: "📝", docx: "📝", txt: "📝",
    zip: "📦", gz: "📦", tar: "📦", rar: "📦",
    apk: "📱",
  };
  return map[ext] || "📄";
}

/* ── Device list ─────────────────────────────────────────────── */

export function renderDeviceList(devices, activeId, { onSelect }) {
  const container = $("#device-list");
  container.innerHTML = "";

  if (devices.length === 0) {
    container.innerHTML = `
      <div class="device-item" style="cursor: default; opacity: 0.5;">
        <span class="device-icon">📱</span>
        <span class="device-name">No devices found</span>
      </div>`;
    return;
  }

  for (const dev of devices) {
    const el = document.createElement("div");
    el.className = "device-item" + (dev.id === activeId ? " active" : "");
    el.innerHTML = `
      <span class="device-icon">📱</span>
      <span class="device-name">${escapeHtml(dev.name)}</span>
      <span class="device-status">${dev.connected ? "●" : ""}</span>`;
    el.addEventListener("click", () => onSelect(dev));
    container.appendChild(el);
  }
}

export function renderScanningIndicator(isScanning) {
  const existing = $("#device-list .scanning-indicator");
  if (existing) existing.remove();

  if (isScanning) {
    const el = document.createElement("div");
    el.className = "scanning-indicator";
    el.innerHTML = `<div class="spinner-sm"></div><span>Scanning…</span>`;
    $("#device-list").prepend(el);
  }
}

/* ── Storage list ────────────────────────────────────────────── */

export function renderStorageList(storages, activeId, { onSelect }) {
  const section = $("#storage-section");
  const container = $("#storage-list");

  if (!storages || storages.length === 0) {
    section.hidden = true;
    return;
  }

  section.hidden = false;
  container.innerHTML = "";

  for (const st of storages) {
    const usedPct = st.max_capacity > 0
      ? Math.round((1 - st.free_space / st.max_capacity) * 100)
      : 0;
    const el = document.createElement("div");
    el.className = "storage-item" + (st.id === activeId ? " active" : "");
    el.innerHTML = `
      <span>${escapeHtml(st.description || "Storage")}</span>
      <div class="storage-bar">
        <div class="storage-bar-fill" style="width: ${usedPct}%"></div>
      </div>`;
    el.addEventListener("click", () => onSelect(st));
    container.appendChild(el);
  }
}

/* ── Breadcrumb ──────────────────────────────────────────────── */

export function renderBreadcrumb(pathSegments, { onNavigate }) {
  const nav = $("#breadcrumb");
  nav.innerHTML = "";

  if (pathSegments.length === 0) {
    nav.innerHTML = `<span class="crumb crumb-root">No device</span>`;
    return;
  }

  pathSegments.forEach((seg, i) => {
    if (i > 0) {
      const sep = document.createElement("span");
      sep.className = "crumb-sep";
      sep.textContent = "›";
      nav.appendChild(sep);
    }

    const crumb = document.createElement("span");
    crumb.className = "crumb";
    crumb.textContent = seg.name;
    if (i < pathSegments.length - 1) {
      crumb.addEventListener("click", () => onNavigate(i));
    }
    nav.appendChild(crumb);
  });
}

/* ── File table ──────────────────────────────────────────────── */

export function renderFileTable(files, selectedHandles, { onToggle, onOpen, onSelectAll }) {
  const tableContainer = $("#file-table-container");
  const tbody = $("#file-list");
  const selectAllCb = $("#select-all");

  tableContainer.hidden = false;
  $("#empty-state").hidden = true;
  $("#loading-state").hidden = true;
  $("#error-state").hidden = true;

  tbody.innerHTML = "";
  selectAllCb.checked = files.length > 0 && selectedHandles.size === files.length;
  selectAllCb.indeterminate = selectedHandles.size > 0 && selectedHandles.size < files.length;
  selectAllCb.onchange = () => onSelectAll(selectAllCb.checked);

  for (const f of files) {
    const tr = document.createElement("tr");
    const isSelected = selectedHandles.has(f.handle);
    tr.className = (f.is_folder ? "file-row-folder" : "") + (isSelected ? " selected" : "");
    tr.innerHTML = `
      <td class="col-check"><input type="checkbox" ${isSelected ? "checked" : ""} /></td>
      <td class="col-name">
        <div class="file-name-cell">
          <span class="file-icon">${fileIcon(f)}</span>
          <span class="file-name">${escapeHtml(f.name)}</span>
        </div>
      </td>
      <td class="col-size file-size">${f.is_folder ? "—" : formatBytes(f.size)}</td>
      <td class="col-modified file-modified">${formatDate(f.modified)}</td>`;

    const checkbox = tr.querySelector("input[type=checkbox]");
    checkbox.addEventListener("change", (e) => {
      e.stopPropagation();
      onToggle(f.handle);
    });

    if (f.is_folder) {
      tr.addEventListener("dblclick", () => onOpen(f));
    }

    tr.addEventListener("click", (e) => {
      if (e.target.tagName === "INPUT") return;
      onToggle(f.handle);
    });

    tbody.appendChild(tr);
  }
}

/* ── UI states ───────────────────────────────────────────────── */

export function showEmpty() {
  $("#empty-state").hidden = false;
  $("#file-table-container").hidden = true;
  $("#loading-state").hidden = true;
  $("#error-state").hidden = true;
}

export function showLoading(text = "Loading…") {
  $("#loading-state").hidden = false;
  $(".loading-text").textContent = text;
  $("#empty-state").hidden = true;
  $("#file-table-container").hidden = true;
  $("#error-state").hidden = true;
}

export function showError(message) {
  $("#error-state").hidden = false;
  $("#error-message").textContent = message;
  $("#empty-state").hidden = true;
  $("#file-table-container").hidden = true;
  $("#loading-state").hidden = true;
}

/* ── Toolbar state ───────────────────────────────────────────── */

export function updateToolbar({ canBack, canUpload, canDownload, canDelete }) {
  $("#btn-back").disabled = !canBack;
  $("#btn-upload").disabled = !canUpload;
  $("#btn-download").disabled = !canDownload;
  $("#btn-delete").disabled = !canDelete;
}

/* ── Transfer panel ──────────────────────────────────────────── */

export function renderTransfers(transfers, { onCancel }) {
  const panel = $("#transfer-panel");
  const list = $("#transfer-list");

  if (transfers.length === 0) {
    panel.hidden = true;
    return;
  }

  panel.hidden = false;
  list.innerHTML = "";

  for (const t of transfers) {
    const el = document.createElement("div");
    el.className = "transfer-item";

    const dirIcon = t.direction === "download" ? "↓" : "↑";
    const fillClass = t.direction === "download" ? "downloading" : "uploading";
    const pct = Math.round(t.progress * 100);

    let statusHtml;
    if (t.status === "complete") {
      statusHtml = `<span class="transfer-percent transfer-complete">Done</span>`;
    } else if (t.status === "error") {
      statusHtml = `<span class="transfer-percent transfer-error">Error</span>`;
    } else {
      statusHtml = `
        <div class="transfer-progress-bar">
          <div class="transfer-progress-fill ${fillClass}" style="width: ${pct}%"></div>
        </div>
        <span class="transfer-percent">${pct}%</span>
        <button class="icon-btn transfer-cancel" title="Cancel transfer">✕</button>`;
    }

    el.innerHTML = `
      <span class="transfer-icon">${dirIcon}</span>
      <span class="transfer-name">${escapeHtml(t.name)}</span>
      ${statusHtml}`;

    const cancelBtn = el.querySelector(".transfer-cancel");
    if (cancelBtn) {
      cancelBtn.addEventListener("click", () => onCancel(t.id));
    }

    list.appendChild(el);
  }
}

/* ── Status bar ──────────────────────────────────────────────── */

export function setStatus(text, dotClass = "disconnected") {
  $("#status-text").innerHTML = `<span class="status-dot ${dotClass}"></span>${escapeHtml(text)}`;
}

export function setStatusInfo(text) {
  $("#status-info").textContent = text;
}

/* ── Escape ──────────────────────────────────────────────────── */

function escapeHtml(str) {
  if (!str) return "";
  return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
