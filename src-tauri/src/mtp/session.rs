//! MTP session management — opening/closing sessions and executing
//! MTP operations (browse storages, list files, get object info).
//!
//! Each session wraps a claimed USB interface and tracks the current
//! transaction ID. All I/O goes through bulk USB endpoints using the
//! PTP container format defined in `protocol.rs`.
//!
//! ## Implementation Notes
//!
//! - Single-open design: the device is opened once in `device::open_mtp_device`
//!   and the handles are transferred here. No double-open issues.
//! - Handles `RC_SESSION_ALREADY_OPEN` gracefully by closing and reopening.
//! - Robust data+response parsing handles devices that pack response right
//!   after data in the same USB transfer.
//! - Stall recovery via `clear_halt` + retry.

use nusb::transfer::{Bulk, In, Out};
use nusb::{Endpoint, MaybeFuture};

use super::device::OpenedMtpDevice;
use super::protocol::*;
use super::types::*;

use std::time::Duration;

/// Maximum USB bulk transfer size (64 KB).
const BULK_TRANSFER_SIZE: usize = 64 * 1024;
/// USB transfer timeout.
const TIMEOUT: Duration = Duration::from_secs(10);
/// Extended timeout for large data transfers.
const DATA_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum retries on stall.
const MAX_STALL_RETRIES: u32 = 2;

/// An active MTP session on a USB device.
pub struct MtpSession {
    /// Bulk-out endpoint for sending commands/data to device.
    ep_out: Endpoint<Bulk, Out>,
    /// Bulk-in endpoint for receiving data/responses from device.
    ep_in: Endpoint<Bulk, In>,
    /// The claimed interface (kept alive for the session lifetime).
    _interface: nusb::Interface,
    /// MTP session ID (always 1 for a single session).
    _session_id: u32,
    /// Monotonically increasing transaction counter.
    transaction_id: u32,
}

impl MtpSession {
    /// Open a new MTP session on the given USB device.
    ///
    /// Uses the pre-opened device handles from `device::open_mtp_device`.
    pub fn open(opened: OpenedMtpDevice) -> Result<Self, MtpError> {
        let mut session = MtpSession {
            ep_out: opened.ep_out,
            ep_in: opened.ep_in,
            _interface: opened.interface,
            _session_id: 1,
            transaction_id: 0,
        };

        // Send OpenSession command
        match session.execute_command(OP_OPEN_SESSION, &[1]) {
            Ok(_) => {
                log::info!("MTP session opened successfully");
            }
            Err(MtpError::Protocol(code)) if code == RC_SESSION_ALREADY_OPEN => {
                // Session already open — close it and reopen
                log::warn!("Session already open (0x{:04X}), closing and reopening", code);
                let _ = session.execute_command(OP_CLOSE_SESSION, &[]);
                session.transaction_id = 0;
                session.execute_command(OP_OPEN_SESSION, &[1])?;
                log::info!("MTP session reopened successfully");
            }
            Err(e) => return Err(e),
        }

        Ok(session)
    }

    /// Close the MTP session gracefully.
    pub fn close(&mut self) -> Result<(), MtpError> {
        let _ = self.execute_command(OP_CLOSE_SESSION, &[]);
        log::info!("MTP session closed");
        Ok(())
    }

    /// Get the next transaction ID.
    fn next_tid(&mut self) -> u32 {
        self.transaction_id += 1;
        self.transaction_id
    }

    // ── Low-level USB I/O ───────────────────────────────────────

    /// Write bytes to the bulk-out endpoint with stall recovery.
    fn bulk_write(&mut self, data: &[u8]) -> Result<(), MtpError> {
        self.bulk_write_inner(data, 0)
    }

    fn bulk_write_inner(&mut self, data: &[u8], retry: u32) -> Result<(), MtpError> {
        let buf = nusb::transfer::Buffer::from(data);
        let completion = self.ep_out.transfer_blocking(buf, TIMEOUT);

        match completion.status {
            Ok(()) => Ok(()),
            Err(nusb::transfer::TransferError::Stall) if retry < MAX_STALL_RETRIES => {
                log::warn!("OUT endpoint stalled, clearing halt (retry {})", retry + 1);
                self.ep_out
                    .clear_halt()
                    .wait()
                    .map_err(|e| MtpError::Usb(format!("clear_halt OUT: {}", e)))?;
                self.bulk_write_inner(data, retry + 1)
            }
            Err(e) => Err(MtpError::Usb(format!("bulk write: {}", e))),
        }
    }

    /// Read from the bulk-in endpoint with stall recovery.
    fn bulk_read(&mut self) -> Result<Vec<u8>, MtpError> {
        self.bulk_read_with_timeout(TIMEOUT)
    }

