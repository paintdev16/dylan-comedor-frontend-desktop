use crate::domain::{DinerId, DinerStatus, MealShift};
use serde::{Deserialize, Serialize};

/// Respuesta de salud del backend para verificación de conectividad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HealthResponse {
    pub service: String,
    pub version: String,
    pub status: String,
}

/// Contrato de entrada para registrar un comensal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreateDinerRequest {
    pub document_number: String,
    pub full_name: String,
}

/// Contrato de salida para representar un comensal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DinerResponse {
    pub id: DinerId,
    pub document_number: String,
    pub full_name: String,
    pub status: DinerStatus,
    #[serde(default)]
    pub has_fingerprint: bool,
}

/// Contrato de entrada para registrar una marcación de comida.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RegisterAttendanceRequest {
    pub diner_id: DinerId,
    pub shift: MealShift,
}

/// Contrato de salida de una marcación de comida registrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AttendanceRecordResponse {
    pub id: u64,
    pub diner_id: DinerId,
    pub diner_name: String,
    pub shift: MealShift,
    pub recorded_at_rfc3339: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diner_response_defaults_missing_optional_fields() {
        let raw =
            r#"{"id":7,"document_number":"12345678","full_name":"Ana Pérez","status":"active"}"#;
        let parsed: DinerResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.id, DinerId(7));
        assert_eq!(parsed.status, DinerStatus::Active);
        assert!(!parsed.has_fingerprint);
    }

    #[test]
    fn attendance_request_roundtrips_json() {
        let req = RegisterAttendanceRequest {
            diner_id: DinerId(42),
            shift: MealShift::Lunch,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: RegisterAttendanceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }
}
