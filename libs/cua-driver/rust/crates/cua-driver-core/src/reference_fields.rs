//! Optional field comparison for native backends. The token envelope stays opaque.
//! A missing value (JSON null) is known; a failed read is tracked separately.
use serde_json::Value;

/// Native capability reads failed: this candidate might be actionable.
pub const ACTIONABILITY_UNKNOWN: u16 = 1 << 15;

pub fn read<T: Default, E>(result: Result<T, E>, unknown: &mut u16, field: u16) -> T {
    result.unwrap_or_else(|_| {
        *unknown |= field;
        T::default()
    })
}

pub fn encode(kind: &str, fields: Value, unknown: u16) -> Vec<u8> {
    serde_json::to_vec(&(kind, fields, unknown & !ACTIONABILITY_UNKNOWN))
        .expect("native reference fields")
}

pub struct ReferenceFields {
    fields: Vec<Value>,
    unknown: u16,
}

impl ReferenceFields {
    pub fn decode(reference: &[u8], kind: &str, count: usize) -> Result<Self, String> {
        let (version, fields, unknown): (String, Vec<Value>, u16) =
            serde_json::from_slice(reference).map_err(|_| "invalid native reference")?;
        if version != kind || fields.len() != count || count >= 15 || unknown >> count != 0 {
            return Err("unsupported native reference".into());
        }
        Ok(Self { fields, unknown })
    }

    pub fn matches(&self, fields: Value, unknown: u16) -> Result<bool, String> {
        let fields = fields.as_array().ok_or("invalid native candidate")?;
        if fields.len() != self.fields.len() {
            return Err("invalid native candidate".into());
        }
        let uncertain = self.unknown | unknown;
        // A known difference rules out a candidate even if another field failed.
        if fields
            .iter()
            .zip(&self.fields)
            .enumerate()
            .any(|(i, (current, observed))| uncertain & (1 << i) == 0 && current != observed)
        {
            return Ok(false);
        }
        if uncertain != 0 {
            return Err(
                "native reference cannot be validated: matching candidate has unreadable fields"
                    .into(),
            );
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn irrelevant_error_does_not_block_but_possible_match_does() {
        let token = encode("test2", json!(["button", "Save"]), 0);
        let target = ReferenceFields::decode(&token, "test2", 2).unwrap();
        assert!(!target.matches(json!(["text area", null]), 2).unwrap());
        assert!(target.matches(json!(["button", null]), 2).is_err());
        assert!(target.matches(json!(["button", "Save"]), 0).unwrap());
        assert!(target
            .matches(json!(["button", "Save"]), ACTIONABILITY_UNKNOWN)
            .is_err());
    }

    #[test]
    fn absence_and_observed_read_errors_are_not_conflated() {
        let absent =
            ReferenceFields::decode(&encode("test2", json!(["button", null]), 0), "test2", 2)
                .unwrap();
        assert!(absent.matches(json!(["button", null]), 0).unwrap());
        assert!(absent.matches(json!(["button", null]), 2).is_err());
        let unreadable =
            ReferenceFields::decode(&encode("test2", json!(["button", null]), 2), "test2", 2)
                .unwrap();
        assert!(unreadable.matches(json!(["button", "Save"]), 0).is_err());
        assert!(!unreadable.matches(json!(["text area", "Save"]), 0).unwrap());
    }

    #[test]
    fn old_payloads_and_other_backend_versions_are_rejected() {
        assert!(ReferenceFields::decode(br#"["button","Save"]"#, "test2", 2).is_err());
        assert!(ReferenceFields::decode(
            &encode("other2", json!(["button", "Save"]), 0),
            "test2",
            2
        )
        .is_err());
    }
}