    fn bulk_read_with_timeout(&mut self, timeout: Duration) -> Result<Vec<u8>, MtpError> {
        self.bulk_read_inner(timeout, 0)
    }

    fn bulk_read_inner(&mut self, timeout: Duration, retry: u32) -> Result<Vec<u8>, MtpError> {
        let buf = nusb::transfer::Buffer::new(BULK_TRANSFER_SIZE);
        let completion = self.ep_in.transfer_blocking(buf, timeout);

        match completion.status {
            Ok(()) => Ok(completion.buffer.into_vec()),
            Err(nusb::transfer::TransferError::Stall) if retry < MAX_STALL_RETRIES => {
                log::warn!("IN endpoint stalled, clearing halt (retry {})", retry + 1);
                self.ep_in
                    .clear_halt()
                    .wait()
                    .map_err(|e| MtpError::Usb(format!("clear_halt IN: {}", e)))?;
                self.bulk_read_inner(timeout, retry + 1)
            }
            Err(e) => Err(MtpError::Usb(format!("bulk read: {}", e))),
        }
    }

    // ── Mid-level MTP transaction helpers ────────────────────────

    /// Send a command and read the response (no data phase).
    fn execute_command(
        &mut self,
        opcode: u16,
        params: &[u32],
    ) -> Result<Container, MtpError> {
        let tid = self.next_tid();
        let cmd = build_command(opcode, tid, params);
        self.bulk_write(&cmd)?;
        self.read_response()
    }

    /// Send a command that expects a data-in phase, then read the response.
    ///
    /// Handles the case where the response is packed into the same USB
    /// transfer as the tail of the data phase.
    fn execute_data_in(
        &mut self,
        opcode: u16,
        params: &[u32],
    ) -> Result<(Vec<u8>, Container), MtpError> {
        let tid = self.next_tid();
        let cmd = build_command(opcode, tid, params);
        self.bulk_write(&cmd)?;

        // Read data phase — may contain response appended at the end
        let (data_payload, trailing_bytes) = self.read_data_phase()?;

        // If there were trailing bytes after the data container, they may
        // be the response container
        let response = if trailing_bytes.len() >= HEADER_LEN {
            parse_container(&trailing_bytes)?
        } else {
            // Response is in a separate USB transfer
            self.read_response()?
        };

        if response.container_type != CONTAINER_RESPONSE {
            return Err(MtpError::InvalidData(format!(
                "expected response container after data, got type {}",
                response.container_type
            )));
        }

        if response.code != RC_OK {
            return Err(MtpError::Protocol(response.code));
        }

        Ok((data_payload, response))
    }

    /// Send a command with a data-out phase, then read the response.
    fn execute_data_out(
        &mut self,
        opcode: u16,
        params: &[u32],
        data: &[u8],
    ) -> Result<Container, MtpError> {
        let tid = self.next_tid();
        let cmd = build_command(opcode, tid, params);
        self.bulk_write(&cmd)?;

        // Send data phase
        let data_packet = build_data(opcode, tid, data);
        self.bulk_write(&data_packet)?;

        // Read response
        self.read_response()
    }

    /// Read a data-phase container that may span multiple USB transfers.
    ///
    /// Returns `(payload, trailing_bytes)` where `trailing_bytes` contains
    /// any bytes read past the end of the data container (which may be the
    /// response container packed into the same transfer).
    fn read_data_phase(&mut self) -> Result<(Vec<u8>, Vec<u8>), MtpError> {
        let first = self.bulk_read_with_timeout(DATA_TIMEOUT)?;
        if first.len() < HEADER_LEN {
            return Err(MtpError::InvalidData("data container too short".into()));
        }

        let container_type = u16::from_le_bytes([first[4], first[5]]);
        if container_type != CONTAINER_DATA {
            return Err(MtpError::InvalidData(format!(
                "expected data container (type 2), got type {}",
                container_type
            )));
        }

        let total_len =
            u32::from_le_bytes([first[0], first[1], first[2], first[3]]) as usize;

        // Handle special case: total_len == 0xFFFFFFFF means unknown length
        // (the device will send a ZLP to signal end)
        let known_length = total_len != 0xFFFFFFFF;
        let expected_payload = if known_length {
            total_len.saturating_sub(HEADER_LEN)
        } else {
            0 // will grow dynamically
        };

        let mut payload = Vec::with_capacity(if known_length { expected_payload } else { BULK_TRANSFER_SIZE });
        let mut trailing = Vec::new();

        // Extract payload from first transfer (skip 12-byte header)
        if first.len() > HEADER_LEN {
            if known_length && first.len() > total_len {
                // First transfer contains data + trailing (possibly response)
                payload.extend_from_slice(&first[HEADER_LEN..total_len]);
                trailing.extend_from_slice(&first[total_len..]);
            } else {
                payload.extend_from_slice(&first[HEADER_LEN..]);
            }
        }

        if known_length {
            // Keep reading until we have the full payload
            while payload.len() < expected_payload {
                let chunk = self.bulk_read_with_timeout(DATA_TIMEOUT)?;
                let remaining = expected_payload - payload.len();
                if chunk.len() > remaining {
                    // This chunk contains end-of-data + trailing (response)
                    payload.extend_from_slice(&chunk[..remaining]);
                    trailing.extend_from_slice(&chunk[remaining..]);
                } else {
                    payload.extend_from_slice(&chunk);
                }
            }
            payload.truncate(expected_payload);
        } else {
            // Unknown length — read until ZLP (zero-length packet) or short packet
            loop {
                let chunk = self.bulk_read_with_timeout(DATA_TIMEOUT)?;
                if chunk.is_empty() {
                    break; // ZLP signals end
                }
                payload.extend_from_slice(&chunk);
                if chunk.len() < BULK_TRANSFER_SIZE {
                    break; // Short packet signals end
                }
            }
        }

        Ok((payload, trailing))
    }

