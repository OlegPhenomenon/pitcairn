use std::collections::HashMap;

use crate::error::AppError;

/// Accumulates per-field validation errors and converts to
/// `AppError::Validation { fields }` (HTTP 422) when non-empty.
#[derive(Default)]
pub struct FieldErrors {
    fields: HashMap<String, String>,
}

impl FieldErrors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&mut self, field: &str, ok: bool, message: &str) {
        if !ok {
            self.fields.insert(field.into(), message.into());
        }
    }

    pub fn require(&mut self, field: &str, value: &str, message: &str) {
        self.check(field, !value.trim().is_empty(), message);
    }

    pub fn max_len(&mut self, field: &str, value: &str, max: usize) {
        self.check(field, value.len() <= max, &format!("must be at most {max} characters"));
    }

    pub fn valid_date(&mut self, field: &str, value: &str) {
        let ok = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok();
        self.check(field, ok, "must be a date YYYY-MM-DD");
    }

    pub fn finish(self) -> Result<(), AppError> {
        if self.fields.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation { fields: self.fields })
        }
    }
}
