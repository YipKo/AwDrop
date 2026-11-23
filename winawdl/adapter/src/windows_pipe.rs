//! A single-client Windows radio-link server for `Bridge`.
//! It does not open a device: the caller supplies a constructed adapter.
//! A stopped or disconnected session is closed before another is accepted.

use crate::{bridge::{Bridge, BridgeError}, RadioAdapter};
use std::{ffi::c_void, io, os::windows::ffi::OsStrExt, path::Path, ptr, thread, time::Duration};

type Handle = *mut c_void;
const INVALID_HANDLE: Handle = -1isize as Handle;
const PIPE_ACCESS_DUPLEX: u32 = 3;
const FIRST_INSTANCE: u32 = 0x0008_0000;
const PIPE_NOWAIT: u32 = 1;
const REJECT_REMOTE_CLIENTS: u32 = 8;
const ERROR_PIPE_LISTENING: i32 = 536;
const ERROR_PIPE_CONNECTED: i32 = 535;
const ERROR_NO_DATA: i32 = 232;
const ERROR_BROKEN_PIPE: i32 = 109;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(name: *const u16, open_mode: u32, pipe_mode: u32,
        max_instances: u32, out_size: u32, in_size: u32, default_timeout: u32,
        security: *mut c_void) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlapped: *mut c_void) -> i32;
    fn DisconnectNamedPipe(pipe: Handle) -> i32;
    fn PeekNamedPipe(pipe: Handle, buffer: *mut c_void, length: u32,
        read: *mut u32, available: *mut u32, left: *mut u32) -> i32;
    fn ReadFile(file: Handle, buffer: *mut c_void, length: u32,
        read: *mut u32, overlapped: *mut c_void) -> i32;
    fn WriteFile(file: Handle, buffer: *const c_void, length: u32,
        written: *mut u32, overlapped: *mut c_void) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

struct Pipe(Handle);
impl Drop for Pipe { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

fn lost(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(ERROR_NO_DATA | ERROR_BROKEN_PIPE))
}

impl Pipe {
    fn create(name: &str) -> io::Result<Self> {
        if !name.starts_with(r"\\.\pipe\") || name.contains('\0') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid local named-pipe path"));
        }
        let wide: Vec<u16> = Path::new(name).as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe { CreateNamedPipeW(wide.as_ptr(), PIPE_ACCESS_DUPLEX | FIRST_INSTANCE,
            PIPE_NOWAIT | REJECT_REMOTE_CLIENTS, 1, 65536, 65536, 0, ptr::null_mut()) };
        if handle == INVALID_HANDLE { Err(io::Error::last_os_error()) } else { Ok(Self(handle)) }
    }

    fn accept(&self) -> io::Result<bool> {
        // In PIPE_NOWAIT mode a successful call only starts listening; the
        // client is connected when a subsequent call returns PIPE_CONNECTED.
        if unsafe { ConnectNamedPipe(self.0, ptr::null_mut()) } != 0 { return Ok(false); }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(ERROR_PIPE_CONNECTED) => Ok(true),
            Some(ERROR_PIPE_LISTENING | ERROR_NO_DATA) => Ok(false),
            _ => Err(error),
        }
    }

    fn available(&self) -> io::Result<u32> {
        let mut available = 0;
        let ok = unsafe { PeekNamedPipe(self.0, ptr::null_mut(), 0, ptr::null_mut(), &mut available, ptr::null_mut()) };
        if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(available) }
    }

    fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut read = 0;
        let ok = unsafe { ReadFile(self.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read, ptr::null_mut()) };
        if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(read as usize) }
    }

    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut written = 0;
        let ok = unsafe { WriteFile(self.0, bytes.as_ptr().cast(), bytes.len() as u32, &mut written, ptr::null_mut()) };
        if ok == 0 { return Err(io::Error::last_os_error()); }
        if written as usize != bytes.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "pipe did not accept a complete message"));
        }
        Ok(())
    }

    fn disconnect(&self) { unsafe { DisconnectNamedPipe(self.0); } }
}

/// Polls a byte-mode local pipe until `stop` returns true. A client that does
/// not drain RX may be disconnected on backpressure, never fed a partial next
/// message. Adapter errors terminate the server for explicit recovery by its
/// owner. The adapter must return promptly from each trait method.
pub fn serve<A: RadioAdapter>(adapter: A, name: &str, mut stop: impl FnMut() -> bool)
    -> Result<A, BridgeError<A::Error>> {
    serve_with_ready(adapter, name, &mut stop, || {})
}

