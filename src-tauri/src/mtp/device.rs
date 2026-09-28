//! USB device discovery using `nusb`.
//!
//! Scans all connected USB devices and identifies those that expose
//! an MTP-compatible interface. Detection is based on **USB interface
//! class codes**, never on hardcoded vendor or product IDs, so every
//! MTP-capable Android device is supported regardless of manufacturer.
//!
//! ## How MTP devices are identified
//!
//! An Android phone in MTP mode exposes one of:
//! - USB interface class `0x06` (Still Image / PTP) — the standard
//! - USB interface class `0xFF` (Vendor Specific) with subclass `0x01`
//!   and protocol `0x01` — used by some Samsung and older devices

use nusb::transfer::{Bulk, In, Out};
use nusb::{Endpoint, MaybeFuture};

use super::protocol::MtpError;
use super::types::DeviceInfo;

/// USB class code for PTP / Still Image (which MTP extends).
const USB_CLASS_IMAGE: u8 = 0x06;
/// Vendor-specific class used by some Android MTP implementations.
const USB_CLASS_VENDOR: u8 = 0xFF;
/// MTP-specific subclass under vendor class.
const MTP_SUBCLASS: u8 = 0x01;
/// MTP-specific protocol under vendor class.
const MTP_PROTOCOL: u8 = 0x01;

/// Check if a given interface descriptor is MTP-compatible.
fn is_mtp_interface(cls: u8, sub: u8, proto: u8) -> bool {
    // Standard PTP/MTP class
    if cls == USB_CLASS_IMAGE {
        return true;
    }
    // Vendor-specific MTP (Samsung, some Xiaomi, etc.)
    if cls == USB_CLASS_VENDOR && sub == MTP_SUBCLASS && proto == MTP_PROTOCOL {
        return true;
    }
    false
}

/// Scan all connected USB devices and return those that expose an
/// MTP-compatible interface.
pub fn scan_mtp_devices() -> Result<Vec<DeviceInfo>, MtpError> {
    let mut devices = Vec::new();

    let device_list = nusb::list_devices()
        .wait()
        .map_err(|e| MtpError::Usb(e.to_string()))?;

    for info in device_list {
        let vid = info.vendor_id();
        let pid = info.product_id();

        // Check every interface on the device for MTP compatibility.
        let has_mtp = info.interfaces().any(|iface| {
            is_mtp_interface(iface.class(), iface.subclass(), iface.protocol())
        });

        if !has_mtp {
            continue;
        }

        let manufacturer = info
            .manufacturer_string()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let product = info
            .product_string()
            .map(|s| s.to_string())
            .unwrap_or_default();

        let name = if !manufacturer.is_empty() && !product.is_empty() {
            format!("{} {}", manufacturer, product)
        } else if !product.is_empty() {
            product
        } else {
            format!("USB Device {:04x}:{:04x}", vid, pid)
        };

        let id = format!("{:04x}:{:04x}:{}", vid, pid, info.bus_id());

        devices.push(DeviceInfo {
            id,
            name,
            vendor_id: vid,
            product_id: pid,
            connected: false,
        });
    }

    log::info!("Found {} MTP device(s)", devices.len());
    Ok(devices)
}

/// Opened MTP device handle containing the claimed interface and
/// typed bulk endpoints, ready for session use.
pub struct OpenedMtpDevice {
    pub interface: nusb::Interface,
    pub ep_out: Endpoint<Bulk, Out>,
    pub ep_in: Endpoint<Bulk, In>,
}

/// Open the USB device identified by the composite ID string
/// (`vid:pid:bus_id`), claim its MTP interface, and return typed
/// bulk endpoints — all in a single open operation.
///
/// This avoids the double-open bug where `find_mtp_endpoints` and
/// `MtpSession::open` would each open the device separately,
/// potentially causing exclusive-lock conflicts on macOS.
pub fn open_mtp_device(device_id: &str) -> Result<OpenedMtpDevice, MtpError> {
    let parts: Vec<&str> = device_id.split(':').collect();
    if parts.len() < 3 {
        return Err(MtpError::DeviceNotFound(format!(
            "invalid device ID format: {}",
            device_id
        )));
    }

    let vid = u16::from_str_radix(parts[0], 16)
        .map_err(|_| MtpError::DeviceNotFound(format!("invalid vid in: {}", device_id)))?;
    let pid = u16::from_str_radix(parts[1], 16)
        .map_err(|_| MtpError::DeviceNotFound(format!("invalid pid in: {}", device_id)))?;
    let bus_id_str = parts[2..].join(":");

    let device_list = nusb::list_devices()
        .wait()
        .map_err(|e| MtpError::Usb(e.to_string()))?;

    // Find by VID + PID + bus_id for unique identification
    let dev_info = device_list
        .into_iter()
        .find(|d| {
            d.vendor_id() == vid
                && d.product_id() == pid
                && d.bus_id() == bus_id_str
        })
        .ok_or_else(|| MtpError::DeviceNotFound(device_id.to_string()))?;

    let device = dev_info
        .open()
        .wait()
        .map_err(|e| MtpError::Usb(format!("failed to open device: {}", e)))?;

    let config = device
        .active_configuration()
        .map_err(|e| MtpError::Usb(format!("failed to get configuration: {}", e)))?;

    // Find the MTP interface and its bulk endpoints
    let mut found = None;
    for iface in config.interfaces() {
        let alt = match iface.alt_settings().next() {
            Some(a) => a,
            None => continue,
        };

        if !is_mtp_interface(alt.class(), alt.subclass(), alt.protocol()) {
            continue;
        }

        let iface_num = alt.interface_number();
        let mut bulk_out = None;
        let mut bulk_in = None;

        for ep in alt.endpoints() {
            if ep.transfer_type() == nusb::descriptors::TransferType::Bulk {
                if ep.direction() == nusb::transfer::Direction::Out {
                    bulk_out = Some(ep.address());
                } else {
                    bulk_in = Some(ep.address());
                }
            }
        }

        if let (Some(out), Some(inp)) = (bulk_out, bulk_in) {
            found = Some((iface_num, out, inp));
            log::info!(
                "MTP endpoints: iface={}, out=0x{:02x}, in=0x{:02x}",
                iface_num,
                out,
                inp
            );
            break;
        }
    }

    let (iface_num, ep_out_addr, ep_in_addr) = found.ok_or_else(|| {
        MtpError::Usb("No MTP interface with bulk endpoints found".into())
    })?;

    // Claim the interface
    let interface = device
        .claim_interface(iface_num)
        .wait()
        .map_err(|e| MtpError::Usb(format!("failed to claim interface: {}", e)))?;

    // Open typed bulk endpoints
    let ep_out: Endpoint<Bulk, Out> = interface
        .endpoint(ep_out_addr)
        .map_err(|e| MtpError::Usb(format!("failed to open OUT endpoint: {}", e)))?;

    let ep_in: Endpoint<Bulk, In> = interface
        .endpoint(ep_in_addr)
        .map_err(|e| MtpError::Usb(format!("failed to open IN endpoint: {}", e)))?;

    Ok(OpenedMtpDevice {
        interface,
        ep_out,
        ep_in,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_mtp_devices_does_not_panic() {
        let res = scan_mtp_devices();
        assert!(res.is_ok(), "scan_mtp_devices should succeed: {:?}", res.err());
    }
}
