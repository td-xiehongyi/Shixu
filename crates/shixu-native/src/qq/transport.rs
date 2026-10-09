//! Transport-independent normalized receive contract, NOT a NapCat wire schema.
//! A real implementation is blocked on G2 capability/protocol verification.
//! Tokens are consumed, never serialized/logged. No send or management methods.
use shixu_core::contracts::{AppResult, notification::*, vault::SecretBytes};
use std::net::SocketAddr;
pub struct LoopbackEndpoint(SocketAddr);
impl LoopbackEndpoint {
    pub fn parse(value: &str) -> AppResult<Self> {
        let address: SocketAddr = value
            .parse()
            .map_err(|_| shixu_core::contracts::error::AppError::InvalidInput)?;
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(shixu_core::contracts::error::AppError::InvalidInput);
        }
        Ok(Self(address))
    }
    pub fn address(&self) -> SocketAddr {
        self.0
    }
}
pub struct Delivery {
    pub message: MessageEnvelope,
    pub cursor: String,
}
/// `complete=true` is exhaustive proof for the entire requested interval from
/// the supplied original anchor. `complete=false` carries unverified results:
/// no contiguous prefix or covered endpoint can be inferred from returned IDs.
/// Results may be persisted, but every subsequent request must replay the anchor.
/// Actual evidence supporting this promise remains a blocked G2 obligation.
pub struct BackfillBatch {
    pub group_id: String,
    pub deliveries: Vec<Delivery>,
    pub complete: bool,
}
/// Implementors must verify configuration/account identity and report only
/// proven capabilities. A cursor is an opaque non-secret ID in group scope;
/// complete asserts the entire requested interval from the original anchor was exhaustively recovered.
/// No redirects, discovery, or alternate destinations are permitted: credentials
/// may only be sent to the trusted literal loopback endpoint. Attachment native
/// IDs must use adapter::stable_attachment_ref; download URLs remain transient.
pub trait ReceiveTransport: Send {
    fn connect(
        &mut self,
        endpoint: &LoopbackEndpoint,
        config: &SourceConfig,
        token: SecretBytes,
    ) -> AppResult<Vec<SourceCapability>>;
    fn disconnect(&mut self) -> AppResult<()>;
    fn next(&mut self) -> AppResult<Option<Delivery>>;
    fn backfill(&mut self, group: &str, cursor: &str) -> AppResult<BackfillBatch>;
}
