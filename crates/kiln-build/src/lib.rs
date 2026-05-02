//! Build pipeline and simulator backends for `kiln`.
//!
//! This crate is a stub at M0. It will host the source-set resolution,
//! build cache, and Verilator backend from M2 onward.

/// Stub marker function so the crate has at least one public symbol.
pub fn placeholder() -> &'static str {
    "kiln-build: not yet implemented"
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_is_stable() {
        assert_eq!(super::placeholder(), "kiln-build: not yet implemented");
    }
}
