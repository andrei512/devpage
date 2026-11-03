//! In-process extension ABI. Returned buffers are released by their producer.
pub use serde_json;
/// The ABI version this crate speaks; a package's manifest and its table must
/// both name it.
pub const API: u32 = 1;
/// Bytes handed across the boundary, laid out as the `Vec<u8>` they came from
/// so their producer can release them.
#[repr(C)]
pub struct Buffer {
    /// The first byte; null when nothing was returned.
    pub ptr: *mut u8,
    /// The number of bytes.
    pub len: usize,
    /// The capacity of the producer's allocation, needed to free it.
    pub capacity: usize,
}
/// What an extension library exports from `devpage_extension_v1`: its ABI
/// version and the two functions devpage calls.
#[repr(C)]
pub struct VTable {
    /// The [`API`] the library was built against.
    pub version: u32,
    /// The table's size in bytes, so a layout mismatch is caught before any
    /// call.
    pub size: usize,
    /// Answers a JSON request, given as bytes, with a JSON reply `{"ok": true,
    /// "value": …}` or `{"ok": false, "error": …}`.
    pub invoke: unsafe extern "C" fn(*const u8, usize) -> Buffer,
    /// Frees a [`Buffer`] that `invoke` returned.
    pub release: unsafe extern "C" fn(Buffer),
}
/// A native extension: one instance per library, created on its first request.
pub trait Extension: Default {
    /// The answer to a JSON `request`, or an error message for devpage to
    /// report.
    fn invoke(&mut self, request: serde_json::Value) -> Result<serde_json::Value, String>;
}
/// Exports `$kind`, an [`Extension`], as the library's [`VTable`], catching
/// panics and turning them into errors.
#[macro_export]
macro_rules! export_extension {
 ($kind:ty) => {
  static INSTANCE: std::sync::OnceLock<std::sync::Mutex<$kind>> = std::sync::OnceLock::new();
  unsafe extern "C" fn dispatch(ptr:*const u8,len:usize)->$crate::Buffer {
   let result=std::panic::catch_unwind(|| {
    let bytes=unsafe { std::slice::from_raw_parts(ptr,len) };
    let request=$crate::serde_json::from_slice(bytes).map_err(|e|e.to_string())?;
    let mut instance=INSTANCE.get_or_init(||std::sync::Mutex::new(<$kind>::default())).lock().map_err(|_|"Extension lock poisoned".to_string())?;
    <$kind as $crate::Extension>::invoke(&mut instance,request)
   }).unwrap_or_else(|_|Err("Extension panicked".into()));
   let reply=match result {Ok(value)=>$crate::serde_json::json!({"ok":true,"value":value}),Err(error)=>$crate::serde_json::json!({"ok":false,"error":error})};
   let mut bytes=$crate::serde_json::to_vec(&reply).unwrap_or_default();
   let buffer=$crate::Buffer{ptr:bytes.as_mut_ptr(),len:bytes.len(),capacity:bytes.capacity()};std::mem::forget(bytes);buffer
  }
  unsafe extern "C" fn release(buffer:$crate::Buffer) {if !buffer.ptr.is_null(){drop(unsafe {Vec::from_raw_parts(buffer.ptr,buffer.len,buffer.capacity)});}}
  #[unsafe(no_mangle)]
  pub extern "C" fn devpage_extension_v1()->*const $crate::VTable {
   static TABLE:$crate::VTable=$crate::VTable{version:$crate::API,size:std::mem::size_of::<$crate::VTable>(),invoke:dispatch,release}; &TABLE
  }
 };
}
