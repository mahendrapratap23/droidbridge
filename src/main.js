/**
 * DroidBridge — Application entry point
 *
 * Manages all application state and wires UI events to Tauri backend
 * commands. State flows one-way: action → state update → re-render.
 */

import * as api from "./modules/api.js";
import * as ui from "./modules/ui.js";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

/* ── Application state ───────────────────────────────────────── */

const state = {
  devices: [],
  connectedDeviceId: null,
  storages: [],
  activeStorageId: null,
  /** Array of { name, handle } representing the current path from root */
  pathStack: [],
  files: [],
  selectedHandles: new Set(),
  transfers: [],
  isScanning: false,
};

/* ── Render cycle ────────────────────────────────────────────── */

function render() {
  const dev = state.devices.find((d) => d.id === state.connectedDeviceId);
  const connected = !!dev;

  ui.renderDeviceList(state.devices, state.connectedDeviceId, {
    onSelect: handleDeviceSelect,
  });

  ui.renderScanningIndicator(state.isScanning);

  ui.renderStorageList(state.storages, state.activeStorageId, {
    onSelect: handleStorageSelect,
  });

  ui.renderBreadcrumb(state.pathStack, {
    onNavigate: handleBreadcrumbNavigate,
  });

  ui.updateToolbar({
    canBack: state.pathStack.length > 1,
    canUpload: connected && state.activeStorageId != null,
    canDownload: state.selectedHandles.size > 0,
    canDelete: state.selectedHandles.size > 0,
  });

  ui.renderTransfers(state.transfers, {
    onCancel: handleCancelTransfer,
  });

  if (connected) {
    ui.setStatus(`Connected to ${dev.name}`, "connected");
  } else if (state.isScanning) {
    ui.setStatus("Scanning for devices…", "scanning");
  } else {
    ui.setStatus("No device connected", "disconnected");
  }

  const totalItems = state.files.length;
  const selectedCount = state.selectedHandles.size;
  if (totalItems > 0) {
    ui.setStatusInfo(
      selectedCount > 0
        ? `${selectedCount} of ${totalItems} selected`
        : `${totalItems} item${totalItems !== 1 ? "s" : ""}`
    );
  } else {
    ui.setStatusInfo("");
  }
}

/* ── Actions ─────────────────────────────────────────────────── */

async function handleScan() {
  state.isScanning = true;
  render();

  try {
    const devices = await api.scanDevices();
    state.devices = devices;
  } catch (err) {
    console.error("Scan failed:", err);
    ui.showError(String(err));
  } finally {
    state.isScanning = false;
    render();
  }
}

async function handleDeviceSelect(device) {
  if (device.id === state.connectedDeviceId) return;

  // Disconnect current device
  if (state.connectedDeviceId) {
    try {
      await api.disconnectDevice(state.connectedDeviceId);
    } catch (_) {
      /* ignore disconnect errors */
    }
  }

  // Reset state
  state.storages = [];
  state.activeStorageId = null;
  state.pathStack = [];
  state.files = [];
  state.selectedHandles.clear();

  ui.showLoading("Connecting…");

  try {
    await api.connectDevice(device.id);
    state.connectedDeviceId = device.id;
    state.devices = state.devices.map((d) =>
      d.id === device.id ? { ...d, connected: true } : { ...d, connected: false }
    );

    // Fetch storages
    const storages = await api.listStorages(device.id);
    state.storages = storages;

    if (storages.length > 0) {
      await handleStorageSelect(storages[0]);
    } else {
      ui.showEmpty();
    }
  } catch (err) {
    console.error("Connect failed:", err);
    state.connectedDeviceId = null;
    ui.showError(`Failed to connect: ${err}`);
  }

  render();
}

async function handleStorageSelect(storage) {
  state.activeStorageId = storage.id;
  state.pathStack = [{ name: storage.description || "Storage", handle: 0 }];
  state.selectedHandles.clear();

  await loadFiles();
}

async function handleBreadcrumbNavigate(index) {
  state.pathStack = state.pathStack.slice(0, index + 1);
  state.selectedHandles.clear();
  await loadFiles();
}

async function handleOpenFolder(entry) {
  state.pathStack.push({ name: entry.name, handle: entry.handle });
  state.selectedHandles.clear();
  await loadFiles();
}

async function handleBack() {
  if (state.pathStack.length <= 1) return;
  state.pathStack.pop();
  state.selectedHandles.clear();
  await loadFiles();
}

async function loadFiles() {
  if (!state.connectedDeviceId || !state.activeStorageId) return;

  ui.showLoading("Loading files…");
  render();

  try {
    const parentHandle = state.pathStack[state.pathStack.length - 1].handle;
    const files = await api.listFiles(
      state.connectedDeviceId,
      state.activeStorageId,
      parentHandle
    );
    state.files = files;

    if (files.length === 0) {
      ui.showEmpty();
      document.querySelector(".empty-title").textContent = "This folder is empty";
      document.querySelector(".empty-description").textContent = "Upload files using the toolbar above.";
      document.querySelector("#btn-scan-empty").hidden = true;
    } else {
      ui.renderFileTable(state.files, state.selectedHandles, {
        onToggle: handleToggleSelect,
        onOpen: handleOpenFolder,
        onSelectAll: handleSelectAll,
      });
    }
  } catch (err) {
    console.error("Failed to list files:", err);
    ui.showError(String(err));
  }

  render();
}

function handleToggleSelect(handle) {
  if (state.selectedHandles.has(handle)) {
    state.selectedHandles.delete(handle);
  } else {
    state.selectedHandles.add(handle);
  }

  ui.renderFileTable(state.files, state.selectedHandles, {
    onToggle: handleToggleSelect,
    onOpen: handleOpenFolder,
    onSelectAll: handleSelectAll,
  });
  render();
}

