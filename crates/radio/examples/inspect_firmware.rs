//! Read-only container inspection; no firmware is sent to a device.
use std::{collections::VecDeque, error::Error, io, path::PathBuf, time::Duration};
use awdrop_radio::backends::mt7922::{boot, download_plan, firmware::{patch_regions, ram_regions}, firmware_mcu::{FirmwareMcu, Queue}, pcie_transport::{DeviceMcuQueues, PcieTransport}};

/// Only validates the generated command flow. It does not open a device.
#[derive(Default)]
struct OfflineMcu { commands: usize, chunks: usize, drains: usize, state: u32, events: VecDeque<Vec<u8>> }
impl DeviceMcuQueues for OfflineMcu {
    fn submit(&mut self, queue: Queue, bytes: &[u8]) -> io::Result<()> {
        if queue == Queue::FirmwareDownload {
            if bytes.is_empty() || bytes.len() > 4096 { return Err(io::Error::other("invalid download chunk")); }
            self.chunks += 1;
            return Ok(());
        }
        if bytes.len() < 64 || bytes[39] == 0 || bytes[39] > 15 { return Err(io::Error::other("invalid command envelope")); }
        let seq = bytes[39];
        self.commands += 1;
        if bytes[36] == 0x04 {
            if bytes.get(64..68) != Some(&[1,0,0,0]) { return Err(io::Error::other("invalid MCU restart")); }
            self.state = 1;
            return Ok(());
        }
        let mut event = vec![0u8; 36];
        event[..4].copy_from_slice(&(7u32 << 27).to_le_bytes());
        event[29] = seq;
        if bytes[36] == 0x10 {
            event[32] = match bytes.get(64..68) {
                Some([1,0,0,0]) => 2,
                Some([0,0,0,0]) => 3,
                _ => return Err(io::Error::other("invalid semaphore command")),
            };
        }
        if bytes[36] == 0x02 { self.state = 3; }
        self.events.push_back(event);
        Ok(())
    }
    fn poll_event(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(self.events.pop_front()) }
    fn download_idle(&mut self) -> io::Result<bool> { self.drains += 1; Ok(true) }
    fn wait(&mut self, _: Duration) -> io::Result<()> { Err(io::Error::other("unexpected offline wait")) }
    fn quiesce_download(&mut self) -> io::Result<()> { Ok(()) }
    fn firmware_state(&mut self) -> io::Result<u32> { Ok(self.state) }
}
fn main() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(std::env::args_os().nth(1).ok_or("pass the firmware directory")?);
    let bundle = awdrop_radio::backends::mt7922::firmware::FirmwareBundle::from_directory(&dir)?;
    let patch = bundle.patch();
    let ram = bundle.ram();
    for (name, regions) in [("patch", patch_regions(patch)?), ("RAM", ram_regions(ram)?)] {
        println!("{name}: {} regions, {} bytes of payload", regions.len(), regions.iter().map(|r| r.data.len()).sum::<usize>());
    }
    println!("offline plans: {} patch steps, {} RAM steps", download_plan::patch(patch)?.len(), download_plan::ram(ram)?.len());
    let transport = PcieTransport::new(OfflineMcu::default(), Duration::from_secs(3))?;
    let mut mcu = FirmwareMcu::new(transport);
    boot::boot(&mut mcu, &bundle)?;
    let io = mcu.into_inner().into_inner();
    println!("simulated MCU: {} commands, {} chunks, {} queue drains", io.commands, io.chunks, io.drains);
    Ok(())
}
