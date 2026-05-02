//! Pure-Rust subprocess wrapper around the `slang` SystemVerilog compiler CLI.
//!
//! At M0 this crate is a stub: it declares the public surface that M1 will
//! fill in, but contains no real logic yet. See `kiln-milestones.md` M1 and
//! `docs/decisions/0001-slang-integration-strategy.md` (added in M1) for the
//! design.

/// Stub returned in place of a real `Slang` handle.
pub fn placeholder() -> &'static str {
    "slang-rs: not yet implemented"
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_is_stable() {
        assert_eq!(super::placeholder(), "slang-rs: not yet implemented");
    }
}
