use std::fs;

const WASM_SRC_PATH: &str = "src/lib.rs";

const PANIC_HANDLER_STRING: &str = "multiversx_sc_wasm_adapter::panic_handler!();\n";

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={WASM_SRC_PATH}");

    // Repalces panic and allocation error handlers to not conflict with std
    let mut buf = fs::read_to_string(WASM_SRC_PATH)?;

    buf = buf.replace(PANIC_HANDLER_STRING, "");

    fs::write(WASM_SRC_PATH, buf)
}