    /// Read a response container.
    fn read_response(&mut self) -> Result<Container, MtpError> {
        let raw = self.bulk_read()?;
        let container = parse_container(&raw)?;

        if container.container_type != CONTAINER_RESPONSE {
            // Some devices send an event container before the response;
            // skip events and keep reading
            if container.container_type == CONTAINER_EVENT {
                log::debug!("Received event (code 0x{:04X}), reading response", container.code);
                let raw2 = self.bulk_read()?;
                let resp = parse_container(&raw2)?;
                if resp.container_type != CONTAINER_RESPONSE {
                    return Err(MtpError::InvalidData(format!(
                        "expected response container, got type {} after event",
                        resp.container_type
                    )));
                }
                if resp.code != RC_OK {
                    return Err(MtpError::Protocol(resp.code));
                }
                return Ok(resp);
            }

            return Err(MtpError::InvalidData(format!(
                "expected response container, got type {}",
                container.container_type
            )));
        }

        if container.code != RC_OK {
            return Err(MtpError::Protocol(container.code));
        }

        Ok(container)
    }

    // ── High-level MTP operations ───────────────────────────────

    /// List storage IDs on the device.
    pub fn get_storage_ids(&mut self) -> Result<Vec<u32>, MtpError> {
        let (data, _resp) = self.execute_data_in(OP_GET_STORAGE_IDS, &[])?;
        read_u32_array(&data, 0)
    }

    /// Get information about a storage volume.
    pub fn get_storage_info(
        &mut self,
        storage_id: u32,
    ) -> Result<StorageInfo, MtpError> {
        let (data, _resp) =
            self.execute_data_in(OP_GET_STORAGE_INFO, &[storage_id])?;

        // StorageInfo dataset layout:
        //  0: u16  StorageType
        //  2: u16  FilesystemType
        //  4: u16  AccessCapability
        //  6: u64  MaxCapacity
        // 14: u64  FreeSpaceInBytes
        // 22: u32  FreeSpaceInObjects
        // 26: PTP string StorageDescription
        let max_capacity = read_u64(&data, 6).unwrap_or(0);
        let free_space = read_u64(&data, 14).unwrap_or(0);
        let (description, _) = read_ptp_string(&data, 26)?;

        let description = if description.is_empty() {
            format!("Storage 0x{:08X}", storage_id)
        } else {
            description
        };

        Ok(StorageInfo {
            id: storage_id,
            description,
            max_capacity,
            free_space,
        })
    }

    /// List object handles in a storage/folder.
    pub fn get_object_handles(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
    ) -> Result<Vec<u32>, MtpError> {
        // 0xFFFFFFFF means root folder in MTP
        let parent = if parent_handle == 0 {
            0xFFFFFFFFu32
        } else {
            parent_handle
        };
        // Params: StorageID, ObjectFormatCode (0 = all), ParentObject
        let (data, _resp) =
            self.execute_data_in(OP_GET_OBJECT_HANDLES, &[storage_id, 0, parent])?;
        read_u32_array(&data, 0)
    }