/// As `serve`, with a callback once the exclusive pipe is created.
pub fn serve_with_ready<A: RadioAdapter>(adapter: A, name: &str, mut stop: impl FnMut() -> bool,
    ready: impl FnOnce()) -> Result<A, BridgeError<A::Error>> {
    let pipe = Pipe::create(name).map_err(BridgeError::Protocol)?;
    ready();
    let mut bridge = Bridge::new(adapter);
    let mut buffer = [0u8; 4096];
    while !stop() {
        if !pipe.accept().map_err(BridgeError::Protocol)? {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let hello = bridge.connect()?;
        if pipe.write(&hello).is_err() { bridge.disconnect(); pipe.disconnect(); continue; }
        while !stop() {
            match pipe.available() {
                Ok(available) if available > 0 => {
                    let amount = (available as usize).min(buffer.len());
                    match pipe.read(&mut buffer[..amount]) {
                        Ok(0) => break,
                        Ok(read) => bridge.ingest(&buffer[..read])?,
                        Err(error) if lost(&error) => break,
                        Err(error) => return Err(BridgeError::Protocol(error)),
                    }
                }
                Ok(_) => {},
                Err(error) if lost(&error) => break,
                Err(error) => return Err(BridgeError::Protocol(error)),
            }
            if let Some(message) = bridge.poll_rx()? {
                if pipe.write(&message).is_err() { break; }
            }
            thread::sleep(Duration::from_millis(1));
        }
        bridge.disconnect();
        pipe.disconnect();
    }
    Ok(bridge.into_adapter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RadioInfo;
    use std::{fs::OpenOptions, io::{Read, Write}, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc}, time::Instant};

    #[derive(Default)] struct Mock { channel: u8 }
    impl RadioAdapter for Mock {
        type Error = io::Error;
        fn info(&self) -> RadioInfo { RadioInfo { channel: 6, width_mhz: 20 } }
        fn set_channel(&mut self, value: u8) -> io::Result<()> { self.channel = value; Ok(()) }
        fn set_bssid_filter(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn set_address(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn transmit(&mut self, _: &[u8]) -> io::Result<()> { Ok(()) }
        fn receive(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(None) }
    }

    #[test] fn local_pipe_exchanges_hello_and_command() {
        let name = format!(r"\\.\pipe\winawdl-adapter-test-{}", std::process::id());
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server_name = name.clone();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let result = serve(Mock::default(), &server_name, || server_stop.load(Ordering::Relaxed));
            let _ = result_tx.send(result.map(|adapter| adapter.channel));
        });
        let (client_tx, client_rx) = mpsc::channel();
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut pipe = loop {
                match OpenOptions::new().read(true).write(true).open(&name) {
                    Ok(pipe) => break pipe,
                    Err(error) if Instant::now() < deadline => { let _ = error; thread::sleep(Duration::from_millis(10)); }
                    Err(error) => { let _ = client_tx.send(Err(error)); return; }
                }
            };
            let mut hello = [0u8; 16];
            let result = pipe.read_exact(&mut hello).and_then(|_| {
                if hello != [1,0,0,0,8,0,0,0,3,0,0,0,6,20,0,0] {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "bad hello"));
                }
                pipe.write_all(&[4,0,0,0,1,0,0,0,44])
            });
            thread::sleep(Duration::from_millis(30));
            let _ = client_tx.send(result);
        });
        let client = client_rx.recv_timeout(Duration::from_secs(5));
        stop.store(true, Ordering::Relaxed);
        assert!(client.expect("client timed out").is_ok());
        assert_eq!(result_rx.recv_timeout(Duration::from_secs(3)).expect("server timed out").unwrap(), 44);
    }

    #[test] fn a_new_client_can_connect_after_disconnect() {
        let name = format!(r"\\.\pipe\winawdl-adapter-reconnect-{}", std::process::id());
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone(); let server_name = name.clone();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let result = serve(Mock::default(), &server_name, || server_stop.load(Ordering::Relaxed));
            let _ = result_tx.send(result.map(|adapter| adapter.channel));
        });
        let (client_tx, client_rx) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| -> io::Result<()> {
                for channel in [6u8, 44] {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    let mut pipe = loop {
                        match OpenOptions::new().read(true).write(true).open(&name) {
                            Ok(pipe) => break pipe,
                            Err(error) if Instant::now() < deadline => { let _ = error; thread::sleep(Duration::from_millis(10)); }
                            Err(error) => return Err(error),
                        }
                    };
                    let mut hello = [0u8; 16]; pipe.read_exact(&mut hello)?;
                    if hello[0] != 1 { return Err(io::Error::new(io::ErrorKind::InvalidData, "bad hello")); }
                    pipe.write_all(&[4,0,0,0,1,0,0,0,channel])?;
                    thread::sleep(Duration::from_millis(30));
                    drop(pipe);
                    thread::sleep(Duration::from_millis(30));
                }
                Ok(())
            })();
            let _ = client_tx.send(result);
        });
        let client = client_rx.recv_timeout(Duration::from_secs(7));
        stop.store(true, Ordering::Relaxed);
        assert!(client.expect("clients timed out").is_ok());
        assert_eq!(result_rx.recv_timeout(Duration::from_secs(3)).expect("server timed out").unwrap(), 44);
    }
}
