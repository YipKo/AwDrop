//! Read-only container inspection; no firmware is sent to a device.
use std::{error::Error, fs, path::PathBuf};
use winawdl_adapter::mt7922_firmware::{patch_regions, ram_regions};
fn main() -> Result<(), Box<dyn Error>> {
    let dir = PathBuf::from(std::env::args_os().nth(1).ok_or("pass the firmware directory")?);
    let patch = fs::read(dir.join("WIFI_MT7922_patch_mcu_1_1_hdr.bin"))?;
    let ram = fs::read(dir.join("WIFI_RAM_CODE_MT7922_1.bin"))?;
    for (name, regions) in [("patch", patch_regions(&patch)?), ("RAM", ram_regions(&ram)?)] {
        println!("{name}: {} regions, {} bytes of payload", regions.len(), regions.iter().map(|r| r.data.len()).sum::<usize>());
    }
    Ok(())
}
