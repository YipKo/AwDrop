//! Diagnostic interface enumeration. Active checks run only when explicitly requested.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    let paths = awdrop_radio::diagnostic::windows::interfaces()?;
    println!("Installed MT7922 diagnostic interfaces: {}", paths.len());
    for path in paths {
        let info = awdrop_radio::diagnostic::windows::query(&path)?;
        println!("MT7922 diagnostic: {} memory resources, {} interrupts, BAR0 matched: {}", info.memory_resources, info.interrupt_resources, info.bar0_bytes.is_some());
        println!("Reset capability query (no reset requested): {:?}", awdrop_radio::diagnostic::windows::reset_capabilities(&path)?);
        if std::env::args().any(|arg| arg == "--dma-test") {
            let dma = awdrop_radio::diagnostic::windows::test_dma_allocation(&path)?;
            println!("WDF ring buffers allocated and released: {} bytes, {}-bit addresses, {}-byte alignment", dma.allocated_bytes, dma.address_bits, dma.alignment);
        }
        if std::env::args().any(|arg| arg == "--ownership-test") {
            let outcome = awdrop_radio::diagnostic::windows::test_ownership(&path)?;
            println!("Ownership round-trip result: {outcome:?}");
            if outcome.outcome != awdrop_radio::diagnostic::OwnershipOutcome::RoundTripSucceeded {
                return Err(std::io::Error::other(format!("ownership test did not succeed: {outcome:?}")));
            }
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() { eprintln!("Windows only"); }
