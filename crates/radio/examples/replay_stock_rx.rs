//! Offline replay of WPP events previously exported from a trace. This does not
//! capture, transmit, open a device, or control Windows tracing. Input records
//! are (u16 message, u32 thread, u16 payload length, payload), all little endian.
use std::{env, fs::File, io::{self, BufReader, Read}};
use awdrop_radio::backends::mt7922::stock_rx::FrameAssembler;

fn main() -> io::Result<()> {
    let path = env::args_os().nth(1).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected an event file"))?;
    let mut input = BufReader::new(File::open(path)?);
    let mut assembler = FrameAssembler::new();
    let mut events = 0usize;
    let mut frames = 0usize;
    let mut awdl = 0usize;
    loop {
        let mut header = [0u8; 8];
        match input.read(&mut header[..1])? {
            0 => break,
            1 => input.read_exact(&mut header[1..])?,
            _ => unreachable!(),
        }
        let message = u16::from_le_bytes([header[0], header[1]]);
        let thread = u32::from_le_bytes(header[2..6].try_into().unwrap());
        let length = u16::from_le_bytes([header[6], header[7]]) as usize;
        let mut payload = vec![0u8; length];
        input.read_exact(&mut payload)?;
        events += 1;
        if let Some(frame) = assembler.ingest(message, thread, &payload)? {
            frames += 1;
            let rtlen = u16::from_le_bytes([frame[2], frame[3]]) as usize;
            if frame.get(rtlen + 24..rtlen + 29) == Some(&[0x7f, 0x00, 0x17, 0xf2, 0x08]) {
                awdl += 1;
            }
        }
    }
    assembler.finish()?;
    println!("events={events} frames={frames} awdl_signatures={awdl}");
    Ok(())
}
