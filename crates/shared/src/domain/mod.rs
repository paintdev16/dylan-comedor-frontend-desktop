use serde::{Deserialize, Serialize};

/// Identificador tipado de un comensal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DinerId(pub u64);

/// Estado operativo de un comensal dentro del comedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DinerStatus {
    Active,
    Inactive,
}

/// Turnos de comida habilitados en el comedor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MealShift {
    Breakfast,
    Lunch,
    Dinner,
}
