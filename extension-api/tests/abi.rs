use devpage_extension_api::{
    Extension, export_extension,
    serde_json::{Value, json},
};
#[derive(Default)]
struct Echo;
impl Extension for Echo {
    fn invoke(&mut self, request: Value) -> Result<Value, String> {
        match request["op"].as_str() {
            Some("echo") => Ok(request),
            Some("panic") => panic!("fixture panic"),
            _ => Err("unknown operation".into()),
        }
    }
}
export_extension!(Echo);

#[test]
fn native_buffers_errors_and_panics_follow_the_abi() {
    let table = unsafe { &*devpage_extension_v1() };
    assert_eq!(table.version, devpage_extension_api::API);
    assert_eq!(table.size, std::mem::size_of_val(table));
    let call = |request: &[u8]| {
        let buffer = unsafe { (table.invoke)(request.as_ptr(), request.len()) };
        let value: Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) })
                .unwrap();
        unsafe { (table.release)(buffer) };
        value
    };
    assert_eq!(
        call(br#"{"op":"echo","input":"hello"}"#)["value"],
        json!({"op":"echo","input":"hello"})
    );
    assert_eq!(call(br#"{"op":"missing"}"#)["error"], "unknown operation");
    assert_eq!(call(b"not json")["ok"], false);
    assert_eq!(call(br#"{"op":"panic"}"#)["error"], "Extension panicked");
}
