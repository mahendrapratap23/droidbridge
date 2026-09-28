//! File transfer with progress reporting and cancellation.
//!
//! Wraps the low-level `MtpSession` operations to add:
//! - Progress callbacks (bytes transferred / total)
//! - Cancellation via a shared flag
//! - Proper error wrapping

use std::fs;
use std::path::Path;

use super::protocol::MtpError;
use super::session::MtpSession;
use super::types::TransferProgress;

/// Download a file from the device to a local path.
///
/// Emits progress updates via the `on_progress` callback.
/// Check `cancel_flag` between chunks to support cancellation.
pub fn download_file(
    session: &mut MtpSession,
    handle: u32,
    file_name: &str,
    dest_dir: &Path,
    transfer_id: &str,
    cancel_flag: &dyn Fn() -> bool,
    on_progress: &dyn Fn(TransferProgress),
) -> Result<(), MtpError> {
    if cancel_flag() {
        return Err(MtpError::Cancelled);
    }

    // Emit initial progress
    on_progress(TransferProgress {
        transfer_id: transfer_id.to_string(),
        file_name: file_name.to_string(),
        direction: "download".to_string(),
        progress: 0.0,
        bytes_transferred: 0,
        total_bytes: 0,
    });

    // Get the object data via MTP
    let data = session.get_object(handle)?;

    if cancel_flag() {
        return Err(MtpError::Cancelled);
    }

    // Write to disk
    let dest_path = dest_dir.join(file_name);
    fs::write(&dest_path, &data)?;

    // Emit completion progress
    on_progress(TransferProgress {
        transfer_id: transfer_id.to_string(),
        file_name: file_name.to_string(),
        direction: "download".to_string(),
        progress: 1.0,
        bytes_transferred: data.len() as u64,
        total_bytes: data.len() as u64,
    });

    log::info!("Downloaded '{}' → {:?}", file_name, dest_path);
    Ok(())
}

/// Upload a local file to the device.
///
/// Emits progress updates via the `on_progress` callback.
pub fn upload_file(
    session: &mut MtpSession,
    storage_id: u32,
    parent_handle: u32,
    file_path: &Path,
    transfer_id: &str,
    cancel_flag: &dyn Fn() -> bool,
    on_progress: &dyn Fn(TransferProgress),
) -> Result<u32, MtpError> {
    if cancel_flag() {
        return Err(MtpError::Cancelled);
    }

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let data = fs::read(file_path)?;
    let total = data.len() as u64;

    // Emit initial progress
    on_progress(TransferProgress {
        transfer_id: transfer_id.to_string(),
        file_name: file_name.to_string(),
        direction: "upload".to_string(),
        progress: 0.0,
        bytes_transferred: 0,
        total_bytes: total,
    });

    if cancel_flag() {
        return Err(MtpError::Cancelled);
    }

    // Send file via MTP
    let handle = session.send_object(storage_id, parent_handle, file_name, &data)?;

    // Emit completion progress
    on_progress(TransferProgress {
        transfer_id: transfer_id.to_string(),
        file_name: file_name.to_string(),
        direction: "upload".to_string(),
        progress: 1.0,
        bytes_transferred: total,
        total_bytes: total,
    });

    Ok(handle)
}
