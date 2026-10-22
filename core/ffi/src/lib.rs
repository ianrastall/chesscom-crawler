//! C ABI facade over [`chesscom_crawler_core::Core`] for the WinUI shell.
//!
//! Contract (ABI version 1):
//!
//! - Commands and queries are protobuf `Request` messages passed to `ccc_call`; the reply is a
//!   `Response` held in a Rust-owned result handle. Bytes from `ccc_result_bytes` stay valid
//!   until `ccc_result_free`.
//! - Status codes report transport outcomes only. Domain failures are `Response.error`.
//! - `ccc_create` sets `*out_core` to null on failure and, when possible, sets `*out_error` to a
//!   result holding an `Error` message. Every other output is null on failure unless stated.
//! - A core handle may be used from any thread, including concurrently. After a contained panic
//!   the handle is unusable: further calls return `CCC_UNUSABLE`; `ccc_destroy` is still valid.
//! - `ccc_destroy` stops the crawl and waits up to 30 seconds for its thread.
//! - Memory is freed only by the side that allocated it.

pub mod convert;

#[allow(clippy::all)]
pub mod messages {
    include!(concat!(env!("OUT_DIR"), "/jupiter.chesscom_crawler.v1.rs"));
}

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chesscom_crawler_core::{Core, CoreConfig, CoreError};
use prost::Message;

use messages::{ErrorKind, request, response};

pub const ABI_VERSION: u32 = 1;

pub const CCC_OK: i32 = 0;
pub const CCC_INVALID_ARGUMENT: i32 = 1;
pub const CCC_MALFORMED_REQUEST: i32 = 2;
pub const CCC_PANIC: i32 = 3;
pub const CCC_UNUSABLE: i32 = 4;
pub const CCC_CREATE_FAILED: i32 = 5;

pub struct CoreHandle {
    core: Core,
    poisoned: AtomicBool,
}

pub struct ResultHandle {
    bytes: Vec<u8>,
}

fn result(message: &impl Message) -> *mut ResultHandle {
    Box::into_raw(Box::new(ResultHandle { bytes: message.encode_to_vec() }))
}

fn error_response(request_id: u64, kind: ErrorKind, message: impl Into<String>, field: &str) -> messages::Response {
    messages::Response {
        request_id,
        result: Some(response::Result::Error(messages::Error {
            kind: kind as i32,
            message: message.into(),
            field: field.to_string(),
        })),
    }
}

fn core_error(request_id: u64, e: CoreError) -> messages::Response {
    match e {
        CoreError::Settings(s) => error_response(request_id, ErrorKind::InvalidSettings, s.message, s.field),
        CoreError::Io(m) => error_response(request_id, ErrorKind::Io, m, ""),
        CoreError::Store(m) => error_response(request_id, ErrorKind::State, m, ""),
    }
}

