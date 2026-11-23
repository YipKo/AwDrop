//! Read-only inventory; do not print instance locators or MAC addresses.
use awdrop_radio::{identify, inventory, registry::{builtins, candidates, AdapterBackend, DeviceId}};
fn main() -> std::io::Result<()> {
    let backends = builtins();
    let refs: Vec<&dyn AdapterBackend> = backends.iter().map(|backend| backend.as_ref()).collect();
    for device in inventory::known_devices()? {
        if let DeviceId::Pci { vendor, device: model } = device.id {
            println!("{:?} PCI {:04x}:{:04x}", identify(vendor, model), vendor, model);
            for (name, status) in candidates(&refs, device.id) { println!("  backend {name}: {status:?}"); }
        }
    }
    Ok(())
}
