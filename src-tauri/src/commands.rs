//! Tauri command handlers.
//!
//! Each `#[tauri::command]` function is the bridge between the
//! JavaScript frontend and the Rust MTP backend. Commands receive
//! typed parameters from `invoke()`, access the shared `AppState`,
//! and return JSON-serialisable results (or error strings).

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::mtp::device;
use crate::mtp::session::MtpSession;
use crate::mtp::transfer;
use crate::mtp::types::*;
use crate::state::AppState;

// ── Device discovery ────────────────────────────────────────────

#[tauri::command]
pub fn scan_devices() -> Result<Vec<DeviceInfo>, String> {
    device::scan_mtp_devices().map_err(|e| e.to_string())
}

// ── Connection management ───────────────────────────────────────

#[tauri::command]
pub fn connect_device(device_id: String, state: State<'_, AppState>) -> Result<(), String> {
    // Open the USB device and claim MTP interface in one shot
    let opened = device::open_mtp_device(&device_id)
        .map_err(|e| format!("Failed to open USB device: {}", e))?;

    // Open MTP session using the pre-claimed interface/endpoints
    let session = MtpSession::open(opened)
        .map_err(|e| format!("Failed to open MTP session: {}", e))?;

    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    sessions.insert(device_id, session);

    Ok(())
}

#[tauri::command]
pub fn disconnect_device(device_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(mut session) = sessions.remove(&device_id) {
        session.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Browsing ────────────────────────────────────────────────────

#[tauri::command]
pub fn list_storages(device_id: String, state: State<'_, AppState>) -> Result<Vec<StorageInfo>, String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get_mut(&device_id)
        .ok_or_else(|| format!("No active session for device {}", device_id))?;

    let storage_ids = session.get_storage_ids().map_err(|e| e.to_string())?;

    let mut storages = Vec::new();
    for sid in storage_ids {
        match session.get_storage_info(sid) {
            Ok(info) => storages.push(info),
            Err(e) => log::warn!("Failed to get storage info for 0x{:08X}: {}", sid, e),
        }
    }

    Ok(storages)
}

#[tauri::command]
pub fn list_files(
    device_id: String,
    storage_id: u32,
    parent_handle: u32,
    state: State<'_, AppState>,
) -> Result<Vec<FileEntry>, String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get_mut(&device_id)
        .ok_or_else(|| format!("No active session for device {}", device_id))?;

    let handles = session
        .get_object_handles(storage_id, parent_handle)
        .map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    for h in handles {
        match session.get_object_info(h) {
            Ok(entry) => entries.push(entry),
            Err(e) => log::warn!("Failed to get object info for handle {}: {}", h, e),
        }
    }

    // Sort: folders first, then by name
    entries.sort_by(|a, b| {
        b.is_folder
            .cmp(&a.is_folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

// ── Downloads ───────────────────────────────────────────────────

#[tauri::command]
pub fn download_files(
    device_id: String,
    object_handles: Vec<u32>,
    dest_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let dest = PathBuf::from(&dest_path);
    if !dest.is_dir() {
        return Err("Destination is not a directory".into());
    }

    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get_mut(&device_id)
        .ok_or_else(|| format!("No active session for device {}", device_id))?;

    for handle in &object_handles {
        let transfer_id = uuid::Uuid::new_v4().to_string();

        // Get file info first
        let info = session
            .get_object_info(*handle)
            .map_err(|e| e.to_string())?;

        let tid_clone = transfer_id.clone();
        let cancel_flag = || {
            state
                .cancel_flags
                .lock()
                .map(|flags| flags.get(&tid_clone).copied().unwrap_or(false))
                .unwrap_or(false)
        };

        let app_clone = app.clone();
        let progress_cb = move |p: TransferProgress| {
            let _ = app_clone.emit("transfer-progress", &p);
        };

        transfer::download_file(
            session,
            *handle,
            &info.name,
            &dest,
            &transfer_id,
            &cancel_flag,
            &progress_cb,
        )
        .map_err(|e| e.to_string())?;

        let _ = app.emit(
            "transfer-complete",
            serde_json::json!({ "transfer_id": transfer_id }),
        );
    }

    Ok(())
}

// ── Uploads ─────────────────────────────────────────────────────

#[tauri::command]
pub fn upload_files(
    device_id: String,
    storage_id: u32,
    parent_handle: u32,
    file_paths: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get_mut(&device_id)
        .ok_or_else(|| format!("No active session for device {}", device_id))?;

    for path_str in &file_paths {
        let path = PathBuf::from(path_str);
        if !path.exists() {
            return Err(format!("File not found: {}", path_str));
        }

        let transfer_id = uuid::Uuid::new_v4().to_string();

        let tid_clone = transfer_id.clone();
        let cancel_flag = || {
            state
                .cancel_flags
                .lock()
                .map(|flags| flags.get(&tid_clone).copied().unwrap_or(false))
                .unwrap_or(false)
        };

        let app_clone = app.clone();
        let progress_cb = move |p: TransferProgress| {
            let _ = app_clone.emit("transfer-progress", &p);
        };

        transfer::upload_file(
            session,
            storage_id,
            parent_handle,
            &path,
            &transfer_id,
            &cancel_flag,
            &progress_cb,
        )
        .map_err(|e| e.to_string())?;

        let _ = app.emit(
            "transfer-complete",
            serde_json::json!({ "transfer_id": transfer_id }),
        );
    }

    Ok(())
}

// ── Cancellation ────────────────────────────────────────────────

#[tauri::command]
pub fn cancel_transfer(transfer_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut flags = state.cancel_flags.lock().map_err(|e| e.to_string())?;
    flags.insert(transfer_id, true);
    Ok(())
}

// ── Deletion ────────────────────────────────────────────────────

#[tauri::command]
pub fn delete_objects(
    device_id: String,
    object_handles: Vec<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get_mut(&device_id)
        .ok_or_else(|| format!("No active session for device {}", device_id))?;

    for handle in &object_handles {
        session
            .delete_object(*handle)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
