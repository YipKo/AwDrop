use awdrop_radio::{identify, inventory, registry::DeviceId, Chip};
use serde::Serialize;
use std::io::{self, Write};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    version: u32,
    transfers_available: bool,
    adapters: Vec<&'static str>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().skip(1).ne(["status"]) {
        return Err("usage: awdrop-desktop-bridge status".into());
    }
    let mut adapters: Vec<_> = inventory::known_devices()?
        .into_iter()
        .filter_map(|device| match device.id {
            DeviceId::Pci { vendor, device } => match identify(vendor, device) {
                Chip::Mt7922 => Some("MT7922"),
                Chip::Mt7925 => Some("MT7925"),
                Chip::Rtl8852ce => Some("RTL8852CE"),
                Chip::Unknown => None,
            },
            DeviceId::Usb { .. } => None,
        })
        .collect();
    adapters.sort_unstable();
    adapters.dedup();

    let mut output = io::stdout().lock();
    serde_json::to_writer(
        &mut output,
        &Status {
            version: 1,
            transfers_available: false,
            adapters,
        },
    )?;
    writeln!(output)?;
    Ok(())
}
