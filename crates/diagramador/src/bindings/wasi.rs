//! C-ABI bindings for `wasm32-wasip1`.
//!
//! Callable from Python (wasmtime), Go (wazero), or any host with a wasm
//! runtime — the same engine the browser uses, so a PDF built on a server
//! matches the one the editor showed.
//!
//! # Calling convention
//!
//! Every entry point returns `i32`: `>= 0` on success (a byte count where one
//! applies), `< 0` on failure. After a failure, read the message with
//! [`dgm_error_ptr`] / [`dgm_error_len`].
//!
//! Results are left in an internal buffer rather than written into caller
//! memory, so the host never has to guess an output size:
//!
//! ```text
//! ptr = dgm_alloc(len)              ; copy the document JSON in
//! n   = dgm_render_pdf(ptr, len)    ; n < 0 means failure
//! out = dgm_result_ptr()            ; read n bytes from here
//! dgm_free(ptr, len)
//! ```
//!
//! The result buffer stays valid until the next call that produces a result.

use std::alloc::Layout;
use std::cell::RefCell;

use crate::engine::Engine;
use crate::spec::{Document, FontWeight};

/// Returned when a pointer/length pair does not describe valid UTF-8 or the
/// arguments are otherwise unusable.
pub const ERR_INVALID_ARGUMENT: i32 = -1;
/// The document JSON could not be parsed.
pub const ERR_PARSE: i32 = -2;
/// A font could not be registered.
pub const ERR_FONT: i32 = -3;
/// Rendering failed.
pub const ERR_RENDER: i32 = -4;

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::new());
    static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

// ─────────────────────────────────────────────────────────────────────────────
// Memory
// ─────────────────────────────────────────────────────────────────────────────

/// Allocate `len` bytes inside the module for the host to write into.
///
/// Free it with [`dgm_free`] and the same `len`.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_alloc(len: u32) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    match Layout::from_size_align(len as usize, 1) {
        // SAFETY: the layout has a non-zero size.
        Ok(layout) => unsafe { std::alloc::alloc(layout) },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Release memory obtained from [`dgm_alloc`].
///
/// # Safety
/// `ptr` must come from [`dgm_alloc`] with the same `len`, and must not have
/// been freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dgm_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(len as usize, 1) {
        // SAFETY: delegated to the caller by this function's contract.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Results and errors
// ─────────────────────────────────────────────────────────────────────────────

/// Pointer to the bytes produced by the last successful call.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_result_ptr() -> *const u8 {
    RESULT.with(|result| result.borrow().as_ptr())
}

/// Length of the last result, in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_result_len() -> u32 {
    RESULT.with(|result| result.borrow().len() as u32)
}

/// Pointer to the last error message, as UTF-8.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_error_ptr() -> *const u8 {
    ERROR.with(|error| error.borrow().as_ptr())
}

/// Length of the last error message, in bytes.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_error_len() -> u32 {
    ERROR.with(|error| error.borrow().len() as u32)
}

/// Schema version this build speaks.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_schema_version() -> u32 {
    crate::spec::SCHEMA_VERSION
}

fn fail(code: i32, message: impl Into<String>) -> i32 {
    ERROR.with(|error| *error.borrow_mut() = message.into());
    code
}

fn succeed(bytes: Vec<u8>) -> i32 {
    let len = bytes.len() as i32;
    RESULT.with(|result| *result.borrow_mut() = bytes);
    ERROR.with(|error| error.borrow_mut().clear());
    len
}

/// # Safety
/// `ptr` must point to `len` readable bytes.
unsafe fn as_slice<'a>(ptr: *const u8, len: u32) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return (len == 0).then_some(&[]);
    }
    // SAFETY: delegated to the caller by this function's contract.
    Some(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
}

/// # Safety
/// `ptr` must point to `len` readable bytes.
unsafe fn as_str<'a>(ptr: *const u8, len: u32) -> Option<&'a str> {
    // SAFETY: delegated to the caller by this function's contract.
    unsafe { as_slice(ptr, len) }.and_then(|bytes| std::str::from_utf8(bytes).ok())
}

// ─────────────────────────────────────────────────────────────────────────────
// Resources
// ─────────────────────────────────────────────────────────────────────────────

