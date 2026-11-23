//! ABI for the MT7922 Windows diagnostic prototype.
//! A diagnostic device is not a radio backend and cannot transfer files.
use std::io;

const SIZE: usize = 48;
const DMA_SIZE: usize = 20;
const OWN_SIZE: usize = 20;
const RESET_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResetCapabilities { pub function: bool, pub platform: bool }
impl ResetCapabilities {
    /// A returned capability is not a guarantee that a reset will succeed or
    /// that it is sufficient to fence DMA. `None` means the query was unavailable.
    pub fn decode(bytes: &[u8]) -> io::Result<Option<Self>> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid MT7922 reset capability response");
        if bytes.len() != RESET_SIZE { return Err(invalid()); }
        let word = |i: usize| u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap());
        if word(0) as usize != RESET_SIZE || word(1) != 1 || word(2) > 1 || word(3) & !3 != 0 { return Err(invalid()); }
        match (word(2), word(3)) {
            (0, 0) => Ok(None),
            (1, flags) => Ok(Some(Self { function: flags & 1 != 0, platform: flags & 2 != 0 })),
            _ => Err(invalid()),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipOutcome {
    RoundTripSucceeded,
    InitiallyDriverOwned,
    RegisterUnavailable,
    ClaimFailedButRestored,
    RecoveryFailed,
    InspectionFailedButRestored,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaState { pub tx_enabled: bool, pub tx_busy: bool, pub rx_enabled: bool, pub rx_busy: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipDiagnostic { pub outcome: OwnershipOutcome, pub dma: Option<DmaState> }
impl OwnershipDiagnostic {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid MT7922 ownership response");
        if bytes.len() != OWN_SIZE { return Err(invalid()); }
        let word = |i: usize| u32::from_le_bytes(bytes[i*4..i*4+4].try_into().unwrap());
        if word(0) as usize != OWN_SIZE || word(1) != 2 || word(3) > 1 || word(4) & !0xf != 0 { return Err(invalid()); }
        let outcome = match word(2) {
            1 => OwnershipOutcome::RoundTripSucceeded,
            2 => OwnershipOutcome::InitiallyDriverOwned,
            3 => OwnershipOutcome::RegisterUnavailable,
            4 => OwnershipOutcome::ClaimFailedButRestored,
            5 => OwnershipOutcome::RecoveryFailed,
            6 => OwnershipOutcome::InspectionFailedButRestored,
            _ => return Err(invalid()),
        };
        if (word(3) == 0 && word(4) != 0) ||
            (outcome == OwnershipOutcome::RoundTripSucceeded && word(3) != 1) ||
            (matches!(outcome, OwnershipOutcome::InitiallyDriverOwned | OwnershipOutcome::RegisterUnavailable |
                OwnershipOutcome::ClaimFailedButRestored | OwnershipOutcome::InspectionFailedButRestored) && word(3) != 0) { return Err(invalid()); }
        let dma = (word(3) == 1).then(|| DmaState { tx_enabled: word(4) & 1 != 0, tx_busy: word(4) & 2 != 0,
            rx_enabled: word(4) & 4 != 0, rx_busy: word(4) & 8 != 0 });
        Ok(Self { outcome, dma })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticInfo {
    pub vendor: u16,
    pub device: u16,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub revision: u8,
    pub memory_resources: u32,
    pub interrupt_resources: u32,
    pub bar0_bytes: Option<u32>,
    pub bar0_matches: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmaDiagnostic {
    pub allocated_bytes: u32,
    pub address_bits: u32,
    pub alignment: u32,
}
impl DmaDiagnostic {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid MT7922 DMA diagnostic response");
        if bytes.len() != DMA_SIZE { return Err(invalid()); }
        let word = |index: usize| u32::from_le_bytes(bytes[index*4..index*4+4].try_into().unwrap());
        if word(0) as usize != DMA_SIZE || word(1) != 3 || word(2) != (256 + 128 + 8) * 16 + 7 * 2048 + 2 * 4096 || word(3) != 32 || word(4) != 16 { return Err(invalid()); }
        Ok(Self { allocated_bytes: word(2), address_bits: word(3), alignment: word(4) })
    }
}

impl DiagnosticInfo {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidData, "invalid MT7922 diagnostic response") }
        if bytes.len() != SIZE { return Err(invalid()); }
        let word = |index: usize| u32::from_le_bytes(bytes[index*4..index*4+4].try_into().unwrap());
        if word(0) as usize != SIZE || word(1) != 1 || word(7) != 0 { return Err(invalid()); }
        let vendor = u16::try_from(word(2)).map_err(|_| invalid())?;
        let device = u16::try_from(word(3)).map_err(|_| invalid())?;
        if crate::identify(vendor, device) != crate::Chip::Mt7922 { return Err(invalid()); }
        let matches = word(11);
        let bar_bytes = word(10);
        if matches > word(8) || (matches != 1 && bar_bytes != 0) { return Err(invalid()); }
        Ok(Self { vendor, device,
            subsystem_vendor: u16::try_from(word(4)).map_err(|_| invalid())?,
            subsystem_device: u16::try_from(word(5)).map_err(|_| invalid())?,
            revision: u8::try_from(word(6)).map_err(|_| invalid())?,
            memory_resources: word(8), interrupt_resources: word(9),
            bar0_bytes: if matches == 1 && bar_bytes != 0 { Some(bar_bytes) } else { None },
            bar0_matches: matches })
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use std::{ffi::{c_void, OsStr, OsString}, os::windows::ffi::{OsStrExt, OsStringExt}, ptr};
    const DIGCF_PRESENT: u32 = 2;
    const DIGCF_DEVICEINTERFACE: u32 = 0x10;
    const ERROR_NO_MORE_ITEMS: i32 = 259;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    const IOCTL_GET_INFO: u32 = (0x22 << 16) | (1 << 14) | (0x800 << 2);
    const IOCTL_DMA_TEST: u32 = (0x22 << 16) | (1 << 14) | (0x801 << 2);
    const IOCTL_OWN_TEST: u32 = (0x22 << 16) | (2 << 14) | (0x802 << 2);
    const IOCTL_RESET_INFO: u32 = (0x22 << 16) | (1 << 14) | (0x803 << 2);
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const OPEN_EXISTING: u32 = 3;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid { data1: u32, data2: u16, data3: u16, data4: [u8; 8] }
    const INTERFACE: Guid = Guid { data1: 0x49e5285d, data2: 0x962b, data3: 0x4d98, data4: [0xa7,0xc0,0x5c,0x72,0xe8,0x60,0xa8,0x16] };
    #[repr(C)]
    struct InterfaceData { size: u32, class: Guid, flags: u32, reserved: usize }
    #[link(name = "setupapi")]
    extern "system" {
        fn SetupDiGetClassDevsW(class: *const Guid, enumerator: *const u16, parent: *mut c_void, flags: u32) -> *mut c_void;
        fn SetupDiEnumDeviceInterfaces(set: *mut c_void, info: *const c_void, class: *const Guid, index: u32, interface: *mut InterfaceData) -> i32;
        fn SetupDiGetDeviceInterfaceDetailW(set: *mut c_void, interface: *mut InterfaceData, detail: *mut c_void, size: u32, required: *mut u32, info: *mut c_void) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(path: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: *mut c_void) -> *mut c_void;
        fn DeviceIoControl(handle: *mut c_void, code: u32, input: *const c_void, input_size: u32, output: *mut c_void, output_size: u32, returned: *mut u32, overlapped: *mut c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    struct InfoSet(*mut c_void);
    impl Drop for InfoSet { fn drop(&mut self) { unsafe { SetupDiDestroyDeviceInfoList(self.0); } } }
    struct Handle(*mut c_void);
    impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

    /// Enumerate only the dedicated diagnostic interface, not vendor devices.
    /// Paths are sensitive local identifiers: do not log or transmit them.
    pub fn interfaces() -> io::Result<Vec<OsString>> {
        // SAFETY: GUID and optional pointers satisfy SetupAPI requirements.
        let raw = unsafe { SetupDiGetClassDevsW(&INTERFACE, ptr::null(), ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) };
        if raw as isize == -1 { return Err(io::Error::last_os_error()); }
        let set = InfoSet(raw);
        let mut paths = Vec::new();
        for index in 0..u32::MAX {
            let mut data = InterfaceData { size: std::mem::size_of::<InterfaceData>() as u32, class: INTERFACE, flags: 0, reserved: 0 };
            // SAFETY: `data` is properly sized and `set` is a live handle.
            if unsafe { SetupDiEnumDeviceInterfaces(set.0, ptr::null(), &INTERFACE, index, &mut data) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS) { return Ok(paths); }
                return Err(error);
            }
            let mut required = 0u32;
            let result = unsafe { SetupDiGetDeviceInterfaceDetailW(set.0, &mut data, ptr::null_mut(), 0, &mut required, ptr::null_mut()) };
            if result == 0 && io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) { return Err(io::Error::last_os_error()); }
            if !(6..=65536).contains(&required) { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid interface path size")); }
            let capacity = required;
            let mut storage = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
            let base = storage.as_mut_ptr().cast::<u8>();
            // The SDK requires cbSize 8 on x64 (6 on x86), and the WCHAR path
            // begins at byte offset 4. Storage is aligned for the structure.
            unsafe { base.cast::<u32>().write(if cfg!(target_pointer_width = "64") { 8 } else { 6 }); }
            // SAFETY: `storage` holds at least `required` bytes and `data` is valid.
            if unsafe { SetupDiGetDeviceInterfaceDetailW(set.0, &mut data, base.cast(), capacity, &mut required, ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
            if required < 6 || required > capacity { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid returned interface path size")); }
            // SAFETY: the path is UTF-16-aligned and bounded by the buffer size.
            let path = unsafe { std::slice::from_raw_parts(base.add(4).cast::<u16>(), (required as usize - 4) / 2) };
            let len = path.iter().position(|&x| x == 0).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unterminated interface path"))?;
            paths.push(OsString::from_wide(&path[..len]));
        }
        Err(io::Error::other("too many diagnostic interfaces"))
    }

    /// Explicitly query one diagnostic interface. Does not write to hardware.
    pub fn query(path: &OsStr) -> io::Result<DiagnosticInfo> {
        DiagnosticInfo::decode(&request(path, IOCTL_GET_INFO, SIZE, GENERIC_READ)?)
    }

    /// Explicitly allocate and immediately free firmware-stage WDF buffers.
    /// No address is returned or programmed into the device.
    pub fn test_dma_allocation(path: &OsStr) -> io::Result<DmaDiagnostic> {
        DmaDiagnostic::decode(&request(path, IOCTL_DMA_TEST, DMA_SIZE, GENERIC_READ)?)
    }

    /// Inspect the driver's cached reset capability query; this never resets a device.
    pub fn reset_capabilities(path: &OsStr) -> io::Result<Option<ResetCapabilities>> {
        ResetCapabilities::decode(&request(path, IOCTL_RESET_INFO, RESET_SIZE, GENERIC_READ)?)
    }

    /// Explicitly switch device ownership and attempt to restore firmware
    /// ownership. This writes a hardware register, requires admin access and
    /// must only be run on an intentionally bound experimental device.
    pub fn test_ownership(path: &OsStr) -> io::Result<OwnershipDiagnostic> {
        OwnershipDiagnostic::decode(&request(path, IOCTL_OWN_TEST, OWN_SIZE, GENERIC_WRITE)?)
    }

    fn request(path: &OsStr, code: u32, size: usize, access: u32) -> io::Result<Vec<u8>> {
        let encoded: Vec<u16> = path.encode_wide().chain(std::iter::once(0)).collect();
        if encoded[..encoded.len()-1].contains(&0) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "interface path contains NUL")); }
        // SAFETY: path is NUL-terminated; unused optional pointers are null.
        let raw = unsafe { CreateFileW(encoded.as_ptr(), access, FILE_SHARE_READ, ptr::null_mut(), OPEN_EXISTING, 0, ptr::null_mut()) };
        if raw as isize == -1 { return Err(io::Error::last_os_error()); }
        let handle = Handle(raw);
        let mut bytes = vec![0u8; size];
        let mut returned = 0u32;
        // SAFETY: output points to a writable fixed-size buffer; operation is synchronous.
        if unsafe { DeviceIoControl(handle.0, code, ptr::null(), 0, bytes.as_mut_ptr().cast(), bytes.len() as u32, &mut returned, ptr::null_mut()) } == 0 { return Err(io::Error::last_os_error()); }
        if returned as usize != bytes.len() { return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected diagnostic response size")); }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response() -> Vec<u8> {
        [48u32, 1, 0x14c3, 0x0616, 0x105b, 0xe0cd, 0, 0, 1, 1, 0x100000, 1]
            .into_iter().flat_map(u32::to_le_bytes).collect()
    }
    #[test]
    fn accepts_matching_diagnostic_only_and_rejects_forged_status() {
        let mut bytes = response();
        let info = DiagnosticInfo::decode(&bytes).unwrap();
        assert_eq!(info.bar0_bytes, Some(0x100000));
        assert_eq!(info.device, 0x0616);
        bytes[28] = 1;
        assert!(DiagnosticInfo::decode(&bytes).is_err());
        assert!(DiagnosticInfo::decode(&bytes[..47]).is_err());
    }
    #[test]
    fn rejects_ambiguous_bar_and_unknown_device() {
        let mut bytes = response();
        bytes[44] = 2;
        assert!(DiagnosticInfo::decode(&bytes).is_err());
        let mut bytes = response();
        bytes[12..16].copy_from_slice(&0x7925u32.to_le_bytes());
        assert!(DiagnosticInfo::decode(&bytes).is_err());
    }
    #[test]
    fn dma_response_requires_expected_size_alignment_and_address_width() {
        let mut bytes: Vec<u8> = [20u32, 3, 28800, 32, 16].into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(DmaDiagnostic::decode(&bytes).unwrap().allocated_bytes, 28800);
        bytes[12] = 64;
        assert!(DmaDiagnostic::decode(&bytes).is_err());
        bytes[12] = 32;
        bytes[4] = 1; // Older single-buffer diagnostics must not masquerade as ring checks.
        assert!(DmaDiagnostic::decode(&bytes).is_err());
    }
    #[test]
    fn ownership_result_distinguishes_recovery_failure() {
        let mut bytes: Vec<u8> = [20u32, 2, 5, 0, 0].into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(OwnershipDiagnostic::decode(&bytes).unwrap().outcome, OwnershipOutcome::RecoveryFailed);
        bytes[8] = 1;
        assert!(OwnershipDiagnostic::decode(&bytes).is_err());
        bytes[12] = 1;
        bytes[16] = 5;
        let info = OwnershipDiagnostic::decode(&bytes).unwrap();
        assert_eq!(info.outcome, OwnershipOutcome::RoundTripSucceeded);
        assert_eq!(info.dma.unwrap(), DmaState { tx_enabled: true, tx_busy: false, rx_enabled: true, rx_busy: false });
        bytes[8] = 7;
        assert!(OwnershipDiagnostic::decode(&bytes).is_err());
        assert!(OwnershipDiagnostic::decode(&bytes[..19]).is_err());
    }
    #[test]
    fn reset_capability_unknown_is_not_interpreted_as_unsupported() {
        let mut bytes: Vec<u8> = [16u32, 1, 0, 0].into_iter().flat_map(u32::to_le_bytes).collect();
        assert_eq!(ResetCapabilities::decode(&bytes).unwrap(), None);
        bytes[8] = 1;
        bytes[12] = 1;
        assert_eq!(ResetCapabilities::decode(&bytes).unwrap(), Some(ResetCapabilities { function: true, platform: false }));
        bytes[8] = 0;
        assert!(ResetCapabilities::decode(&bytes).is_err());
        bytes[8] = 1;
        bytes[12] = 4;
        assert!(ResetCapabilities::decode(&bytes).is_err());
    }
}
