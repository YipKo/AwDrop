//! Offline AWDL integration fixture; this deliberately never accesses hardware.
use std::{collections::VecDeque, io, path::PathBuf, time::{Duration, Instant}};
use winawdl_adapter::{RadioAdapter, RadioInfo, PIPE_NAME, windows_pipe};

#[derive(Default)] struct Mock { frames: VecDeque<Vec<u8>>, echoed: bool }
impl RadioAdapter for Mock {
    type Error = io::Error;
    fn info(&self) -> RadioInfo { RadioInfo { channel: 6, width_mhz: 20 } }
    fn set_channel(&mut self, channel: u8) -> io::Result<()> {
        if channel == 6 { Ok(()) } else { Err(io::Error::other("mock supports channel 6 only")) }
    }
    fn set_bssid_filter(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
    fn set_address(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
    fn transmit(&mut self, frame: &[u8]) -> io::Result<()> {
        if !self.echoed && frame.len() >= 4 {
            let header = u16::from_le_bytes([frame[2], frame[3]]) as usize;
            if header + 16 <= frame.len() {
                let mut rx = vec![0, 0, 10, 0, 0x22, 0, 0, 0, 0, 0xd8];
                rx.extend_from_slice(&frame[header..]);
                rx[20..26].copy_from_slice(&[2, 0x11, 0x22, 0x33, 0x44, 0x55]);
                self.frames.push_back(rx);
                self.echoed = true;
            }
        }
        Ok(())
    }
    fn receive(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(self.frames.pop_front()) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ready = PathBuf::from(std::env::args_os().nth(1).ok_or("missing ready-file argument")?);
    let started = Instant::now();
    let mut ready_error = None;
    let mock = windows_pipe::serve_with_ready(Mock::default(), PIPE_NAME,
        || started.elapsed() >= Duration::from_secs(8),
        || { if let Err(error) = std::fs::write(ready, b"ready") { ready_error = Some(error); } })?;
    if let Some(error) = ready_error { return Err(error.into()); }
    if !mock.echoed { return Err("no simulated AWDL frame received".into()); }
    Ok(())
}