/// Register a font face.
///
/// `weight` of 0 and `italic` of -1 mean "read it from the font".
///
/// # Safety
/// Both pointer/length pairs must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dgm_add_font(
    family_ptr: *const u8,
    family_len: u32,
    data_ptr: *const u8,
    data_len: u32,
    weight: u32,
    italic: i32,
) -> i32 {
    // SAFETY: delegated to the caller by this function's contract.
    let (Some(family), Some(data)) =
        (unsafe { as_str(family_ptr, family_len) }, unsafe { as_slice(data_ptr, data_len) })
    else {
        return fail(ERR_INVALID_ARGUMENT, "família ou dados de fonte inválidos");
    };

    let weight = (weight > 0).then_some(FontWeight(weight as u16));
    let italic = match italic {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };

    ENGINE.with(|engine| {
        match engine
            .borrow_mut()
            .add_font(family, data.to_vec(), weight, italic)
        {
            Ok(id) => id.0 as i32,
            Err(error) => fail(ERR_FONT, error.to_string()),
        }
    })
}

/// Register image bytes under a key.
///
/// # Safety
/// Both pointer/length pairs must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dgm_add_image(
    key_ptr: *const u8,
    key_len: u32,
    data_ptr: *const u8,
    data_len: u32,
) -> i32 {
    // SAFETY: delegated to the caller by this function's contract.
    let (Some(key), Some(data)) =
        (unsafe { as_str(key_ptr, key_len) }, unsafe { as_slice(data_ptr, data_len) })
    else {
        return fail(ERR_INVALID_ARGUMENT, "chave ou dados de imagem inválidos");
    };

    ENGINE.with(|engine| engine.borrow_mut().add_image(key, data.to_vec()));
    0
}

/// Drop every registered font and image.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_clear() {
    ENGINE.with(|engine| engine.borrow_mut().clear());
}

// ─────────────────────────────────────────────────────────────────────────────
// Layout and rendering
// ─────────────────────────────────────────────────────────────────────────────

/// Lay a document out; the result buffer receives the display list as JSON.
///
/// # Safety
/// The pointer/length pair must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dgm_layout(json_ptr: *const u8, json_len: u32) -> i32 {
    // SAFETY: delegated to the caller by this function's contract.
    let Some(json) = (unsafe { as_str(json_ptr, json_len) }) else {
        return fail(ERR_INVALID_ARGUMENT, "o JSON do documento não é UTF-8");
    };

    let document: Document = match serde_json::from_str(json) {
        Ok(document) => document,
        Err(error) => return fail(ERR_PARSE, format!("documento inválido: {error}")),
    };

    let list = ENGINE.with(|engine| engine.borrow().layout(&document));
    match serde_json::to_vec(&list) {
        Ok(bytes) => succeed(bytes),
        Err(error) => fail(ERR_RENDER, error.to_string()),
    }
}

/// Render a document; the result buffer receives the PDF bytes.
///
/// # Safety
/// The pointer/length pair must describe readable memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dgm_render_pdf(json_ptr: *const u8, json_len: u32) -> i32 {
    // SAFETY: delegated to the caller by this function's contract.
    let Some(json) = (unsafe { as_str(json_ptr, json_len) }) else {
        return fail(ERR_INVALID_ARGUMENT, "o JSON do documento não é UTF-8");
    };

    let document: Document = match serde_json::from_str(json) {
        Ok(document) => document,
        Err(error) => return fail(ERR_PARSE, format!("documento inválido: {error}")),
    };

    ENGINE.with(|engine| match engine.borrow().render_pdf(&document) {
        Ok(bytes) => succeed(bytes),
        Err(error) => fail(ERR_RENDER, error.to_string()),
    })
}

