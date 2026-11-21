use crate::{dto::CreateDinerRequest, error::ValidationError};

/// Valida un número de documento (DNI/carnet): 8 a 15 dígitos alfanuméricos sin espacios.
pub fn validate_document_number(raw: &str) -> Result<String, ValidationError> {
    let trimmed = raw.trim();
    if trimmed.len() < 8
        || trimmed.len() > 15
        || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(ValidationError::new(
            "document_number",
            "El documento debe tener entre 8 y 15 caracteres alfanuméricos.",
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Valida el nombre completo de un comensal.
pub fn validate_full_name(raw: &str) -> Result<String, ValidationError> {
    let trimmed = raw.trim();
    if trimmed.chars().count() < 2 || trimmed.chars().count() > 120 {
        return Err(ValidationError::new(
            "full_name",
            "El nombre completo debe tener entre 2 y 120 caracteres.",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Valida una solicitud completa de registro de comensal.
pub fn validate_create_diner(
    req: &CreateDinerRequest,
) -> Result<CreateDinerRequest, ValidationError> {
    Ok(CreateDinerRequest {
        document_number: validate_document_number(&req.document_number)?,
        full_name: validate_full_name(&req.full_name)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_normalizes_create_diner() {
        let req = CreateDinerRequest {
            document_number: " 12345678 ".into(),
            full_name: "  María Gómez  ".into(),
        };
        let valid = validate_create_diner(&req).unwrap();
        assert_eq!(valid.document_number, "12345678");
        assert_eq!(valid.full_name, "María Gómez");
    }

    #[test]
    fn rejects_invalid_document_or_name() {
        assert_eq!(
            validate_document_number("123").unwrap_err().field,
            "document_number"
        );
        assert_eq!(
            validate_document_number("1234-5678").unwrap_err().field,
            "document_number"
        );
        assert_eq!(validate_full_name(" ").unwrap_err().field, "full_name");
    }
}
