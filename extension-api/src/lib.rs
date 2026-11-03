//! In-process extension ABI. Returned buffers are released by their producer.
pub use serde_json;
pub const API: u32 = 1;
#[repr(C)]
pub struct Buffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub capacity: usize,
}
#[repr(C)]
pub struct VTable {
    pub version: u32,
    pub size: usize,
    pub invoke: unsafe extern "C" fn(*const u8, usize) -> Buffer,
    pub release: unsafe extern "C" fn(Buffer),
}
pub trait Extension: Default {
    fn invoke(&mut self, request: serde_json::Value) -> Result<serde_json::Value, String>;
}
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
