// Minimal Rust example with Mutex for MIR dump.
// Generate MIR: rustc --emit=mir -Z unpretty=mir tiny_mutex.rs

fn main() {
    let m = std::sync::Mutex::new(1);
    let _g = m.lock().unwrap();
}
