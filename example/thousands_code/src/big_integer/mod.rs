#![cfg(feature = "big-math")]

pub mod fast_factorial;
pub mod multiply;
pub mod poly1305;

pub use self::fast_factorial::fast_factorial;
pub use self::multiply::multiply;
pub use self::poly1305::Poly1305;
