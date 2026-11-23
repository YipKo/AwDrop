//! Read-only hardware inventory. A matching model is not a working backend.
use crate::{identify, Chip, registry::{Device, DeviceId}};
use std::io;

/// Parse a Windows PCI instance ID without storing only the model: the full
/// locator is needed to distinguish multiple cards with identical IDs.
#[cfg(any(windows, test))]
fn windows_pci_id(instance: &str) -> Option<DeviceId> {
    let (bus, rest) = instance.split_once('\\')?;
    if !bus.eq_ignore_ascii_case("PCI") { return None; }
    let hardware = rest.split('\\').next()?;
    let mut vendor = None;
    let mut device = None;
    for field in hardware.split('&') {
        if field.len() != 8 { continue; }
        let (Some(prefix), Some(value)) = (field.get(..4), field.get(4..)) else { continue };
        if prefix.eq_ignore_ascii_case("VEN_") {
            if vendor.is_some() { return None; }
            vendor = Some(u16::from_str_radix(value, 16).ok()?);
        }
        if prefix.eq_ignore_ascii_case("DEV_") {
            if device.is_some() { return None; }
            device = Some(u16::from_str_radix(value, 16).ok()?);
        }
    }
    Some(DeviceId::Pci { vendor: vendor?, device: device? })
}

/// List present PCI cards recognized by the common model table. This does not
/// claim the cards, check driver readiness, or mean the device can do AWDL.
pub fn known_devices() -> io::Result<Vec<Device>> {
    #[cfg(windows)] { windows::known_devices() }
    #[cfg(target_os = "linux")] { linux::known_devices() }
    #[cfg(not(any(windows, target_os = "linux")))] { Err(io::Error::new(io::ErrorKind::Unsupported, "hardware inventory unavailable")) }
}

fn known(id: DeviceId) -> bool {
    match id { DeviceId::Pci { vendor, device } => identify(vendor, device) != Chip::Unknown, DeviceId::Usb { .. } => false }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ffi::c_void, ptr};
    const DIGCF_PRESENT: u32 = 2;
    const DIGCF_ALLCLASSES: u32 = 4;
    const ERROR_NO_MORE_ITEMS: i32 = 259;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    const MAX_ID_LENGTH: u32 = 32768;
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Guid { data1: u32, data2: u16, data3: u16, data4: [u8; 8] }
    #[repr(C)]
    struct DeviceInfo { size: u32, class: Guid, devinst: u32, reserved: usize }
    #[link(name = "setupapi")]
    extern "system" {
        fn SetupDiGetClassDevsW(class: *const Guid, enumerator: *const u16, parent: *mut c_void, flags: u32) -> *mut c_void;
        fn SetupDiEnumDeviceInfo(set: *mut c_void, index: u32, info: *mut DeviceInfo) -> i32;
        fn SetupDiGetDeviceInstanceIdW(set: *mut c_void, info: *mut DeviceInfo, output: *mut u16, capacity: u32, required: *mut u32) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: *mut c_void) -> i32;
    }
    struct InfoSet(*mut c_void);
    impl Drop for InfoSet { fn drop(&mut self) { unsafe { SetupDiDestroyDeviceInfoList(self.0); } } }

    fn instance_id(set: &InfoSet, info: &mut DeviceInfo) -> io::Result<String> {
        let mut needed = 0u32;
        // SAFETY: the first call queries the size; `info` belongs to `set`.
        let first = unsafe { SetupDiGetDeviceInstanceIdW(set.0, info, ptr::null_mut(), 0, &mut needed) };
        if first == 0 && io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
            return Err(io::Error::last_os_error());
        }
        if needed == 0 || needed > MAX_ID_LENGTH { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid PnP identifier size")); }
        let mut buffer = vec![0u16; needed as usize];
        // SAFETY: the vector contains `needed` writable UTF-16 code units.
        if unsafe { SetupDiGetDeviceInstanceIdW(set.0, info, buffer.as_mut_ptr(), needed, &mut needed) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = buffer.iter().position(|&unit| unit == 0).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unterminated PnP identifier"))?;
        String::from_utf16(&buffer[..length]).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid PnP identifier"))
    }

    pub(super) fn known_devices() -> io::Result<Vec<Device>> {
        let enumerator: Vec<u16> = "PCI\0".encode_utf16().collect();
        // SAFETY: enumerator is NUL-terminated and the optional pointers are null.
        let handle = unsafe { SetupDiGetClassDevsW(ptr::null(), enumerator.as_ptr(), ptr::null_mut(), DIGCF_PRESENT | DIGCF_ALLCLASSES) };
        if handle as isize == -1 { return Err(io::Error::last_os_error()); }
        let set = InfoSet(handle);
        let mut devices = Vec::new();
        for index in 0..u32::MAX {
            let mut info = DeviceInfo { size: std::mem::size_of::<DeviceInfo>() as u32, class: Guid::default(), devinst: 0, reserved: 0 };
            // SAFETY: `info.size` is the structure size required by SetupAPI.
            if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut info) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS) { return Ok(devices); }
                return Err(error);
            }
            let instance = instance_id(&set, &mut info)?;
            if let Some(id) = windows_pci_id(&instance).filter(|&id| known(id)) {
                devices.push(Device { id, locator: instance });
            }
        }
        Err(io::Error::other("too many PCI devices"))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{fs, path::Path};
    fn value(path: &Path) -> io::Result<u16> {
        let raw = fs::read_to_string(path)?;
        u16::from_str_radix(raw.trim().trim_start_matches("0x"), 16).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid PCI id"))
    }
    pub(super) fn known_devices() -> io::Result<Vec<Device>> {
        let mut devices = Vec::new();
        for entry in fs::read_dir("/sys/bus/pci/devices")? {
            let entry = entry?;
            let path = entry.path();
            let (Ok(vendor), Ok(device)) = (value(&path.join("vendor")), value(&path.join("device"))) else { continue };
            let id = DeviceId::Pci { vendor, device };
            if known(id) { devices.push(Device { id, locator: path.to_string_lossy().into_owned() }); }
        }
        Ok(devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn windows_instance_id_parsing_requires_pci_and_complete_hex_fields() {
        assert_eq!(windows_pci_id(r"PCI\VEN_14C3&DEV_0616&SUBSYS_00000000\instance"), Some(DeviceId::Pci { vendor: 0x14c3, device: 0x0616 }));
        assert_eq!(windows_pci_id(r"pci\ven_0b48&dev_7922\instance"), Some(DeviceId::Pci { vendor: 0x0b48, device: 0x7922 }));
        assert_eq!(windows_pci_id(r"USB\VEN_14C3&DEV_0616\instance"), None);
        assert_eq!(windows_pci_id(r"PCI\VEN_14C3&DEV_zzzz\instance"), None);
        assert_eq!(windows_pci_id(r"PCI\VEN_14C3&VEN_1234&DEV_0616\instance"), None);
        assert_eq!(windows_pci_id("PCI\\abééé&VEN_14C3&DEV_0616\\instance"), Some(DeviceId::Pci { vendor: 0x14c3, device: 0x0616 }));
    }
}
