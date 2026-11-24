//! The adoption mapping of a mapped library (F3.0b of Frente F;
//! `desenho-f3-v3-adendo.md` §4 and §7, `desenho-f3-v2.md` §2.3 and §2.8).
//!
//! A kind is mapped on this device when it is in [`ADDRESSED`]: the wire is
//! the same for every F3 build (copy numbers plus proofs), and only the
//! mapper reads the list (v2 §2.4). It stays empty until the families map
//! their kinds, each with the references to it (F3.1a onwards), so every
//! adoption is still verbatim.

/// The ident kinds the mapper maps on this device (none yet: F3.1a adds
/// the first ones).
pub(crate) const ADDRESSED: &[&str] = &[];

/// Whether `kind` (an ident kind) is mapped on this device.
pub(crate) fn addressed(kind: &str) -> bool {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(kinds) = hooks::ADDRESSED.with(|k| k.borrow().clone()) {
        return kinds.contains(&kind);
    }
    ADDRESSED.contains(&kind)
}

/// The kind set of the tests of the mapper, before any family maps one.
#[cfg(any(test, feature = "test-support"))]
pub mod hooks {
    thread_local! {
        /// Replaces [`super::ADDRESSED`] on this thread.
        pub static ADDRESSED: std::cell::RefCell<Option<Vec<&'static str>>> =
            const { std::cell::RefCell::new(None) };
    }
}
