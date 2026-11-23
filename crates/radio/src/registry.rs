//! Common, side-effect-free backend matching and type-erased session interface.
//!
//! `probe` reports only what a backend knows without claiming the device.
//! `open` may acquire hardware and must be called explicitly by its owner.
use crate::{RadioAdapter, RadioInfo};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceId {
    Pci { vendor: u16, device: u16 },
    Usb { vendor: u16, product: u16 },
}

/// A physical instance, not just its model. The platform inventory supplies
/// an opaque locator (for example a PCI instance ID or a Linux sysfs path).
/// Backends must treat it as untrusted and revalidate it when opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    pub locator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    NotMatched,
    /// The hardware matches, but the reason explains why it cannot be opened.
    Unavailable(&'static str),
    /// The backend can attempt to open it; success is not guaranteed.
    Candidate,
}

/// How an implementation expects to access a device, not a guarantee that
/// the current card or its installed driver supports the required features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Uses capabilities supplied by an existing driver (for example a filter).
    ExistingDriver,
    /// Requires the physical device to be handed to a different driver.
    Exclusive,
    /// The implementation has not declared its access requirements.
    Unknown,
}

#[derive(Debug)]
pub struct BackendError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}
impl BackendError {
    pub fn unavailable(message: impl Into<String>) -> Self { Self { message: message.into(), source: None } }
    fn operation(error: impl Error + Send + Sync + 'static) -> Self {
        Self { message: error.to_string(), source: Some(Box::new(error)) }
    }
}
impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.message) }
}
impl Error for BackendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> { self.source.as_deref().map(|e| e as _) }
}

/// Type-erased adapter so one session runner can use independent chip backends.
pub struct ErasedAdapter(Box<dyn RadioAdapter<Error = BackendError> + Send>);
struct Erase<A>(A);
impl<A: RadioAdapter> RadioAdapter for Erase<A> {
    type Error = BackendError;
    fn info(&self) -> RadioInfo { self.0.info() }
    fn set_channel(&mut self, channel: u8) -> Result<(), Self::Error> { self.0.set_channel(channel).map_err(BackendError::operation) }
    fn set_bssid_filter(&mut self, address: [u8; 6]) -> Result<(), Self::Error> { self.0.set_bssid_filter(address).map_err(BackendError::operation) }
    fn set_address(&mut self, address: [u8; 6]) -> Result<(), Self::Error> { self.0.set_address(address).map_err(BackendError::operation) }
    fn transmit(&mut self, frame: &[u8]) -> Result<(), Self::Error> { self.0.transmit(frame).map_err(BackendError::operation) }
    fn receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error> { self.0.receive().map_err(BackendError::operation) }
    fn shutdown(&mut self) -> Result<(), Self::Error> { self.0.shutdown().map_err(BackendError::operation) }
}
impl ErasedAdapter {
    pub fn new<A: RadioAdapter + Send + 'static>(adapter: A) -> Self { Self(Box::new(Erase(adapter))) }
}
impl RadioAdapter for ErasedAdapter {
    type Error = BackendError;
    fn info(&self) -> RadioInfo { self.0.info() }
    fn set_channel(&mut self, channel: u8) -> Result<(), Self::Error> { self.0.set_channel(channel) }
    fn set_bssid_filter(&mut self, address: [u8; 6]) -> Result<(), Self::Error> { self.0.set_bssid_filter(address) }
    fn set_address(&mut self, address: [u8; 6]) -> Result<(), Self::Error> { self.0.set_address(address) }
    fn transmit(&mut self, frame: &[u8]) -> Result<(), Self::Error> { self.0.transmit(frame) }
    fn receive(&mut self) -> Result<Option<Vec<u8>>, Self::Error> { self.0.receive() }
    fn shutdown(&mut self) -> Result<(), Self::Error> { self.0.shutdown() }
}

pub trait AdapterBackend: Send + Sync {
    /// Stable identifier; keep it unique among registered backends.
    fn id(&self) -> &'static str;
    /// Describes the backend's required access before any device is touched.
    fn access_mode(&self) -> AccessMode { AccessMode::Unknown }
    /// Must not claim, change or rebind hardware.
    fn probe(&self, device: DeviceId) -> Probe;
    /// Optional read-only checks for this particular instance and its current
    /// driver. A matching model alone must not be treated as verified support.
    fn probe_device(&self, device: &Device) -> Probe { self.probe(device.id) }
    /// Explicitly open one card; failure must not be reported as success.
    fn open(&self, device: &Device) -> Result<ErasedAdapter, BackendError>;
}

/// Built-ins only advertise matching status. No backend here claims a card.
pub fn builtins() -> Vec<Box<dyn AdapterBackend>> {
    let backends: Vec<Box<dyn AdapterBackend>> = vec![Box::new(crate::backends::mt7922::Mt7922)];
    #[cfg(windows)]
    {
        let mut backends = backends;
        backends.push(Box::new(crate::backends::mt7922::Mt7922Stock));
        backends
    }
    #[cfg(not(windows))]
    { backends }
}

/// Matches *all* candidates so overlapping backends never silently win based
/// on registration order. The caller must choose a backend explicitly.
pub fn candidates<'a>(backends: &'a [&'a dyn AdapterBackend], device: DeviceId) -> Vec<(&'a str, Probe)> {
    backends.iter().filter_map(|backend| match backend.probe(device) {
        Probe::NotMatched => None,
        result => Some((backend.id(), result)),
    }).collect()
}