/// Reads `len` bytes at `ptr`; a null pointer is valid only with length zero.
///
/// # Safety
/// `ptr` must be valid for reads of `len` bytes for the duration of the call.
unsafe fn input<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `ptr` is readable for `len` bytes during this call.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// Dispatches one decoded request against the core.
pub fn dispatch(core: &Core, req: messages::Request) -> messages::Response {
    let id = req.request_id;
    let ok = |r: response::Result| messages::Response { request_id: id, result: Some(r) };
    match req.command {
        Some(request::Command::GetStatus(_)) => {
            ok(response::Result::Status(convert::status_to_message(&core.status())))
        }
        Some(request::Command::GetSettings(_)) => {
            ok(response::Result::Settings(convert::settings_to_message(&core.settings())))
        }
        Some(request::Command::UpdateSettings(u)) => {
            let Some(settings) = u.settings else {
                return error_response(id, ErrorKind::InvalidSettings, "No settings were supplied.", "");
            };
            match convert::settings_from_message(&settings)
                .map_err(CoreError::Settings)
                .and_then(|s| core.update_settings(s))
            {
                Ok(saved) => ok(response::Result::Settings(convert::settings_to_message(&saved))),
                Err(e) => core_error(id, e),
            }
        }
        Some(request::Command::StartCrawl(_)) => match core.start() {
            Ok(()) => ok(response::Result::Done(messages::Done { message: "Crawl started.".into() })),
            Err(e) => core_error(id, e),
        },
        Some(request::Command::StopCrawl(_)) => {
            core.stop();
            ok(response::Result::Done(messages::Done { message: "Stop requested.".into() }))
        }
        Some(request::Command::ExportCollections(_)) => match core.export_all() {
            Ok(n) => {
                ok(response::Result::Done(messages::Done { message: format!("Rewrote collections for {n} months.") }))
            }
            Err(e) => core_error(id, e),
        },
        None => error_response(id, ErrorKind::UnknownCommand, "Unknown or missing command.", ""),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ccc_abi_version() -> u32 {
    ABI_VERSION
}

/// Creates a core from a serialized `CreateConfig`.
///
/// # Safety
/// `config_ptr` must be readable for `config_len` bytes. `out_core` must be a valid pointer;
/// `out_error` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_create(
    config_ptr: *const u8,
    config_len: usize,
    out_core: *mut *mut CoreHandle,
    out_error: *mut *mut ResultHandle,
) -> i32 {
    if out_core.is_null() {
        return CCC_INVALID_ARGUMENT;
    }
    // SAFETY: checked non-null; the caller provides a writable pointer.
    unsafe { *out_core = std::ptr::null_mut() };
    if !out_error.is_null() {
        // SAFETY: non-null and writable per the contract.
        unsafe { *out_error = std::ptr::null_mut() };
    }
    let outcome = catch_unwind(|| {
        // SAFETY: forwarded caller guarantee.
        let bytes = unsafe { input(config_ptr, config_len) }.ok_or((CCC_INVALID_ARGUMENT, None))?;
        let config = messages::CreateConfig::decode(bytes).map_err(|_| (CCC_MALFORMED_REQUEST, None))?;
        if config.data_dir.is_empty() {
            return Err((
                CCC_INVALID_ARGUMENT,
                Some(Box::new(error_response(0, ErrorKind::Io, "No data folder was given.", ""))),
            ));
        }
        let core = Core::open(CoreConfig {
            data_dir: PathBuf::from(config.data_dir),
            default_output_dir: PathBuf::from(config.default_output_dir),
        })
        .map_err(|e| (CCC_CREATE_FAILED, Some(Box::new(core_error(0, e)))))?;
        Ok(Box::into_raw(Box::new(CoreHandle { core, poisoned: AtomicBool::new(false) })))
    });
    match outcome {
        Ok(Ok(handle)) => {
            // SAFETY: checked non-null above.
            unsafe { *out_core = handle };
            CCC_OK
        }
        Ok(Err((status, error))) => {
            if let (Some(e), false) = (error, out_error.is_null()) {
                // SAFETY: checked non-null.
                unsafe { *out_error = result(e.as_ref()) };
            }
            status
        }
        Err(_) => CCC_PANIC,
    }
}

/// Sends a serialized `Request`; on `CCC_OK`, `*out_result` holds a `Response`.
///
/// # Safety
/// `core` must come from `ccc_create` and not be destroyed. `request_ptr` must be readable for
/// `request_len` bytes. `out_result` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_call(
    core: *const CoreHandle,
    request_ptr: *const u8,
    request_len: usize,
    out_result: *mut *mut ResultHandle,
) -> i32 {
    if core.is_null() || out_result.is_null() {
        return CCC_INVALID_ARGUMENT;
    }
    // SAFETY: checked non-null; writable per the contract.
    unsafe { *out_result = std::ptr::null_mut() };
    // SAFETY: the caller guarantees a live handle from ccc_create.
    let handle = unsafe { &*core };
    if handle.poisoned.load(Ordering::Acquire) {
        return CCC_UNUSABLE;
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: forwarded caller guarantee.
        let bytes = unsafe { input(request_ptr, request_len) }.ok_or(CCC_INVALID_ARGUMENT)?;
        let req = messages::Request::decode(bytes).map_err(|_| CCC_MALFORMED_REQUEST)?;
        Ok(result(&dispatch(&handle.core, req)))
    }));
    match outcome {
        Ok(Ok(r)) => {
            // SAFETY: checked non-null above.
            unsafe { *out_result = r };
            CCC_OK
        }
        Ok(Err(status)) => status,
        Err(_) => {
            handle.poisoned.store(true, Ordering::Release);
            CCC_PANIC
        }
    }
}

/// Removes up to `max` pending activity events; `*out_result` holds an `EventBatch`.
///
/// # Safety
/// As for `ccc_call`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_poll_events(core: *const CoreHandle, max: u32, out_result: *mut *mut ResultHandle) -> i32 {
    if core.is_null() || out_result.is_null() {
        return CCC_INVALID_ARGUMENT;
    }
    // SAFETY: checked non-null; writable per the contract.
    unsafe { *out_result = std::ptr::null_mut() };
    // SAFETY: the caller guarantees a live handle from ccc_create.
    let handle = unsafe { &*core };
    if handle.poisoned.load(Ordering::Acquire) {
        return CCC_UNUSABLE;
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let events = handle.core.poll_events(max as usize);
        result(&messages::EventBatch { events: events.iter().map(convert::event_to_message).collect() })
    })) {
        Ok(r) => {
            // SAFETY: checked non-null above.
            unsafe { *out_result = r };
            CCC_OK
        }
        Err(_) => {
            handle.poisoned.store(true, Ordering::Release);
            CCC_PANIC
        }
    }
}

/// Exposes a result's bytes. Valid until `ccc_result_free`.
///
/// # Safety
/// `result` must come from this library and not be freed; `out_ptr` and `out_len` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_result_bytes(
    result: *const ResultHandle,
    out_ptr: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    if result.is_null() || out_ptr.is_null() || out_len.is_null() {
        return CCC_INVALID_ARGUMENT;
    }
    // SAFETY: all pointers checked non-null; the caller guarantees `result` is live.
    unsafe {
        let r = &*result;
        *out_ptr = r.bytes.as_ptr();
        *out_len = r.bytes.len();
    }
    CCC_OK
}

