use super::{AtspiIdentity, AtspiNode};
use cua_driver_core::element_token::{self, ResolvedElement};
use cua_driver_core::protocol::ToolResult;
fn fields(node: &AtspiNode) -> serde_json::Value {
    serde_json::json!([
        node.role,
        node.name,
        node.description,
        node.actions,
        node.depth,
        node.in_web_content
    ])
}
pub fn reference_for_node(node: &AtspiNode) -> Vec<u8> {
    cua_driver_core::reference_fields::encode("atspi2", fields(node), node.reference_unknown)
}
pub async fn resolve_element_args(
    pid: i32,
    element_index: Option<usize>,
    element_token: Option<&str>,
    snapshot_id: Option<&str>,
    window_id: Option<u64>,
    tool_name: &str,
) -> Result<ResolvedElement<AtspiIdentity>, ToolResult> {
    match element_token::resolve_native(
        pid,
        element_index,
        element_token,
        snapshot_id,
        window_id,
        tool_name,
        move |window, target| resolve_fresh(pid, window, target),
    )
    .await?
    {
        ResolvedElement::Element {
            window_id,
            via_token,
            element: (element_index, element),
            ..
        } => Ok(ResolvedElement::Element {
            window_id,
            element_index,
            via_token,
            element,
        }),
        ResolvedElement::None => Ok(ResolvedElement::None),
    }
}
pub(crate) fn resolve_fresh(
    pid: i32,
    window_id: u64,
    reference: &[u8],
) -> Result<Option<(usize, AtspiIdentity)>, String> {
    let tree = super::walk_tree(pid as u32, window_id, None);
    if !tree.trusted {
        return Err("current accessibility state is unavailable from AT-SPI".into());
    }
    if !tree.window_scoped {
        return Err(format!(
            "current accessibility state is not scoped to window_id {window_id}"
        ));
    }
    resolve_nodes(reference, tree.nodes, tree.complete)
}

fn resolve_nodes(
    reference: &[u8],
    nodes: Vec<AtspiNode>,
    complete: bool,
) -> Result<Option<(usize, AtspiIdentity)>, String> {
    if !complete {
        return Err("incomplete accessibility tree cannot establish a unique element".into());
    }
    use cua_driver_core::reference_fields::{ReferenceFields, ACTIONABILITY_UNKNOWN};
    let target = ReferenceFields::decode(reference, "atspi2", 6)?;
    let mut matched = None;
    for node in nodes {
        if node.element_index.is_none() && node.reference_unknown & ACTIONABILITY_UNKNOWN == 0 {
            continue;
        }
        if target.matches(fields(&node), node.reference_unknown)? {
            if matched.is_some() {
                return Ok(None);
            }
            matched = node.element_index.map(|index| (index, node.identity));
        }
    }
    match matched {
        None => Ok(None),
        Some((_, None)) => Err("matched element has no native AT-SPI identity".into()),
        Some((index, Some(identity))) => Ok(Some((index, identity))),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(i: usize, name: &str) -> AtspiNode {
        AtspiNode {
            reference_unknown: 0,
            element_index: Some(i),
            role: "button".into(),
            name: Some(name.into()),
            value: None,
            checked: None,
            enabled: Some(true),
            selected: None,
            description: None,
            actions: vec!["click".into()],
            element_key: i as u64,
            identity: None,
            depth: 0,
            parent_element_index: None,
            in_web_content: false,
        }
    }
    #[test]
    fn unrelated_metadata_error_does_not_block_atspi_reference() {
        let mut button = node(0, "Save");
        button.identity = Some(AtspiIdentity {
            bus_name: ":1.42".into(),
            path: "/button".into(),
            frame_bus_name: ":1.42".into(),
            frame_path: "/window".into(),
        });
        let reference = reference_for_node(&button);
        let mut other = node(1, "Save");
        other.role = "text".into();
        other.reference_unknown = 1 << 1; // Name failed.
        assert_eq!(
            resolve_nodes(&reference, vec![other, button.clone()], true)
                .unwrap()
                .unwrap()
                .0,
            0
        );
        let mut possible_duplicate = button.clone();
        possible_duplicate.reference_unknown = 1 << 1;
        assert!(resolve_nodes(&reference, vec![button.clone(), possible_duplicate], true).is_err());
        let mut uncertain = button.clone();
        uncertain.element_index = None;
        uncertain.reference_unknown = cua_driver_core::reference_fields::ACTIONABILITY_UNKNOWN;
        assert!(resolve_nodes(&reference, vec![button, uncertain], true).is_err());
    }

    #[test]
    fn atspi_reference_requires_one_complete_current_match() {
        let reference = reference_for_node(&node(0, "Save"));
        let identity = AtspiIdentity {
            bus_name: ":1.42".into(),
            path: "/button".into(),
            frame_bus_name: ":1.42".into(),
            frame_path: "/window".into(),
        };
        let mut matched = node(9, "Save");
        matched.identity = Some(identity.clone());
        assert_eq!(
            resolve_nodes(&reference, vec![matched.clone()], true).unwrap(),
            Some((9, identity))
        );
        assert_eq!(
            resolve_nodes(&reference, vec![node(0, "Delete")], true).unwrap(),
            None
        );
        // Missing native identity must not hide a duplicate description.
        assert_eq!(
            resolve_nodes(&reference, vec![node(0, "Save"), matched], true).unwrap(),
            None
        );
        assert_eq!(
            resolve_nodes(&reference, vec![node(0, "Save")], true).unwrap_err(),
            "matched element has no native AT-SPI identity"
        );
        assert!(resolve_nodes(&reference, vec![node(0, "Save")], false).is_err());
    }

    #[test]
    fn semantic_identity_ignores_traversal_index() {
        assert_eq!(
            reference_for_node(&node(1, "Save")),
            reference_for_node(&node(9, "Save"))
        );
        assert_ne!(
            reference_for_node(&node(1, "Save")),
            reference_for_node(&node(1, "Cancel"))
        );
    }
}
