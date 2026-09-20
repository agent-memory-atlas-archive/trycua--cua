//! Shared read/act boundary for backend-owned native references.
//!
//! A reference identifies an object, not a snapshot. Windows and controls use
//! the same primitive. Native serialization, storage, expiry, and thread-affine
//! ownership belong to the backend; this module contains no reference table.
//!
//! Public tool adapters must authorize their operation before entering here.
//! `resolve` must authenticate the reference and validate its runtime/scope;
//! the backend must also check live scope and capability before native input.
use crate::protocol::{ToolCall, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Opaque backend-owned wire value. Never interpret it as a raw pointer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Reference(String);

impl Reference {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Reference {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Native-system boundary, implemented independently by each platform.
///
/// A lease owns the native reference for the entire operation. Implementations
/// that spawn native work must move the lease into that work, not drop it when
/// an awaiting caller is cancelled. Retaining a local proxy does not guarantee
/// that its remote control is still alive. References must never be redirected
/// to a replacement using descriptive matching.
#[async_trait]
pub trait NativeReferenceBackend: Send + Sync {
    type Lease: Send;

    /// Bootstrap discovery with a backend-owned root reference. Subsequent
    /// window/control discovery uses ordinary reads, not another address type.
    async fn root(&self) -> Result<Reference, ToolResult>;

    /// Acquire an owned lease, or refuse before read/action dispatch. Another
    /// observation must not itself invalidate an existing reference.
    async fn resolve(&self, reference: &Reference) -> Result<Self::Lease, ToolResult>;

    /// Observe the leased object. Discovery may publish child references using
    /// backend-owned storage; observations do not own those references.
    async fn read(&self, lease: Self::Lease, options: Value) -> ToolResult;

    /// Execute through the leased native object, never a replacement found by
    /// title/description. Native validation and existing actuator rules apply.
    async fn act(&self, lease: Self::Lease, action: ToolCall) -> ToolResult;
}

/// Read the same reference primitive that actions accept.
pub async fn read<B: NativeReferenceBackend>(
    backend: &B,
    reference: &Reference,
    options: Value,
) -> ToolResult {
    match backend.resolve(reference).await {
        Ok(lease) => backend.read(lease, options).await,
        Err(refusal) => refusal,
    }
}

/// Resolve before dispatch and give the native operation ownership of its lease.
pub async fn act<B: NativeReferenceBackend>(
    backend: &B,
    reference: &Reference,
    action: ToolCall,
) -> ToolResult {
    match backend.resolve(reference).await {
        Ok(lease) => backend.act(lease, action).await,
        Err(refusal) => refusal,
    }
}
