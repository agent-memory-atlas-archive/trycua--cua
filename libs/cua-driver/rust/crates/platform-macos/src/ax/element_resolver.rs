use super::bindings::AXUIElementRef;
use super::tree::AXNode;
use core_foundation::base::{CFRelease, CFRetain, CFTypeRef};
use cua_driver_core::element_token::{self, ResolvedElement};
use cua_driver_core::protocol::ToolResult;
pub struct RetainedElement(usize);
impl RetainedElement {
    pub fn as_ptr(&self) -> usize {
        self.0
    }
    pub unsafe fn retain(ptr: usize) -> Self {
        if ptr != 0 {
            unsafe { CFRetain(ptr as AXUIElementRef as CFTypeRef) };
        }
        Self(ptr)
    }
}
impl Clone for RetainedElement {
    fn clone(&self) -> Self {
        unsafe { Self::retain(self.0) }
    }
}
impl Drop for RetainedElement {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { CFRelease(self.0 as AXUIElementRef as CFTypeRef) };
        }
    }
}
pub struct FreshAxElements {
    pub elements: Vec<usize>,
}
impl FreshAxElements {
    pub fn from_nodes(nodes: &[AXNode]) -> Self {
        Self {
            elements: nodes
                .iter()
                .filter(|n| n.element_index.is_some())
                .map(|n| n.element_ptr)
                .collect(),
        }
    }
    fn retain_element(&self, index: usize) -> Option<RetainedElement> {
        self.elements
            .get(index)
            .filter(|p| **p != 0)
            .map(|p| unsafe { RetainedElement::retain(*p) })
    }
}
impl Drop for FreshAxElements {
    fn drop(&mut self) {
        for ptr in &self.elements {
            if *ptr != 0 {
                unsafe { CFRelease(*ptr as AXUIElementRef as CFTypeRef) };
            }
        }
    }
}
fn fields(n: &AXNode) -> serde_json::Value {
    serde_json::json!([
        n.role,
        n.title,
        n.description,
        n.identifier,
        n.actions,
        n.depth,
        n.in_web_content
    ])
}
pub fn reference_for_node(n: &AXNode) -> Vec<u8> {
    cua_driver_core::reference_fields::encode("ax2", fields(n), n.reference_unknown)
}
pub async fn resolve_element_args(
    pid: i32,
    element_index: Option<usize>,
    element_token: Option<&str>,
    snapshot_id: Option<&str>,
    window_id: Option<u64>,
    tool: &str,
) -> Result<ResolvedElement<RetainedElement>, ToolResult> {
    element_token::resolve_native(
        pid,
        element_index,
        element_token,
        snapshot_id,
        window_id,
        tool,
        move |w, t| resolve_fresh(pid, w, t),
    )
    .await
}
pub(crate) fn resolve_fresh(
    pid: i32,
    w: u64,
    reference: &[u8],
) -> Result<Option<RetainedElement>, String> {
    let w =
        u32::try_from(w).map_err(|_| format!("window_id {w} is not a valid macOS window id"))?;
    let tree = super::tree::walk_tree(pid, Some(w), None);
    let payload = FreshAxElements::from_nodes(&tree.nodes);
    let matched = resolve_nodes(reference, &tree.nodes, !tree.truncated)?;
    Ok(matched.and_then(|i| payload.retain_element(i)))
}
// Matching policy belongs to AX, not to the authenticated token envelope.
fn resolve_nodes(
    reference: &[u8],
    nodes: &[AXNode],
    complete: bool,
) -> Result<Option<usize>, String> {
    if !complete {
        return Err("incomplete accessibility tree cannot establish a unique element".into());
    }
    use cua_driver_core::reference_fields::{ReferenceFields, ACTIONABILITY_UNKNOWN};
    let target = ReferenceFields::decode(reference, "ax2", 7)?;
    let mut matched = None;
    for node in nodes {
        if node.element_index.is_none()
            && (node.enabled == Some(false) || node.reference_unknown & ACTIONABILITY_UNKNOWN == 0)
        {
            continue;
        }
        if target.matches(fields(node), node.reference_unknown)? {
            if matched.is_some() {
                return Ok(None);
            }
            matched = node.element_index;
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(index: usize, title: &str) -> AXNode {
        AXNode {
            reference_unknown: 0,
            element_index: Some(index),
            role: "AXButton".into(),
            title: Some(title.into()),
            value: None,
            description: None,
            identifier: None,
            help: None,
            actions: vec!["AXPress".into()],
            element_ptr: 0,
            depth: 0,
            parent_element_index: None,
            frame: None,
            value_state: None,
            value_description: None,
            min_value: None,
            max_value: None,
            enabled: Some(true),
            selected: None,
            in_web_content: false,
        }
    }

    #[test]
    fn unrelated_description_error_does_not_block_button_reference() {
        let button = node(0, "Save");
        let reference = reference_for_node(&button);
        let mut other = node(1, "Save");
        other.role = "AXTextArea".into();
        other.reference_unknown = 1 << 2; // AXDescription failed, not absent.
        assert_eq!(
            resolve_nodes(&reference, &[button.clone(), other], true).unwrap(),
            Some(0)
        );
        let mut possible_duplicate = button.clone();
        possible_duplicate.reference_unknown = 1 << 2;
        assert!(resolve_nodes(&reference, &[button.clone(), possible_duplicate], true).is_err());
        let mut unknown_actionability = button.clone();
        unknown_actionability.element_index = None;
        unknown_actionability.reference_unknown =
            cua_driver_core::reference_fields::ACTIONABILITY_UNKNOWN;
        assert!(resolve_nodes(
            &reference,
            &[button.clone(), unknown_actionability.clone()],
            true
        )
        .is_err());
        unknown_actionability.enabled = Some(false);
        assert_eq!(
            resolve_nodes(&reference, &[button.clone(), unknown_actionability], true).unwrap(),
            Some(0)
        );
        let mut unreadable_observation = button.clone();
        unreadable_observation.reference_unknown = 1 << 2;
        assert!(resolve_nodes(
            &reference_for_node(&unreadable_observation),
            &[button],
            true
        )
        .is_err());
    }

    #[test]
    fn ax_reference_requires_one_complete_current_match() {
        let reference = reference_for_node(&node(0, "Save"));
        assert_eq!(
            resolve_nodes(&reference, &[node(9, "Save")], true).unwrap(),
            Some(9)
        );
        assert_eq!(
            resolve_nodes(&reference, &[node(0, "Delete")], true).unwrap(),
            None
        );
        assert_eq!(
            resolve_nodes(&reference, &[node(0, "Save"), node(1, "Save")], true).unwrap(),
            None
        );
        assert!(resolve_nodes(&reference, &[node(0, "Save")], false).is_err());
        let mut web_node = node(0, "Save");
        web_node.in_web_content = true;
        assert_eq!(resolve_nodes(&reference, &[web_node], true).unwrap(), None);
    }

    #[test]
    fn empty_projection_has_no_element() {
        assert!(FreshAxElements::from_nodes(&[]).retain_element(0).is_none());
    }
}