/// Instance-aware matching for backends whose requirements depend on the
/// currently installed driver. This never opens or automatically selects one.
pub fn candidates_for_device<'a>(backends: &'a [&'a dyn AdapterBackend], device: &Device) -> Vec<(&'a str, Probe)> {
    backends.iter().filter_map(|backend| match backend.probe_device(device) {
        Probe::NotMatched => None,
        result => Some((backend.id(), result)),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backends::mt7922::Mt7922, bridge::Bridge};
    use std::io;

    struct Mock;
    impl RadioAdapter for Mock {
        type Error = io::Error;
        fn info(&self) -> RadioInfo { RadioInfo { channel: 6, width_mhz: 20 } }
        fn set_channel(&mut self, _: u8) -> io::Result<()> { Err(io::Error::other("test failure")) }
        fn set_bssid_filter(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn set_address(&mut self, _: [u8; 6]) -> io::Result<()> { Ok(()) }
        fn transmit(&mut self, _: &[u8]) -> io::Result<()> { Ok(()) }
        fn receive(&mut self) -> io::Result<Option<Vec<u8>>> { Ok(None) }
        fn shutdown(&mut self) -> io::Result<()> { Ok(()) }
    }
    #[test] fn erased_adapter_works_with_bridge_and_keeps_error_source() {
        let mut bridge = Bridge::new(ErasedAdapter::new(Mock));
        assert!(bridge.connect().is_ok());
        let err = bridge.ingest(&[4,0,0,0,1,0,0,0,6]).unwrap_err();
        match err { crate::bridge::BridgeError::Adapter(error) => {
            assert_eq!(error.to_string(), "test failure"); assert!(error.source().is_some());
        }, _ => panic!("expected adapter error") }
    }
    #[test] fn mt7922_matches_without_claiming_support() {
        let backend = Mt7922;
        assert_eq!(backend.access_mode(), if cfg!(windows) { AccessMode::Exclusive } else { AccessMode::Unknown });
        let device = DeviceId::Pci { vendor: 0x14c3, device: 0x0616 };
        assert!(matches!(backend.probe(device), Probe::Unavailable(_)));
        assert!(backend.open(&Device { id: device, locator: "test-instance".into() }).is_err());
        assert!(candidates(&[&backend], device).len() == 1);
        assert!(matches!(backend.probe(DeviceId::Pci { vendor: 0x0b48, device: 0x7922 }), Probe::Unavailable(_)));
        assert!(candidates(&[&backend], DeviceId::Usb { vendor: 0x14c3, product: 0x0616 }).is_empty());
    }
    #[cfg(windows)]
    #[test] fn stock_driver_research_is_visible_but_never_claims_a_working_adapter() {
        use crate::backends::mt7922::Mt7922Stock;
        let backend = Mt7922Stock;
        assert_eq!(backend.access_mode(), AccessMode::ExistingDriver);
        let id = DeviceId::Pci { vendor: 0x14c3, device: 0x0616 };
        assert!(matches!(backend.probe(id), Probe::Unavailable(_)));
        assert!(backend.open(&Device { id, locator: "unverified".into() }).is_err());
        assert_eq!(backend.probe(DeviceId::Pci { vendor: 0x14c3, device: 0x7925 }), Probe::NotMatched);
        assert!(builtins().iter().any(|candidate| candidate.id() == "mt7922-stock"));
    }
    #[test] fn overlapping_backends_are_all_visible_to_the_caller() {
        struct Candidate(&'static str);
        impl AdapterBackend for Candidate {
            fn id(&self) -> &'static str { self.0 }
            fn probe(&self, _: DeviceId) -> Probe { Probe::Candidate }
            fn open(&self, _: &Device) -> Result<ErasedAdapter, BackendError> { Ok(ErasedAdapter::new(Mock)) }
        }
        let a = Candidate("first");
        let b = Candidate("second");
        assert_eq!(a.access_mode(), AccessMode::Unknown);
        let id = DeviceId::Pci { vendor: 1, device: 2 };
        assert_eq!(candidates(&[&a, &b], id), [("first", Probe::Candidate), ("second", Probe::Candidate)]);
    }
    #[test] fn instance_probe_can_distinguish_two_identical_cards() {
        struct Shared;
        impl AdapterBackend for Shared {
            fn id(&self) -> &'static str { "shared" }
            fn access_mode(&self) -> AccessMode { AccessMode::ExistingDriver }
            fn probe(&self, _: DeviceId) -> Probe { Probe::Candidate }
            fn probe_device(&self, device: &Device) -> Probe {
                if device.locator == "supported" { Probe::Candidate } else { Probe::Unavailable("driver capability not verified") }
            }
            fn open(&self, _: &Device) -> Result<ErasedAdapter, BackendError> { Err(BackendError::unavailable("not implemented")) }
        }
        let id = DeviceId::Pci { vendor: 1, device: 2 };
        let backend = Shared;
        let a = Device { id, locator: "supported".into() };
        let b = Device { id, locator: "different-instance".into() };
        assert_eq!(candidates_for_device(&[&backend], &a), [("shared", Probe::Candidate)]);
        assert_eq!(candidates_for_device(&[&backend], &b), [("shared", Probe::Unavailable("driver capability not verified"))]);
    }
}
