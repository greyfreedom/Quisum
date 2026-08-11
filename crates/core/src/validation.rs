use crate::DomainError;

/// Domain types validate invariants after construction or deserialization.
pub trait Validate {
    /// Validate all invariants required before persistence or use.
    fn validate(&self) -> Result<(), DomainError>;
}

pub(crate) fn validate_text(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), DomainError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || value.chars().count() > maximum {
        return Err(DomainError::InvalidValue(field));
    }
    Ok(())
}

pub(crate) fn validate_topic(value: &str, field: &'static str) -> Result<(), DomainError> {
    validate_text(value, 80, field)?;
    if value.contains(['\r', '\n']) {
        return Err(DomainError::InvalidValue(field));
    }
    Ok(())
}

pub(crate) fn validate_string_list(
    values: &[String],
    maximum_items: usize,
    maximum_length: usize,
    field: &'static str,
) -> Result<(), DomainError> {
    if values.len() > maximum_items {
        return Err(DomainError::InvalidValue(field));
    }
    for value in values {
        validate_text(value, maximum_length, field)?;
    }
    Ok(())
}

pub(crate) fn validate_method_id(value: &str, field: &'static str) -> Result<(), DomainError> {
    let Some((name, version)) = value.split_once('@') else {
        return Err(DomainError::InvalidValue(field));
    };
    let name_valid = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    let mut version_parts = version.split('.');
    let version_valid = (0..3).all(|_| {
        version_parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && version_parts.next().is_none();
    if value.len() > 160 || !name_valid || !version_valid {
        return Err(DomainError::InvalidValue(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_limits_count_unicode_characters_and_raw_padding() {
        assert!(validate_text(&"界".repeat(4_000), 4_000, "test.text").is_ok());
        assert_eq!(
            validate_text(&format!("x{}", " ".repeat(4_000)), 4_000, "test.text"),
            Err(DomainError::InvalidValue("test.text"))
        );
    }

    #[test]
    fn method_identifier_matches_the_public_schema() {
        assert!(validate_method_id("event-debrief@1.0.0", "test.method").is_ok());
        assert_eq!(
            validate_method_id("Event Debrief latest", "test.method"),
            Err(DomainError::InvalidValue("test.method"))
        );
    }
}
