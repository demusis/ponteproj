//! Asks GitHub, only when the person clicks, whether a newer Ponte exists.
//! Nothing here runs by itself: no check at startup, no periodic check.

/// What Ajustes ▸ Sobre o Ponte shows about the last check.
#[derive(Default)]
pub struct UpdateState {}
