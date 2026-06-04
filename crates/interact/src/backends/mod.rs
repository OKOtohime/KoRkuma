pub mod cdp;
pub mod stub;

#[cfg(target_os = "windows")]
pub mod windows;

pub use cdp::CdpBackend;
pub use stub::StubBackend;

#[cfg(target_os = "windows")]
pub use self::windows::{SendInputBackend, UiaBackend, WinMsgBackend};