/// Outline of one glyph; the result buffer receives an SVG path as UTF-8.
///
/// Returns 0 when the glyph has no outline, which is normal for a space.
#[unsafe(no_mangle)]
pub extern "C" fn dgm_glyph_path(font: u32, glyph: u32) -> i32 {
    ENGINE.with(|engine| {
        match engine.borrow().glyph_path(font, glyph as u16) {
            Some(path) => succeed(path.into_bytes()),
            None => succeed(Vec::new()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::test_fonts;

    /// Drive the ABI exactly as a host would, through raw pointers.
    fn call(json: &str, render: bool) -> Result<Vec<u8>, (i32, String)> {
        let bytes = json.as_bytes();
        let ptr = dgm_alloc(bytes.len() as u32);
        // SAFETY: `ptr` has room for `bytes.len()` bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };

        let code = if render {
            unsafe { dgm_render_pdf(ptr, bytes.len() as u32) }
        } else {
            unsafe { dgm_layout(ptr, bytes.len() as u32) }
        };

        let out = if code < 0 {
            let message = ERROR.with(|error| error.borrow().clone());
            Err((code, message))
        } else {
            // SAFETY: the result buffer holds `code` readable bytes.
            let slice =
                unsafe { std::slice::from_raw_parts(dgm_result_ptr(), dgm_result_len() as usize) };
            assert_eq!(slice.len() as i32, code, "returned length must match");
            Ok(slice.to_vec())
        };

        // SAFETY: `ptr` came from `dgm_alloc` with this length.
        unsafe { dgm_free(ptr, bytes.len() as u32) };
        out
    }

    fn register_font() -> bool {
        let Some(bytes) = test_fonts::dejavu() else {
            return false;
        };
        let family = b"corpo";
        // SAFETY: both slices are live for the duration of the call.
        let id = unsafe {
            dgm_add_font(
                family.as_ptr(),
                family.len() as u32,
                bytes.as_ptr(),
                bytes.len() as u32,
                400,
                0,
            )
        };
        assert!(id >= 0, "font registration failed");
        true
    }

    const DOC: &str = r#"{"pages":[{"frames":[
        {"type":"text","rect":[56,56,400,200],"blocks":["Olá mundo"]}
    ]}]}"#;

    #[test]
    fn alloc_and_free_round_trip() {
        let ptr = dgm_alloc(64);
        assert!(!ptr.is_null());
        // SAFETY: matching alloc/free pair.
        unsafe { dgm_free(ptr, 64) };

        // Zero-length allocations are a no-op, not a crash.
        assert!(dgm_alloc(0).is_null());
        unsafe { dgm_free(std::ptr::null_mut(), 0) };
    }

    #[test]
    fn layout_returns_a_display_list() {
        dgm_clear();
        if !register_font() {
            return;
        }

        let bytes = call(DOC, false).expect("layout succeeded");
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\"pages\""));
        assert!(text.contains("\"glyphs\""));
        assert!(text.contains("Olá mundo"));
    }

    #[test]
    fn render_returns_pdf_bytes() {
        dgm_clear();
        if !register_font() {
            return;
        }

        let bytes = call(DOC, true).expect("render succeeded");
        assert!(bytes.starts_with(b"%PDF-1."));
    }

    #[test]
    fn malformed_json_reports_a_parse_error() {
        dgm_clear();
        let (code, message) = call("{ nope", false).unwrap_err();
        assert_eq!(code, ERR_PARSE);
        assert!(!message.is_empty());
    }

    #[test]
    fn rendering_without_fonts_reports_an_error() {
        dgm_clear();
        let (code, message) = call(DOC, true).unwrap_err();
        assert_eq!(code, ERR_RENDER);
        assert!(message.contains("font"), "unexpected message: {message}");
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        dgm_clear();
        let bad = [0xff, 0xfe, 0xfd];
        // SAFETY: the slice is live for the duration of the call.
        let code = unsafe { dgm_layout(bad.as_ptr(), bad.len() as u32) };
        assert_eq!(code, ERR_INVALID_ARGUMENT);
        assert!(dgm_error_len() > 0);
    }

    #[test]
    fn a_space_has_no_outline_but_is_not_an_error() {
        dgm_clear();
        if !register_font() {
            return;
        }
        // Glyph 3 is the space in DejaVu; any outline-less glyph returns 0.
        assert_eq!(dgm_glyph_path(0, 3), 0);
    }

    #[test]
    fn the_two_bindings_agree_on_the_schema_version() {
        assert_eq!(dgm_schema_version(), crate::spec::SCHEMA_VERSION);
    }
}
