//! Adapter contract and version 3 wire format used by winawdl's AWDL daemon.
//! Implementations must provide actual hardware access; this crate does not
//! install drivers or claim that recognizing a device means it is supported.

use std::io::{self, Read, Write};

pub mod bridge;
pub mod backends;
pub mod registry;
pub mod inventory;
pub mod diagnostic;
// Preserve existing consumers while new code uses backends::mt7922.
pub use backends::mt7922::{dma as mt7922_dma, firmware as mt7922_firmware,
                           mcu as mt7922_mcu, rx as mt7922_rx};
#[cfg(windows)]
pub mod windows_pipe;

pub const VERSION: u32 = 3;
pub const MAX_PAYLOAD: usize = 16_384;
pub const PIPE_NAME: &str = r"\\.\pipe\winawdl-radio";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip { Rtl8852ce, Mt7922, Mt7925, Unknown }

/// PCI identity only, not a test of driver readiness or runtime support.
pub fn identify(vendor: u16, device: u16) -> Chip {
    match (vendor, device) {
        (0x10ec, 0xc852) => Chip::Rtl8852ce,
        (0x14c3, 0x0616 | 0x7922) | (0x0b48, 0x7922) => Chip::Mt7922,
        (0x14c3, 0x7925) => Chip::Mt7925,
        _ => Chip::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioInfo { pub channel: u8, pub width_mhz: u8 }

/// A backend owns its session resources; it may either share the existing
/// driver or require exclusive card access. Frame bytes include radiotap;
/// TX frames do not include an FCS. Calls must fail on unsupported operations
/// rather than pretend they succeeded. The implementer handles regulatory
/// restrictions, frame filtering, acknowledgement and shutdown. A failed
/// shutdown must retain DMA memory until the platform can safely release it.
pub trait RadioAdapter {
    type Error: std::error::Error + Send + Sync + 'static;
    fn info(&self) -> RadioInfo;
    fn set_channel(&mut self, channel: u8) -> Result<(), Self::Error>;
    fn set_bssid_filter(&mut self, address: [u8; 6]) -> Result<(), Self::Error>;
    fn set_address(&mut self, address: [u8; 6]) -> Result<(), Self::Error>;
    fn transmit(&mut self, radiotap_frame: &[u8]) -> Result<(), Self::Error>;
    /// Returns the next received radiotap frame, or None if no frame is ready.
    fn receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;
    /// Stop the session and restore resources, reporting any failure. Must be
    /// idempotent; Drop must still perform best-effort cleanup.
    fn shutdown(&mut self) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command { Transmit(Vec<u8>), SetChannel(u8), SetBssidFilter([u8; 6]), SetAddress([u8; 6]) }

fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

/// Reads exactly one client message, rejecting oversized and malformed input.
pub fn read_command(mut input: impl Read) -> io::Result<Command> {
    let mut header = [0u8; 8];
    input.read_exact(&mut header)?;
    let kind = u16::from_le_bytes([header[0], header[1]]);
    let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if length > MAX_PAYLOAD { return Err(invalid("radio payload too large")); }
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    match (kind, payload.as_slice()) {
        (3, frame) if !frame.is_empty() => Ok(Command::Transmit(frame.to_vec())),
        (4, [channel]) => Ok(Command::SetChannel(*channel)),
        (5, bytes) if bytes.len() == 6 => Ok(Command::SetBssidFilter(bytes.try_into().unwrap())),
        (6, bytes) if bytes.len() == 6 => Ok(Command::SetAddress(bytes.try_into().unwrap())),
        _ => Err(invalid("unknown or malformed radio command")),
    }
}

pub fn apply<A: RadioAdapter>(adapter: &mut A, command: Command) -> Result<(), A::Error> {
    match command {
        Command::Transmit(frame) => adapter.transmit(&frame),
        Command::SetChannel(channel) => adapter.set_channel(channel),
        Command::SetBssidFilter(address) => adapter.set_bssid_filter(address),
        Command::SetAddress(address) => adapter.set_address(address),
    }
}

fn write_message(mut output: impl Write, kind: u16, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_PAYLOAD { return Err(invalid("radio payload too large")); }
    output.write_all(&kind.to_le_bytes())?;
    output.write_all(&0u16.to_le_bytes())?;
    output.write_all(&(payload.len() as u32).to_le_bytes())?;
    output.write_all(payload)
}

pub fn write_hello(output: impl Write, info: RadioInfo) -> io::Result<()> {
    let mut payload = [0u8; 8];
    payload[..4].copy_from_slice(&VERSION.to_le_bytes());
    payload[4] = info.channel;
    payload[5] = info.width_mhz;
    write_message(output, 1, &payload)
}

pub fn write_rx(output: impl Write, frame: &[u8]) -> io::Result<()> {
    if frame.is_empty() { return Err(invalid("empty receive frame")); }
    write_message(output, 2, frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new(); write_message(&mut bytes, kind, payload).unwrap(); bytes
    }
    #[test] fn pci_identity_is_not_a_support_claim() {
        assert_eq!(identify(0x14c3, 0x0616), Chip::Mt7922);
        assert_eq!(identify(0x14c3, 0x7922), Chip::Mt7922);
        assert_eq!(identify(0x0b48, 0x7922), Chip::Mt7922);
        assert_eq!(identify(0x14c3, 0x7925), Chip::Mt7925);
        assert_eq!(identify(0x10ec, 0xc852), Chip::Rtl8852ce);
        assert_eq!(identify(0x14c3, 0xc852), Chip::Unknown);
    }
    #[test] fn hello_matches_c_wire_format() {
        let mut bytes = vec![]; write_hello(&mut bytes, RadioInfo { channel: 44, width_mhz: 80 }).unwrap();
        assert_eq!(bytes, [1,0,0,0,8,0,0,0,3,0,0,0,44,80,0,0]);
    }
    #[test] fn commands_decode_and_reject_bad_lengths() {
        assert_eq!(read_command(message(4, &[6]).as_slice()).unwrap(), Command::SetChannel(6));
        assert_eq!(read_command(message(5, &[1,2,3,4,5,6]).as_slice()).unwrap(), Command::SetBssidFilter([1,2,3,4,5,6]));
        assert_eq!(read_command(message(3, &[1,2]).as_slice()).unwrap(), Command::Transmit(vec![1,2]));
        assert!(read_command(message(6, &[1,2]).as_slice()).is_err());
        assert!(read_command(message(99, &[]).as_slice()).is_err());
        assert!(read_command([3,0,0,0,1,64,0,0].as_slice()).is_err());
        assert!(read_command([3,0,0,0,1,0,0,0].as_slice()).is_err());
    }
    #[test] fn rx_is_bounded_and_encoded() {
        let mut bytes = vec![]; write_rx(&mut bytes, &[0,1,2]).unwrap();
        assert_eq!(bytes, [2,0,0,0,3,0,0,0,0,1,2]);
        assert!(write_rx(&mut bytes, &[]).is_err());
        assert!(write_rx(&mut bytes, &vec![0; MAX_PAYLOAD + 1]).is_err());
    }
    #[test] fn commands_reach_the_adapter_and_propagate_failures() {
        #[derive(Default)] struct Mock { calls: Vec<String> }
        impl RadioAdapter for Mock {
            type Error = io::Error;
            fn info(&self) -> RadioInfo { RadioInfo { channel: 6, width_mhz: 20 } }
            fn set_channel(&mut self, c: u8) -> io::Result<()> {
                if c == 0 { return Err(invalid("invalid channel")); }
                self.calls.push(format!("channel {c}")); Ok(())
            }
            fn set_bssid_filter(&mut self, a: [u8; 6]) -> io::Result<()> { self.calls.push(format!("bssid {a:?}")); Ok(()) }
            fn set_address(&mut self, a: [u8; 6]) -> io::Result<()> { self.calls.push(format!("address {a:?}")); Ok(()) }
            fn transmit(&mut self, frame: &[u8]) -> io::Result<()> { self.calls.push(format!("tx {frame:?}")); Ok(()) }
            fn receive(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(None) }
            fn shutdown(&mut self) -> io::Result<()> { Ok(()) }
        }
        let mut adapter = Mock::default();
        for (kind, data) in [(4, vec![6]), (5, vec![1; 6]), (6, vec![2; 6]), (3, vec![3, 4])] {
            apply(&mut adapter, read_command(message(kind, &data).as_slice()).unwrap()).unwrap();
        }
        assert_eq!(adapter.calls, ["channel 6", "bssid [1, 1, 1, 1, 1, 1]", "address [2, 2, 2, 2, 2, 2]", "tx [3, 4]"]);
        assert!(apply(&mut adapter, Command::SetChannel(0)).is_err());
        assert_eq!(adapter.calls.len(), 4);
    }
}