function handleSelectAll(checked) {
  state.selectedHandles.clear();
  if (checked) {
    for (const f of state.files) {
      state.selectedHandles.add(f.handle);
    }
  }
  ui.renderFileTable(state.files, state.selectedHandles, {
    onToggle: handleToggleSelect,
    onOpen: handleOpenFolder,
    onSelectAll: handleSelectAll,
  });
  render();
}

async function handleDownload() {
  if (state.selectedHandles.size === 0 || !state.connectedDeviceId) return;

  // Use Tauri dialog to pick save directory
  try {
    const destPath = await openDialog({ directory: true, title: "Save files to…" });
    if (!destPath) return;

    const handles = Array.from(state.selectedHandles);
    await api.downloadFiles(state.connectedDeviceId, handles, destPath);
  } catch (err) {
    console.error("Download failed:", err);
    ui.showError(`Download failed: ${err}`);
  }
}

async function handleUpload() {
  if (!state.connectedDeviceId || !state.activeStorageId) return;

  try {
    const filePaths = await openDialog({ multiple: true, title: "Select files to upload" });
    if (!filePaths || filePaths.length === 0) return;

    const parentHandle = state.pathStack[state.pathStack.length - 1].handle;
    await api.uploadFiles(
      state.connectedDeviceId,
      state.activeStorageId,
      parentHandle,
      Array.isArray(filePaths) ? filePaths : [filePaths]
    );

    // Refresh after upload
    await loadFiles();
  } catch (err) {
    console.error("Upload failed:", err);
    ui.showError(`Upload failed: ${err}`);
  }
}

async function handleDelete() {
  if (state.selectedHandles.size === 0 || !state.connectedDeviceId) return;

  const count = state.selectedHandles.size;
  const confirmed = confirm(`Delete ${count} item${count > 1 ? "s" : ""}? This cannot be undone.`);
  if (!confirmed) return;

  try {
    await api.deleteObjects(state.connectedDeviceId, Array.from(state.selectedHandles));
    state.selectedHandles.clear();
    await loadFiles();
  } catch (err) {
    console.error("Delete failed:", err);
    ui.showError(`Delete failed: ${err}`);
  }
}

async function handleCancelTransfer(transferId) {
  try {
    await api.cancelTransfer(transferId);
  } catch (err) {
    console.error("Cancel failed:", err);
  }
}

/* ── Event listeners ─────────────────────────────────────────── */

function bindEvents() {
  document.getElementById("btn-scan").addEventListener("click", handleScan);
  document.getElementById("btn-scan-empty").addEventListener("click", handleScan);
  document.getElementById("btn-back").addEventListener("click", handleBack);
  document.getElementById("btn-download").addEventListener("click", handleDownload);
  document.getElementById("btn-upload").addEventListener("click", handleUpload);
  document.getElementById("btn-delete").addEventListener("click", handleDelete);
  document.getElementById("btn-close-transfers").addEventListener("click", () => {
    state.transfers = state.transfers.filter((t) => t.status === "active");
    render();
  });
  document.getElementById("btn-retry").addEventListener("click", loadFiles);

  // Keyboard shortcuts
  document.addEventListener("keydown", (e) => {
    if (e.key === "Backspace" && !e.target.matches("input, textarea")) {
      e.preventDefault();
      handleBack();
    }
    if (e.key === "a" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSelectAll(true);
    }
    if (e.key === "Escape") {
      state.selectedHandles.clear();
      ui.renderFileTable(state.files, state.selectedHandles, {
        onToggle: handleToggleSelect,
        onOpen: handleOpenFolder,
        onSelectAll: handleSelectAll,
      });
      render();
    }
  });
}

/* ── Tauri event subscriptions ───────────────────────────────── */

async function subscribeToEvents() {
  await api.onDeviceAttached((device) => {
    const exists = state.devices.some((d) => d.id === device.id);
    if (!exists) {
      state.devices.push(device);
      render();
    }
  });

  await api.onDeviceDetached((payload) => {
    state.devices = state.devices.filter((d) => d.id !== payload.device_id);
    if (state.connectedDeviceId === payload.device_id) {
      state.connectedDeviceId = null;
      state.storages = [];
      state.activeStorageId = null;
      state.pathStack = [];
      state.files = [];
      state.selectedHandles.clear();
      ui.showEmpty();
    }
    render();
  });

  await api.onTransferProgress((data) => {
    const idx = state.transfers.findIndex((t) => t.id === data.transfer_id);
    if (idx >= 0) {
      state.transfers[idx].progress = data.progress;
      state.transfers[idx].name = data.file_name || state.transfers[idx].name;
    } else {
      state.transfers.push({
        id: data.transfer_id,
        name: data.file_name || "Unknown",
        direction: data.direction || "download",
        progress: data.progress,
        status: "active",
      });
    }
    render();
  });

  await api.onTransferComplete((data) => {
    const idx = state.transfers.findIndex((t) => t.id === data.transfer_id);
    if (idx >= 0) {
      state.transfers[idx].status = "complete";
      state.transfers[idx].progress = 1;
    }
    render();
  });

  await api.onTransferError((data) => {
    const idx = state.transfers.findIndex((t) => t.id === data.transfer_id);
    if (idx >= 0) {
      state.transfers[idx].status = "error";
    }
    render();
  });
}

/* ── Init ────────────────────────────────────────────────────── */

document.addEventListener("DOMContentLoaded", async () => {
  bindEvents();
  await subscribeToEvents();
  ui.showEmpty();
  render();

  // Auto-scan on startup
  handleScan();
});