/// Frees a result. Null is ignored.
///
/// # Safety
/// `result` must come from this library and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_result_free(result: *mut ResultHandle) {
    if !result.is_null() {
        // SAFETY: ownership returns to Rust exactly once per the contract.
        drop(unsafe { Box::from_raw(result) });
    }
}

/// Stops the crawl, waits up to 30 seconds for it, and frees the core. Null is ignored.
///
/// # Safety
/// `core` must come from `ccc_create`, must not be in use by another call, and must not be used
/// afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ccc_destroy(core: *mut CoreHandle) {
    if core.is_null() {
        return;
    }
    // SAFETY: ownership returns to Rust exactly once per the contract.
    let handle = unsafe { Box::from_raw(core) };
    let _ = catch_unwind(AssertUnwindSafe(move || {
        handle.core.shutdown(Duration::from_secs(30));
        drop(handle);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(core: *const CoreHandle, req: &messages::Request) -> messages::Response {
        let bytes = req.encode_to_vec();
        let mut out = std::ptr::null_mut();
        let status = unsafe { ccc_call(core, bytes.as_ptr(), bytes.len(), &mut out) };
        assert_eq!(status, CCC_OK);
        let (mut ptr, mut len) = (std::ptr::null(), 0usize);
        assert_eq!(unsafe { ccc_result_bytes(out, &mut ptr, &mut len) }, CCC_OK);
        let resp = messages::Response::decode(unsafe { std::slice::from_raw_parts(ptr, len) }).unwrap();
        unsafe { ccc_result_free(out) };
        resp
    }

    #[test]
    fn round_trips_through_the_abi() {
        assert_eq!(ccc_abi_version(), ABI_VERSION);
        let dir = tempfile_dir();
        let config = messages::CreateConfig {
            data_dir: dir.join("data").to_string_lossy().into(),
            default_output_dir: dir.join("out").to_string_lossy().into(),
        }
        .encode_to_vec();
        let mut core = std::ptr::null_mut();
        let mut err = std::ptr::null_mut();
        assert_eq!(unsafe { ccc_create(config.as_ptr(), config.len(), &mut core, &mut err) }, CCC_OK);
        assert!(err.is_null());

        let resp = call(
            core,
            &messages::Request { request_id: 7, command: Some(request::Command::GetSettings(Default::default())) },
        );
        assert_eq!(resp.request_id, 7);
        let Some(response::Result::Settings(mut settings)) = resp.result else { panic!("{resp:?}") };
        assert_eq!(settings.floors, vec![2400]);
        assert!(settings.output_folder.ends_with("out"));

        settings.floors = vec![2700, 2400];
        let resp = call(
            core,
            &messages::Request {
                request_id: 8,
                command: Some(request::Command::UpdateSettings(messages::UpdateSettings {
                    settings: Some(settings.clone()),
                })),
            },
        );
        let Some(response::Result::Settings(saved)) = resp.result else { panic!("{resp:?}") };
        assert_eq!(saved.floors, vec![2400, 2700]);

        settings.request_interval_seconds = 0.1;
        let resp = call(
            core,
            &messages::Request {
                request_id: 9,
                command: Some(request::Command::UpdateSettings(messages::UpdateSettings { settings: Some(settings) })),
            },
        );
        let Some(response::Result::Error(e)) = resp.result else { panic!("{resp:?}") };
        assert_eq!(e.kind, ErrorKind::InvalidSettings as i32);
        assert_eq!(e.field, "request_interval_seconds");

        let resp = call(core, &messages::Request { request_id: 10, command: None });
        assert!(matches!(resp.result, Some(response::Result::Error(_))));

        let resp = call(
            core,
            &messages::Request { request_id: 11, command: Some(request::Command::GetStatus(Default::default())) },
        );
        let Some(response::Result::Status(status)) = resp.result else { panic!("{resp:?}") };
        assert_eq!(status.state, messages::RunState::Idle as i32);

        let garbage = [0xFFu8, 0xFF, 0xFF];
        let mut out = std::ptr::null_mut();
        assert_eq!(unsafe { ccc_call(core, garbage.as_ptr(), garbage.len(), &mut out) }, CCC_MALFORMED_REQUEST);
        assert!(out.is_null());

        let mut events = std::ptr::null_mut();
        assert_eq!(unsafe { ccc_poll_events(core, 100, &mut events) }, CCC_OK);
        unsafe { ccc_result_free(events) };

        unsafe { ccc_destroy(core) };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_reports_bad_input() {
        let mut core = std::ptr::null_mut();
        let mut err = std::ptr::null_mut();
        let empty = messages::CreateConfig::default().encode_to_vec();
        assert_eq!(unsafe { ccc_create(empty.as_ptr(), empty.len(), &mut core, &mut err) }, CCC_INVALID_ARGUMENT);
        assert!(core.is_null() && !err.is_null());
        unsafe { ccc_result_free(err) };
        assert_eq!(unsafe { ccc_create(std::ptr::null(), 3, &mut core, std::ptr::null_mut()) }, CCC_INVALID_ARGUMENT);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ccc-ffi-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
