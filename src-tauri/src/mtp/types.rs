//! Shared types used across the MTP subsystem and Tauri commands.
//!
//! All types derive `Serialize` so they can be returned from Tauri
//! commands directly.

use serde::{Deserialize, Serialize};

/// A discovered USB device that appears to support MTP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique identifier (bus-address based).
    pub id: String,
    /// Human-readable name (manufacturer + product, or fallback).
    pub name: String,
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Whether we currently hold an MTP session.
    pub connected: bool,
}

/// A storage volume exposed by the device (e.g. "Internal Storage").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    /// MTP storage ID.
    pub id: u32,
    /// Human-readable description.
    pub description: String,
    /// Total capacity in bytes.
    pub max_capacity: u64,
    /// Free space in bytes.
    pub free_space: u64,
}

/// A file or folder inside an MTP storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// MTP object handle.
    pub handle: u32,
    /// File or folder name.
    pub name: String,
    /// True if this entry is a directory.
    pub is_folder: bool,
    /// Size in bytes (0 for folders).
    pub size: u64,
    /// Last-modified timestamp as ISO 8601 string (may be empty).
    pub modified: String,
}

/// Transfer progress event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub file_name: String,
    pub direction: String,
    pub progress: f64,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}