    /// Get information about a specific object (file or folder).
    pub fn get_object_info(&mut self, handle: u32) -> Result<FileEntry, MtpError> {
        let (data, _resp) =
            self.execute_data_in(OP_GET_OBJECT_INFO, &[handle])?;

        // ObjectInfo dataset layout (offsets in bytes):
        //  0: u32  StorageID
        //  4: u16  ObjectFormat
        //  6: u16  ProtectionStatus
        //  8: u32  ObjectCompressedSize (lower 32 bits)
        // 12: u16  ThumbFormat
        // 14: u32  ThumbCompressedSize
        // 18: u32  ThumbPixWidth
        // 22: u32  ThumbPixHeight
        // 26: u32  ImagePixWidth
        // 30: u32  ImagePixHeight
        // 34: u32  ImageBitDepth
        // 38: u32  ParentObject
        // 42: u16  AssociationType
        // 44: u32  AssociationDesc
        // 48: u32  SequenceNumber
        // 52: PTP string Filename
        let format = read_u16(&data, 4).unwrap_or(0);
        let size = read_u32(&data, 8).unwrap_or(0) as u64;
        let is_folder = format == FORMAT_ASSOCIATION;

        // Parse the variable-length strings starting at offset 52
        let (name, name_len) = read_ptp_string(&data, 52)?;
        let date_offset = 52 + name_len;
        let (_date_created, dc_len) =
            read_ptp_string(&data, date_offset).unwrap_or_default();
        let (date_modified, _) =
            read_ptp_string(&data, date_offset + dc_len).unwrap_or_default();

        Ok(FileEntry {
            handle,
            name,
            is_folder,
            size,
            modified: date_modified,
        })
    }

    /// Download the raw bytes of an object (file).
    pub fn get_object(&mut self, handle: u32) -> Result<Vec<u8>, MtpError> {
        let (data, _resp) = self.execute_data_in(OP_GET_OBJECT, &[handle])?;
        Ok(data)
    }

    /// Upload a file to the device.
    ///
    /// Returns the new object handle assigned by the device.
    pub fn send_object(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        filename: &str,
        file_data: &[u8],
    ) -> Result<u32, MtpError> {
        // Build ObjectInfo dataset for SendObjectInfo
        let mut info = Vec::new();
        info.extend_from_slice(&storage_id.to_le_bytes());       // StorageID
        info.extend_from_slice(&0x3000u16.to_le_bytes());        // ObjectFormat: Undefined
        info.extend_from_slice(&0u16.to_le_bytes());             // ProtectionStatus
        info.extend_from_slice(&(file_data.len() as u32).to_le_bytes()); // CompressedSize
        info.extend_from_slice(&0u16.to_le_bytes());             // ThumbFormat
        info.extend_from_slice(&0u32.to_le_bytes());             // ThumbCompressedSize
        info.extend_from_slice(&0u32.to_le_bytes());             // ThumbPixWidth
        info.extend_from_slice(&0u32.to_le_bytes());             // ThumbPixHeight
        info.extend_from_slice(&0u32.to_le_bytes());             // ImagePixWidth
        info.extend_from_slice(&0u32.to_le_bytes());             // ImagePixHeight
        info.extend_from_slice(&0u32.to_le_bytes());             // ImageBitDepth
        info.extend_from_slice(&parent_handle.to_le_bytes());    // ParentObject
        info.extend_from_slice(&0u16.to_le_bytes());             // AssociationType
        info.extend_from_slice(&0u32.to_le_bytes());             // AssociationDesc
        info.extend_from_slice(&0u32.to_le_bytes());             // SequenceNumber
        info.extend(&encode_ptp_string(filename));               // Filename
        info.extend(&encode_ptp_string(""));                     // DateCreated
        info.extend(&encode_ptp_string(""));                     // DateModified
        info.extend(&encode_ptp_string(""));                     // Keywords

        let parent = if parent_handle == 0 {
            0xFFFFFFFFu32
        } else {
            parent_handle
        };
        let resp =
            self.execute_data_out(OP_SEND_OBJECT_INFO, &[storage_id, parent], &info)?;

        // Response params for SendObjectInfo:
        //   param1: StorageID used
        //   param2: Parent handle used
        //   param3: New object handle
        let new_handle = if resp.payload.len() >= 12 {
            read_u32(&resp.payload, 8)?
        } else {
            0
        };

        // SendObject — send the actual file data
        self.execute_data_out(OP_SEND_OBJECT, &[], file_data)?;

        log::info!("Uploaded '{}' → handle {}", filename, new_handle);
        Ok(new_handle)
    }

    /// Delete an object from the device.
    pub fn delete_object(&mut self, handle: u32) -> Result<(), MtpError> {
        // Params: ObjectHandle, ObjectFormatCode (0 = regardless of format)
        self.execute_command(OP_DELETE_OBJECT, &[handle, 0])?;
        log::info!("Deleted object handle {}", handle);
        Ok(())
    }
}
