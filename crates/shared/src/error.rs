use serde::{Deserialize, Serialize};
use std::fmt;

/// Códigos de error estables entre backend y cliente de escritorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    Validation,
    NotFound,
    Conflict,
    Unauthorized,
    Internal,
}

/// Cuerpo de error HTTP compartido sin filtrar detalles internos de infraestructura.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiErrorBody {
    pub code: ApiErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// Error puro de validación reutilizable en cliente y servidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: &'static str,
}

impl ValidationError {
    pub const fn new(field: &'static str, message: &'static str) -> Self {
        Self { field, message }
    }

    pub fn into_api_error(self) -> ApiErrorBody {
        ApiErrorBody {
            code: ApiErrorCode::Validation,
            message: self.message.to_owned(),
            field: Some(self.field.to_owned()),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}
