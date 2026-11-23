//! MT7922 / Connac2 offline building blocks. This is not a working driver.
pub mod dma;
pub mod firmware;
pub mod mcu;
pub mod rx;
pub mod tx;
pub mod monitor;
pub mod download_plan;
pub mod download;
pub mod firmware_mcu;
pub mod registers;
pub mod pcie_transport;
pub mod boot;
pub mod stock_rx;
pub mod stock_control;
#[cfg(windows)]
pub mod stock_etl;

use crate::registry::{AccessMode, AdapterBackend, BackendError, Device, DeviceId, ErasedAdapter, Probe};

/// A discoverable but deliberately non-operational backend. Identifying a
/// matching PCI ID never claims that the card has been opened or is usable.
pub struct Mt7922;

impl AdapterBackend for Mt7922 {
    fn id(&self) -> &'static str { "mt7922" }
    fn access_mode(&self) -> AccessMode {
        if cfg!(windows) { AccessMode::Exclusive } else { AccessMode::Unknown }
    }
    fn probe(&self, device: DeviceId) -> Probe {
        match device {
            DeviceId::Pci { vendor, device } if crate::identify(vendor, device) == crate::Chip::Mt7922 =>
                Probe::Unavailable("MT7922 hardware driver is not implemented"),
            _ => Probe::NotMatched,
        }
    }
    fn open(&self, _device: &Device) -> Result<ErasedAdapter, BackendError> {
        Err(BackendError::unavailable("MT7922 hardware driver is not implemented"))
    }
}

/// Research path for the existing Windows vendor driver. One bounded RX
/// experiment worked, but this is not a usable AirDrop adapter yet: live
/// capture lifecycle, channel management, raw TX and recovery are incomplete.
#[cfg(windows)]
pub struct Mt7922Stock;

#[cfg(windows)]
impl AdapterBackend for Mt7922Stock {
    fn id(&self) -> &'static str { "mt7922-stock" }
    fn access_mode(&self) -> AccessMode { AccessMode::ExistingDriver }
    fn probe(&self, device: DeviceId) -> Probe {
        match device {
            DeviceId::Pci { vendor: 0x14c3, device: 0x0616 } =>
                Probe::Unavailable("stock-driver RX experiment only; live session and raw TX are not implemented"),
            _ => Probe::NotMatched,
        }
    }
    fn open(&self, _device: &Device) -> Result<ErasedAdapter, BackendError> {
        Err(BackendError::unavailable("stock-driver live session and raw TX are not implemented"))
    }
}
