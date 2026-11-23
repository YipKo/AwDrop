//! Offline ETL replay; never starts tracing or accesses the wireless device.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use std::{env, io, path::PathBuf};
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected ETL path"))?;
    let fail_after_one = match args.next() {
        None => false,
        Some(option) if option == "--verify-error" => true,
        Some(_) => return Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown option")),
    };
    if args.next().is_some() { return Err(io::Error::new(io::ErrorKind::InvalidInput, "too many arguments")); }
    let mut awdl = 0usize;
    let result = awdrop_radio::backends::mt7922::stock_etl::replay(&path, &mut |frame| {
        if fail_after_one { return Err(io::Error::other("deliberate callback error")); }
        let rtlen = u16::from_le_bytes([frame[2], frame[3]]) as usize;
        if frame.get(rtlen + 24..rtlen + 29) == Some(&[0x7f, 0x00, 0x17, 0xf2, 0x08]) { awdl += 1; }
        Ok(())
    });
    if fail_after_one {
        return match result {
            Err(error) if error.to_string() == "deliberate callback error" => {
                println!("callback error propagated"); Ok(())
            }
            _ => Err(io::Error::other("expected callback error was not returned")),
        };
    }
    let count = result?;
    println!("frames={count} awdl_signatures={awdl}");
    Ok(())
}

#[cfg(not(windows))]
fn main() { eprintln!("ETL replay requires Windows."); }
