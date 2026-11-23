//! Windows ETW consumption for the experimentally observed stock-driver RX.
//! This module does not start a tracing session or access the wireless device.
//! Live consumption is not hardware-validated; device control, TX, loss
//! monitoring, channel management and recovery remain unfinished work.

use std::{ffi::{c_void, OsStr}, io, os::windows::ffi::OsStrExt, panic::{catch_unwind, AssertUnwindSafe}, path::Path, ptr, slice, sync::Mutex};
use windows_sys::Win32::System::Diagnostics::Etw::{
    CloseTrace, OpenTraceW, ProcessTrace, EVENT_RECORD, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_LOGFILEW_0, EVENT_TRACE_LOGFILEW_1, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
};

use super::stock_rx::FrameAssembler;

struct Context<'a> {
    assembler: FrameAssembler,
    callback: &'a mut (dyn FnMut(Vec<u8>) -> io::Result<()> + Send),
    error: Option<io::Error>,
    frames: usize,
}

fn stock_guid(record: &EVENT_RECORD) -> bool {
    let guid = &record.EventHeader.ProviderId;
    guid.data1 == 0x91f19637 && guid.data2 == 0x6cdd && guid.data3 == 0x38a0 &&
        guid.data4 == [0x0e, 0xfc, 0x7c, 0xdf, 0x9a, 0x3a, 0x31, 0x6e]
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    if record.is_null() { return; }
    // ProcessTrace supplies our Context for the lifetime of the processing call.
    let record = unsafe { &*record };
    if record.UserContext.is_null() || !stock_guid(record) { return; }
    let state = unsafe { &*record.UserContext.cast::<Mutex<Context<'_>>>() };
    let mut context = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if context.error.is_some() { return; }
    let outcome = catch_unwind(AssertUnwindSafe(|| -> io::Result<()> {
        let len = record.UserDataLength as usize;
        let bytes = if len == 0 { &[][..] } else {
            if record.UserData.is_null() { return Err(io::Error::new(io::ErrorKind::InvalidData, "null ETW event data")); }
            unsafe { slice::from_raw_parts(record.UserData.cast::<u8>(), len) }
        };
        if let Some(frame) = context.assembler.ingest(record.EventHeader.EventDescriptor.Id,
                                                      record.EventHeader.ThreadId, bytes)? {
            (context.callback)(frame)?;
            context.frames += 1;
        }
        Ok(())
    }));
    match outcome {
        Ok(Ok(())) => (),
        Ok(Err(error)) => context.error = Some(error),
        Err(_) => context.error = Some(io::Error::other("ETW frame callback panicked")),
    }
}

unsafe extern "system" fn on_buffer(logfile: *mut EVENT_TRACE_LOGFILEW) -> u32 {
    if logfile.is_null() { return 0; }
    let logfile = unsafe { &*logfile };
    if logfile.Context.is_null() { return 0; }
    let state = unsafe { &*logfile.Context.cast::<Mutex<Context<'_>>>() };
    let mut context = state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if logfile.LogfileHeader.BuffersLost != 0 && context.error.is_none() {
        context.error = Some(io::Error::other("ETW trace reports lost buffers"));
    }
    u32::from(context.error.is_none())
}

/// Replay an existing ETL file, delivering frames synchronously in trace order.
/// Callback output remains provisional until this function succeeds: a later
/// malformed event, reported buffer loss or callback failure invalidates the run.
pub fn replay(path: &Path, callback: &mut (dyn FnMut(Vec<u8>) -> io::Result<()> + Send)) -> io::Result<usize> {
    consume(path.as_os_str(), false, callback)
}

/// Consume an already-running *real-time* session. This does not create,
/// enable, disable, or stop the session. The owner must start the consumer
/// before enabling sniffer output, stop the session to unblock ProcessTrace,
/// and query the session's own loss counters. Neither real-time behaviour nor
/// recovery have been validated on hardware; do not advertise a live backend.
pub fn consume_existing_session(name: &OsStr, callback: &mut (dyn FnMut(Vec<u8>) -> io::Result<()> + Send)) -> io::Result<usize> {
    consume(name, true, callback)
}

fn consume(source: &OsStr, realtime: bool, callback: &mut (dyn FnMut(Vec<u8>) -> io::Result<()> + Send)) -> io::Result<usize> {
    let mut wide: Vec<u16> = source.encode_wide().collect();
    if wide.is_empty() || wide.contains(&0) { return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid ETW source")); }
    wide.push(0);
    let mut state = Mutex::new(Context { assembler: FrameAssembler::new(), callback, error: None, frames: 0 });
    // Windows requires the output fields to be zeroed before OpenTrace.
    let mut log: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    if realtime { log.LoggerName = wide.as_mut_ptr(); } else { log.LogFileName = wide.as_mut_ptr(); }
    let mode = PROCESS_TRACE_MODE_EVENT_RECORD | if realtime { PROCESS_TRACE_MODE_REAL_TIME } else { 0 };
    log.Anonymous1 = EVENT_TRACE_LOGFILEW_0 { ProcessTraceMode: mode };
    log.Anonymous2 = EVENT_TRACE_LOGFILEW_1 { EventRecordCallback: Some(on_event) };
    log.BufferCallback = Some(on_buffer);
    log.Context = (&mut state as *mut Mutex<Context<'_>>).cast::<c_void>();
    let handle = unsafe { OpenTraceW(&mut log) };
    if handle.Value == u64::MAX { return Err(io::Error::last_os_error()); }
    let status = unsafe { ProcessTrace(&handle, 1, ptr::null(), ptr::null()) };
    let close = unsafe { CloseTrace(handle) };
    let context = state.into_inner().map_err(|_| io::Error::other("ETW state lock poisoned"))?;
    if let Some(error) = context.error { return Err(error); }
    if status != 0 { return Err(io::Error::from_raw_os_error(status as i32)); }
    if close != 0 { return Err(io::Error::from_raw_os_error(close as i32)); }
    context.assembler.finish()?;
    Ok(context.frames)
}
