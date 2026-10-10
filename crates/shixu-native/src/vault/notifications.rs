//! Internal native events; no WebView event authority or OS session/power hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeVaultEvent {
    WindowClosed,
    Revoked,
}
