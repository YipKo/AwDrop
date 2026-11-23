//! Read-only Windows WLAN device-service enumeration. No service commands,
//! scans, profile reads or settings changes are performed.
#[cfg(not(windows))]
fn main() { eprintln!("This diagnostic is available on Windows only."); }

#[cfg(windows)]
fn main() -> std::io::Result<()> { windows::run() }

#[cfg(windows)]
mod windows {
    use std::{ffi::{c_char, c_void}, io, mem, ptr};
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid { a: u32, b: u16, c: u16, d: [u8; 8] }
    impl std::fmt::Display for Guid {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-", self.a, self.b, self.c, self.d[0], self.d[1])?;
            for byte in &self.d[2..] { write!(f, "{byte:02x}")?; }
            Ok(())
        }
    }
    #[repr(C)]
    struct Interface { guid: Guid, description: [u16; 256], state: u32 }
    #[link(name = "wlanapi")]
    extern "system" {
        fn WlanOpenHandle(version: u32, reserved: *mut c_void, negotiated: *mut u32, handle: *mut Handle) -> u32;
        fn WlanCloseHandle(handle: Handle, reserved: *mut c_void) -> u32;
        fn WlanEnumInterfaces(handle: Handle, reserved: *mut c_void, list: *mut *mut u8) -> u32;
        fn WlanFreeMemory(buffer: *mut c_void);
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> Handle;
        fn GetProcAddress(module: Handle, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(module: Handle) -> i32;
    }
    type GetServices = unsafe extern "system" fn(Handle, *const Guid, *mut *mut u8) -> u32;
    struct Client(Handle);
    impl Drop for Client { fn drop(&mut self) { unsafe { WlanCloseHandle(self.0, ptr::null_mut()); } } }
    struct Buffer(*mut u8);
    impl Drop for Buffer { fn drop(&mut self) { if !self.0.is_null() { unsafe { WlanFreeMemory(self.0.cast()); } } } }
    struct Library(Handle);
    impl Drop for Library { fn drop(&mut self) { unsafe { FreeLibrary(self.0); } } }
    fn status(code: u32) -> io::Error { io::Error::from_raw_os_error(code as i32) }

    pub fn run() -> io::Result<()> {
        unsafe {
            let mut handle = ptr::null_mut();
            let mut version = 0;
            let result = WlanOpenHandle(2, ptr::null_mut(), &mut version, &mut handle);
            if result != 0 { return Err(status(result)); }
            let client = Client(handle);
            let dll: Vec<u16> = "wlanapi.dll\0".encode_utf16().collect();
            let module = LoadLibraryW(dll.as_ptr());
            if module.is_null() { return Err(io::Error::last_os_error()); }
            let library = Library(module);
            let proc = GetProcAddress(library.0, b"WlanGetSupportedDeviceServices\0".as_ptr().cast());
            if proc.is_null() { return Err(io::Error::new(io::ErrorKind::Unsupported, "WLAN device-service query is unavailable on this Windows version")); }
            let get_services: GetServices = mem::transmute(proc);
            let mut list = Buffer(ptr::null_mut());
            let result = WlanEnumInterfaces(client.0, ptr::null_mut(), &mut list.0);
            if result != 0 { return Err(status(result)); }
            if list.0.is_null() { return Err(io::Error::new(io::ErrorKind::InvalidData, "missing WLAN interface list")); }
            let count = ptr::read_unaligned(list.0.cast::<u32>());
            if count > 256 { return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid WLAN interface count")); }
            println!("WLAN interfaces: {count}; read-only device-service enumeration");
            let mut query_failed = false;
            for i in 0..count as usize {
                let item = &*list.0.add(8 + i * mem::size_of::<Interface>()).cast::<Interface>();
                let end = item.description.iter().position(|&v| v == 0).unwrap_or(item.description.len());
                println!("  {}", String::from_utf16_lossy(&item.description[..end]));
                let mut services = Buffer(ptr::null_mut());
                let result = get_services(client.0, &item.guid, &mut services.0);
                if result != 0 {
                    // ASCII names remain readable even when a Windows console
                    // or PowerShell redirect uses a non-UTF-8 code page.
                    let name = match result { 5 => "ERROR_ACCESS_DENIED", 50 => "ERROR_NOT_SUPPORTED", 232 => "ERROR_NO_DATA", _ => "WIN32_ERROR" };
                    println!("    query failed: {result} ({name})");
                    query_failed = true;
                    continue;
                }
                if services.0.is_null() { println!("    invalid null service list"); query_failed = true; continue; }
                let n = ptr::read_unaligned(services.0.cast::<u32>());
                if n > 256 { println!("    invalid service count: {n}"); query_failed = true; continue; }
                println!("    device services: {n}");
                for j in 0..n as usize {
                    let guid = ptr::read_unaligned(services.0.add(8 + j * mem::size_of::<Guid>()).cast::<Guid>());
                    println!("      {guid}");
                }
            }
            if query_failed { return Err(io::Error::other("one or more device-service queries failed; capability remains unknown")); }
        }
        Ok(())
    }
}
