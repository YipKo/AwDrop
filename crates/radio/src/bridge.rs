//! Transport-independent session state for a radio-link server.
//!
//! The caller owns transport scheduling and serializes access to this object.
//! In particular it must continue polling RX even when the client sends no
//! commands. No Windows driver or named pipe is opened by this module.

use crate::{apply, read_command, write_hello, write_rx, RadioAdapter, MAX_PAYLOAD};
use std::fmt;
use std::io::{self, Cursor};

#[derive(Debug)]
pub enum BridgeError<E> {
    Protocol(io::Error),
    Adapter(E),
    Disconnected,
}

impl<E: fmt::Display> fmt::Display for BridgeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(f, "radio-link protocol: {error}"),
            Self::Adapter(error) => write!(f, "radio adapter: {error}"),
            Self::Disconnected => write!(f, "no connected client"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for BridgeError<E> {}

/// Single-client state. `connect` and `disconnect` reset partial input, so
/// bytes from one client can never be interpreted as another client's command.
pub struct Bridge<A> {
    adapter: A,
    input: Vec<u8>,
    connected: bool,
}

impl<A: RadioAdapter> Bridge<A> {
    pub fn new(adapter: A) -> Self { Self { adapter, input: Vec::new(), connected: false } }

    pub fn connect(&mut self) -> Result<Vec<u8>, BridgeError<A::Error>> {
        self.input.clear();
        self.connected = true;
        let mut hello = Vec::new();
        write_hello(&mut hello, self.adapter.info()).map_err(BridgeError::Protocol)?;
        Ok(hello)
    }

    pub fn disconnect(&mut self) {
        self.connected = false;
        self.input.clear();
    }

    /// Accept arbitrarily fragmented or coalesced pipe reads, without ever
    /// buffering more than one maximum-sized message at a time.
    pub fn ingest(&mut self, mut bytes: &[u8]) -> Result<(), BridgeError<A::Error>> {
        if !self.connected { return Err(BridgeError::Disconnected); }
        while !bytes.is_empty() {
            let target = if self.input.len() < 8 {
                8
            } else {
                let length = u32::from_le_bytes(self.input[4..8].try_into().unwrap()) as usize;
                if length > MAX_PAYLOAD {
                    self.disconnect();
                    return Err(BridgeError::Protocol(io::Error::new(io::ErrorKind::InvalidData, "radio payload too large")));
                }
                8 + length
            };
            let take = (target - self.input.len()).min(bytes.len());
            self.input.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.input.len() < 8 { continue; }
            let length = u32::from_le_bytes(self.input[4..8].try_into().unwrap()) as usize;
            if length > MAX_PAYLOAD {
                self.disconnect();
                return Err(BridgeError::Protocol(io::Error::new(io::ErrorKind::InvalidData, "radio payload too large")));
            }
            if self.input.len() == 8 + length {
                let command = match read_command(Cursor::new(&self.input)) {
                    Ok(command) => command,
                    Err(error) => { self.disconnect(); return Err(BridgeError::Protocol(error)); }
                };
                self.input.clear();
                if let Err(error) = apply(&mut self.adapter, command) {
                    self.disconnect();
                    return Err(BridgeError::Adapter(error));
                }
            }
        }
        Ok(())
    }

    /// Poll one hardware RX frame. The caller writes the returned message as
    /// one serialized unit; concurrent writes must not interleave.
    pub fn poll_rx(&mut self) -> Result<Option<Vec<u8>>, BridgeError<A::Error>> {
        if !self.connected { return Err(BridgeError::Disconnected); }
        let frame = match self.adapter.receive() {
            Ok(frame) => frame,
            Err(error) => { self.disconnect(); return Err(BridgeError::Adapter(error)); }
        };
        let Some(frame) = frame else { return Ok(None) };
        let mut message = Vec::new();
        if let Err(error) = write_rx(&mut message, &frame) {
            self.disconnect();
            return Err(BridgeError::Protocol(error));
        }
        Ok(Some(message))
    }

    pub fn into_adapter(self) -> A { self.adapter }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RadioInfo, write_message};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Mock { channel: u8, sent: Vec<Vec<u8>>, rx: VecDeque<Vec<u8>> }
    impl RadioAdapter for Mock {
        type Error = io::Error;
        fn info(&self) -> RadioInfo { RadioInfo { channel: 6, width_mhz: 20 } }
        fn set_channel(&mut self, channel: u8) -> io::Result<()> {
            if channel == 0 { return Err(io::Error::other("rejected")); }
            self.channel = channel; Ok(())
        }
        fn set_bssid_filter(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn set_address(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn transmit(&mut self, frame: &[u8]) -> io::Result<()> { self.sent.push(frame.to_vec()); Ok(()) }
        fn receive(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(self.rx.pop_front()) }
        fn shutdown(&mut self) -> io::Result<()> { Ok(()) }
    }
    fn message(kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut message = Vec::new(); write_message(&mut message, kind, payload).unwrap(); message
    }

    #[test]
    fn fragmented_and_coalesced_commands_reach_adapter() {
        let mut bridge = Bridge::new(Mock::default());
        assert_eq!(bridge.connect().unwrap(), [1,0,0,0,8,0,0,0,3,0,0,0,6,20,0,0]);
        let mut bytes = message(4, &[44]); bytes.extend(message(3, &[0, 0, 8, 0]));
        for fragment in bytes.chunks(3) { bridge.ingest(fragment).unwrap(); }
        let adapter = bridge.into_adapter();
        assert_eq!(adapter.channel, 44);
        assert_eq!(adapter.sent, [vec![0, 0, 8, 0]]);
    }

    #[test]
    fn rx_can_flow_without_commands_and_disconnect_discards_partial_input() {
        let adapter = Mock { rx: VecDeque::from([vec![0, 0, 8, 0]]), ..Default::default() };
        let mut bridge = Bridge::new(adapter); bridge.connect().unwrap();
        assert_eq!(bridge.poll_rx().unwrap().unwrap(), message(2, &[0, 0, 8, 0]));
        assert!(bridge.poll_rx().unwrap().is_none());
        bridge.ingest(&message(4, &[6])[..5]).unwrap();
        bridge.disconnect();
        assert!(matches!(bridge.ingest(&[0]), Err(BridgeError::Disconnected)));
        bridge.connect().unwrap(); bridge.ingest(&message(4, &[44])).unwrap();
        assert_eq!(bridge.into_adapter().channel, 44);
    }

    #[test]
    fn malformed_or_rejected_commands_end_the_session() {
        let mut bridge = Bridge::new(Mock::default()); bridge.connect().unwrap();
        assert!(matches!(bridge.ingest(&[3,0,0,0,1,64,0,0]), Err(BridgeError::Protocol(_))));
        assert!(matches!(bridge.poll_rx(), Err(BridgeError::Disconnected)));
        bridge.connect().unwrap();
        assert!(matches!(bridge.ingest(&message(4, &[0])), Err(BridgeError::Adapter(_))));
        assert!(matches!(bridge.ingest(&message(4, &[6])), Err(BridgeError::Disconnected)));
    }

    #[test]
    fn malformed_backend_rx_disconnects_the_client() {
        let adapter = Mock { rx: VecDeque::from([vec![]]), ..Default::default() };
        let mut bridge = Bridge::new(adapter);
        bridge.connect().unwrap();
        assert!(matches!(bridge.poll_rx(), Err(BridgeError::Protocol(_))));
        assert!(matches!(bridge.poll_rx(), Err(BridgeError::Disconnected)));
    }
}
