//! The shared operation boundary owns no native objects or reference table.
use async_trait::async_trait;
use cua_driver_core::{
    native_reference::{self, NativeReferenceBackend, Reference},
    protocol::{ToolCall, ToolResult},
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

// Native-system boundary double: resolving obtains an owned operation lease.
struct Backend {
    leases: Arc<AtomicUsize>,
}
struct Lease(Arc<AtomicUsize>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
#[async_trait]
impl NativeReferenceBackend for Backend {
    type Lease = Lease;
    async fn root(&self) -> Result<Reference, ToolResult> {
        Ok(Reference::from(
            "backend-owned-window-or-control".to_owned(),
        ))
    }
    async fn resolve(&self, reference: &Reference) -> Result<Lease, ToolResult> {
        if reference.as_str() != "backend-owned-window-or-control" {
            return Err(ToolResult::error("reference unavailable"));
        }
        self.leases.fetch_add(1, Ordering::SeqCst);
        Ok(Lease(self.leases.clone()))
    }
    async fn read(&self, _lease: Lease, options: Value) -> ToolResult {
        assert_eq!(self.leases.load(Ordering::SeqCst), 1);
        ToolResult::text(format!("observed:{options}"))
    }
    async fn act(&self, _lease: Lease, action: ToolCall) -> ToolResult {
        assert_eq!(self.leases.load(Ordering::SeqCst), 1);
        ToolResult::text(format!("executed:{}", action.name))
    }
}

#[tokio::test]
async fn read_and_act_use_the_same_reference_and_owned_lease() {
    let backend = Backend {
        leases: Arc::new(AtomicUsize::new(0)),
    };
    let reference = backend.root().await.unwrap();
    let read = native_reference::read(&backend, &reference, json!({})).await;
    assert!(!read.is_error.unwrap_or(false));
    assert_eq!(backend.leases.load(Ordering::SeqCst), 0);
    let act = native_reference::act(
        &backend,
        &reference,
        ToolCall {
            name: "click".into(),
            args: json!({}),
        },
    )
    .await;
    assert!(!act.is_error.unwrap_or(false));
    assert_eq!(backend.leases.load(Ordering::SeqCst), 0);
    let read = native_reference::read(&backend, &reference, json!({})).await;
    assert!(!read.is_error.unwrap_or(false));
    assert_eq!(backend.leases.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failed_resolution_refuses_both_operations_without_a_lease() {
    let backend = Backend {
        leases: Arc::new(AtomicUsize::new(0)),
    };
    let reference = Reference::from("expired-or-foreign".to_owned());
    assert!(native_reference::read(&backend, &reference, json!({}))
        .await
        .is_error
        .unwrap_or(false));
    assert!(native_reference::act(
        &backend,
        &reference,
        ToolCall {
            name: "click".into(),
            args: json!({})
        }
    )
    .await
    .is_error
    .unwrap_or(false));
    assert_eq!(backend.leases.load(Ordering::SeqCst), 0);
}

#[test]
fn reference_wire_value_is_opaque_and_has_no_window_or_control_variant() {
    let reference = Reference::from("backend-owned-window-or-control".to_owned());
    let wire = serde_json::to_value(&reference).unwrap();
    assert_eq!(wire, json!("backend-owned-window-or-control"));
    let restored: Reference = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, reference);
}
