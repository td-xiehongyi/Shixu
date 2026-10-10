//! Fixed native event vocabulary; no WebView event authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeVaultEvent {
    SessionLocked,
    SessionUnlocked,
    Suspend,
    Resume,
    WindowClosed,
    Revoked,
}
/// Actual message values; disconnect/logoff conservatively revoke the session.
pub fn decode(message: u32, event: usize) -> Option<NativeVaultEvent> {
    match (message, event) {
        (0x02b1, 2 | 4 | 6 | 7) => Some(NativeVaultEvent::SessionLocked),
        (0x02b1, 8) => Some(NativeVaultEvent::SessionUnlocked),
        (0x0218, 4) => Some(NativeVaultEvent::Suspend),
        (0x0218, 18) => Some(NativeVaultEvent::Resume),
        _ => None,
    }
}
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod windows;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_session_and_automatic_power_messages_have_authority() {
        assert_eq!(decode(0x2b1, 7), Some(NativeVaultEvent::SessionLocked));
        assert_eq!(decode(0x2b1, 8), Some(NativeVaultEvent::SessionUnlocked));
        for code in [2, 4, 6] {
            assert_eq!(decode(0x2b1, code), Some(NativeVaultEvent::SessionLocked));
        }
        assert_eq!(decode(0x218, 4), Some(NativeVaultEvent::Suspend));
        assert_eq!(decode(0x218, 18), Some(NativeVaultEvent::Resume));
        assert_eq!(decode(0x218, 7), None); // RESUMESUSPEND follows automatic; no second restart.
        assert_eq!(decode(0x400, 7), None);
    }
}
