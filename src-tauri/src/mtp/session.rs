//! MTP session management — opening/closing sessions and executing
//! MTP operations (browse storages, list files, get object info).
//!
//! Each session wraps a claimed USB interface and tracks the current
//! transaction ID. All I/O goes through bulk USB endpoints using the
//! PTP container format defined in `protocol.rs`.
//!
//! ## Status
//!
//! This is a real MTP protocol implementation built on `nusb`. It has
//! been written against the PTP/MTP specification but **has not yet
//! been tested with a physical device**. The USB discovery layer works
//! immediately; the session and transfer layers require a connected
//! Android phone for end-to-end validation.

use nusb::transfer::{Bulk, In, Out};
use nusb::{Endpoint, MaybeFuture};

use super::protocol::*;
use super::types::*;

use std::time::Duration;

/// Maximum USB bulk transfer size (64 KB).
const BULK_TRANSFER_SIZE: usize = 64 * 1024;
/// USB transfer timeout.
const TIMEOUT: Duration = Duration::from_secs(5);

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
    /// Claims the MTP interface, opens typed bulk endpoints, then
    /// sends an `OpenSession` command.
    pub fn open(vid: u16, pid: u16) -> Result<Self, MtpError> {
        let (iface_num, ep_out_addr, ep_in_addr) =
            super::device::find_mtp_endpoints(vid, pid)?;

        // Re-open the device and claim the interface
        let device_list = nusb::list_devices()
            .wait()
            .map_err(|e| MtpError::Usb(e.to_string()))?;
        let dev_info = device_list
            .into_iter()
            .find(|d| d.vendor_id() == vid && d.product_id() == pid)
            .ok_or_else(|| {
                MtpError::DeviceNotFound(format!("{:04x}:{:04x}", vid, pid))
            })?;

        let device = dev_info
            .open()
            .wait()
            .map_err(|e| MtpError::Usb(e.to_string()))?;
        let interface = device
            .claim_interface(iface_num)
            .wait()
            .map_err(|e| MtpError::Usb(e.to_string()))?;

        // Open typed bulk endpoints
        let ep_out: Endpoint<Bulk, Out> = interface
            .endpoint(ep_out_addr)
            .map_err(|e| MtpError::Usb(e.to_string()))?;

        let ep_in: Endpoint<Bulk, In> = interface
            .endpoint(ep_in_addr)
            .map_err(|e| MtpError::Usb(e.to_string()))?;

        let mut session = MtpSession {
            ep_out,
            ep_in,
            _interface: interface,
            _session_id: 1,
            transaction_id: 0,
        };

        // Send OpenSession command
        session.execute_command(OP_OPEN_SESSION, &[1])?;

        log::info!("MTP session opened for {:04x}:{:04x}", vid, pid);
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
    fn execute_data_in(
        &mut self,
        opcode: u16,
        params: &[u32],
    ) -> Result<(Vec<u8>, Container), MtpError> {
        let tid = self.next_tid();
        let cmd = build_command(opcode, tid, params);
        self.bulk_write(&cmd)?;

        // Read data phase
        let data_payload = self.read_data()?;

        // Read response
        let response = self.read_response()?;

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

    /// Write bytes to the bulk-out endpoint.
    fn bulk_write(&mut self, data: &[u8]) -> Result<(), MtpError> {
        let mut buf = nusb::transfer::Buffer::new(data.len());
        buf.extend_from_slice(data);
        let completion = self.ep_out.transfer_blocking(buf, TIMEOUT);
        completion
            .into_result()
            .map_err(|e| MtpError::Usb(format!("bulk write: {:?}", e)))?;
        Ok(())
    }

    /// Read from the bulk-in endpoint.
    fn bulk_read(&mut self) -> Result<Vec<u8>, MtpError> {
        let buf = nusb::transfer::Buffer::new(BULK_TRANSFER_SIZE);
        let completion = self.ep_in.transfer_blocking(buf, TIMEOUT);
        let completed = completion
            .into_result()
            .map_err(|e| MtpError::Usb(format!("bulk read: {:?}", e)))?;
        Ok(completed.into_vec())
    }

    /// Read a data-phase container (may span multiple USB transfers).
    fn read_data(&mut self) -> Result<Vec<u8>, MtpError> {
        let first = self.bulk_read()?;
        if first.len() < HEADER_LEN {
            return Err(MtpError::InvalidData("data container too short".into()));
        }

        let total_len =
            u32::from_le_bytes([first[0], first[1], first[2], first[3]]) as usize;

        // Extract payload (skip 12-byte header)
        let mut payload = Vec::with_capacity(total_len.saturating_sub(HEADER_LEN));
        if first.len() > HEADER_LEN {
            payload.extend_from_slice(&first[HEADER_LEN..]);
        }

        // If the data spans multiple USB transfers, keep reading
        while payload.len() + HEADER_LEN < total_len {
            let chunk = self.bulk_read()?;
            payload.extend_from_slice(&chunk);
        }

        // Trim to exact size
        let expected_payload = total_len.saturating_sub(HEADER_LEN);
        payload.truncate(expected_payload);

        Ok(payload)
    }

    /// Read a response container.
    fn read_response(&mut self) -> Result<Container, MtpError> {
        let raw = self.bulk_read()?;
        let container = parse_container(&raw)?;

        if container.container_type != CONTAINER_RESPONSE {
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
        let parent = if parent_handle == 0 {
            0xFFFFFFFFu32
        } else {
            parent_handle
        };
        let (data, _resp) =
            self.execute_data_in(OP_GET_OBJECT_HANDLES, &[storage_id, 0, parent])?;
        read_u32_array(&data, 0)
    }

    /// Get information about a specific object (file or folder).
    pub fn get_object_info(&mut self, handle: u32) -> Result<FileEntry, MtpError> {
        let (data, _resp) =
            self.execute_data_in(OP_GET_OBJECT_INFO, &[handle])?;

        // ObjectInfo dataset layout:
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
    pub fn send_object(
        &mut self,
        storage_id: u32,
        parent_handle: u32,
        filename: &str,
        file_data: &[u8],
    ) -> Result<u32, MtpError> {
        // Build ObjectInfo dataset for SendObjectInfo
        let mut info = Vec::new();
        info.extend_from_slice(&storage_id.to_le_bytes());
        info.extend_from_slice(&0x3000u16.to_le_bytes()); // Undefined format
        info.extend_from_slice(&0u16.to_le_bytes()); // ProtectionStatus
        info.extend_from_slice(&(file_data.len() as u32).to_le_bytes());
        info.extend_from_slice(&0u16.to_le_bytes()); // ThumbFormat
        info.extend_from_slice(&0u32.to_le_bytes()); // ThumbCompressedSize
        info.extend_from_slice(&0u32.to_le_bytes()); // ThumbPixWidth
        info.extend_from_slice(&0u32.to_le_bytes()); // ThumbPixHeight
        info.extend_from_slice(&0u32.to_le_bytes()); // ImagePixWidth
        info.extend_from_slice(&0u32.to_le_bytes()); // ImagePixHeight
        info.extend_from_slice(&0u32.to_le_bytes()); // ImageBitDepth
        info.extend_from_slice(&parent_handle.to_le_bytes());
        info.extend_from_slice(&0u16.to_le_bytes()); // AssociationType
        info.extend_from_slice(&0u32.to_le_bytes()); // AssociationDesc
        info.extend_from_slice(&0u32.to_le_bytes()); // SequenceNumber
        info.extend(&encode_ptp_string(filename));
        info.extend(&encode_ptp_string("")); // DateCreated
        info.extend(&encode_ptp_string("")); // DateModified
        info.extend(&encode_ptp_string("")); // Keywords

        let parent = if parent_handle == 0 {
            0xFFFFFFFFu32
        } else {
            parent_handle
        };
        let resp =
            self.execute_data_out(OP_SEND_OBJECT_INFO, &[storage_id, parent], &info)?;

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
        self.execute_command(OP_DELETE_OBJECT, &[handle, 0])?;
        log::info!("Deleted object handle {}", handle);
        Ok(())
    }
}
