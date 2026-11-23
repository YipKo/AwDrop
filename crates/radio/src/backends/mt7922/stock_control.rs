//! Encoding only for the single stock-driver sniffer setup observed on one
//! MT7922. This module performs no IOCTLs and validates no device or driver.
//! The caller must check the exact installed driver, device identity, wireless
//! port state, regulatory constraints, user consent, and lifecycle recovery.

/// Private IOCTL selector used in one experiment. Not a supported vendor API.
pub const SNIFFER_SELECTOR: u32 = 0xfffcf01c;
/// `IOCTL_NDIS_QUERY_GLOBAL_STATS` at the private device, not a guarantee that
/// the request itself is read-only: its payload can change hardware state.
pub const IOCTL_QUERY_GLOBAL_STATS: u32 = 0x00170002;
pub const REQUEST_LEN: usize = 28;

/// The only channel/width combination that has been tested on this machine.
#[derive(Debug, Clone, Copy)]
pub enum ObservedChannel { Ghz5Channel44Mhz20 }

/// Encode the observed start request. This alone does not validate hardware or
/// start anything. More channels must not be inferred from this experiment.
pub fn start(channel: ObservedChannel) -> [u8; REQUEST_LEN] {
    let mut request = [0u8; REQUEST_LEN];
    match channel {
        ObservedChannel::Ghz5Channel44Mhz20 => {
            request[5] = 1;
            request[8] = 44;
            request[12] = 0;
        }
    }
    request
}

/// Encode the observed stop request. A successful return from the driver does
/// not prove that the radio or the original Wi-Fi connection recovered.
pub fn stop() -> [u8; REQUEST_LEN] {
    let mut request = [0u8; REQUEST_LEN];
    request[0] = 1;
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_the_bounded_hardware_experiment_without_touching_a_device() {
        let mut expected = [0u8; REQUEST_LEN];
        expected[5] = 1;
        expected[8] = 44;
        assert_eq!(start(ObservedChannel::Ghz5Channel44Mhz20), expected);
        let mut stopped = [0u8; REQUEST_LEN];
        stopped[0] = 1;
        assert_eq!(stop(), stopped);
    }
}
