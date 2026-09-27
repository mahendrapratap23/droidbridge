/**
 * DroidBridge — Tauri IPC API wrapper
 *
 * All Tauri command invocations are centralised here so the UI code
 * never calls `invoke()` directly.
 */

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/* ── Device discovery ────────────────────────────────────────── */

export async function scanDevices() {
  return invoke("scan_devices");
}

export async function connectDevice(deviceId) {
  return invoke("connect_device", { deviceId });
}

export async function disconnectDevice(deviceId) {
  return invoke("disconnect_device", { deviceId });
}

/* ── Browsing ────────────────────────────────────────────────── */

export async function listStorages(deviceId) {
  return invoke("list_storages", { deviceId });
}

export async function listFiles(deviceId, storageId, parentHandle) {
  return invoke("list_files", { deviceId, storageId, parentHandle });
}

/* ── Transfers ───────────────────────────────────────────────── */

export async function downloadFiles(deviceId, objectHandles, destPath) {
  return invoke("download_files", { deviceId, objectHandles, destPath });
}

export async function uploadFiles(deviceId, storageId, parentHandle, filePaths) {
  return invoke("upload_files", { deviceId, storageId, parentHandle, filePaths });
}

export async function cancelTransfer(transferId) {
  return invoke("cancel_transfer", { transferId });
}

export async function deleteObjects(deviceId, objectHandles) {
  return invoke("delete_objects", { deviceId, objectHandles });
}

/* ── Event listeners ─────────────────────────────────────────── */

export function onDeviceAttached(callback) {
  return listen("device-attached", (e) => callback(e.payload));
}

export function onDeviceDetached(callback) {
  return listen("device-detached", (e) => callback(e.payload));
}

export function onTransferProgress(callback) {
  return listen("transfer-progress", (e) => callback(e.payload));
}

export function onTransferComplete(callback) {
  return listen("transfer-complete", (e) => callback(e.payload));
}

export function onTransferError(callback) {
  return listen("transfer-error", (e) => callback(e.payload));
}
