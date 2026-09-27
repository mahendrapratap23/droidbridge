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

use nusb::MaybeFuture;
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
            let cls = iface.class();
            let sub = iface.subclass();
            let proto = iface.protocol();

            // Standard PTP/MTP class
            if cls == USB_CLASS_IMAGE {
                return true;
            }
            // Vendor-specific MTP (Samsung, some Xiaomi, etc.)
            if cls == USB_CLASS_VENDOR && sub == MTP_SUBCLASS && proto == MTP_PROTOCOL {
                return true;
            }
            false
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

/// Find the MTP interface number and bulk endpoint addresses for a device.
/// Returns `(interface_number, bulk_out_ep, bulk_in_ep)`.
pub fn find_mtp_endpoints(
    vid: u16,
    pid: u16,
) -> Result<(u8, u8, u8), MtpError> {
    let device_list = nusb::list_devices()
        .wait()
        .map_err(|e| MtpError::Usb(e.to_string()))?;

    let dev_info = device_list
        .into_iter()
        .find(|d| d.vendor_id() == vid && d.product_id() == pid)
        .ok_or_else(|| MtpError::DeviceNotFound(format!("{:04x}:{:04x}", vid, pid)))?;

    let device = dev_info
        .open()
        .wait()
        .map_err(|e| MtpError::Usb(e.to_string()))?;
    let config = device
        .active_configuration()
        .map_err(|e| MtpError::Usb(e.to_string()))?;

    for iface in config.interfaces() {
        let alt = iface.alt_settings().next();
        let alt = match alt {
            Some(a) => a,
            None => continue,
        };

        let cls = alt.class();
        let sub = alt.subclass();
        let proto = alt.protocol();

        let is_mtp = cls == USB_CLASS_IMAGE
            || (cls == USB_CLASS_VENDOR && sub == MTP_SUBCLASS && proto == MTP_PROTOCOL);

        if !is_mtp {
            continue;
        }

        let iface_num = alt.interface_number();
        let mut bulk_out = None;
        let mut bulk_in = None;

        for ep in alt.endpoints() {
            match ep.transfer_type() {
                nusb::descriptors::TransferType::Bulk => {
                    if ep.direction() == nusb::transfer::Direction::Out {
                        bulk_out = Some(ep.address());
                    } else {
                        bulk_in = Some(ep.address());
                    }
                }
                _ => {}
            }
        }

        if let (Some(out), Some(inp)) = (bulk_out, bulk_in) {
            log::info!(
                "MTP endpoints: iface={}, out=0x{:02x}, in=0x{:02x}",
                iface_num,
                out,
                inp
            );
            return Ok((iface_num, out, inp));
        }
    }

    Err(MtpError::Usb(
        "No MTP interface with bulk endpoints found".into(),
    ))
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

