use serde::{Deserialize, Serialize};
use std::fmt;
use crate::errors::DomainError;

// ── FilePath ─────────────────────────────────────────────────────────────────

/// Absolute path to a Rust source file.
/// Immutable once constructed; equality is byte-exact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FilePath(String);

impl FilePath {
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let s = raw.into();
        if s.is_empty() {
            return Err(DomainError::InvalidFilePath(s));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── ByteRange ────────────────────────────────────────────────────────────────

/// Half-open byte range `[start, end)` within a source file.
/// Invariant: `start < end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

impl ByteRange {
    pub fn new(start: u32, end: u32) -> Result<Self, DomainError> {
        if start >= end {
            return Err(DomainError::EmptySelectionRange);
        }
        Ok(Self { start, end })
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }
}

// ── FunctionName ─────────────────────────────────────────────────────────────

/// Valid Rust identifier used as the name for the extracted function.
/// Must be non-empty and a legal Rust identifier (basic check).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionName(String);

impl FunctionName {
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let s = raw.into();
        if s.is_empty() || !is_valid_rust_ident(&s) {
            return Err(DomainError::InvalidFunctionName(s));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FunctionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── OwnershipKind ────────────────────────────────────────────────────────────

/// How a value crossing the new function boundary should be passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OwnershipKind {
    /// Pass by value (move semantics).
    Owned,
    /// Pass as `&T` (shared/immutable reference).
    SharedRef,
    /// Pass as `&mut T` (exclusive mutable reference).
    MutRef,
}

// ── LifetimeParameter ────────────────────────────────────────────────────────

/// A named lifetime parameter such as `'a` or `'static`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LifetimeParameter(String);

impl LifetimeParameter {
    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let s = raw.into();
        if !s.starts_with('\'') || s.len() < 2 {
            return Err(DomainError::InvalidLifetimeParameter(s));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The well-known `'static` lifetime.
    pub fn static_lifetime() -> Self {
        Self("'static".into())
    }
}

impl fmt::Display for LifetimeParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── ControlFlowKind ──────────────────────────────────────────────────────────

/// Non-local control-flow statements that may appear in the extracted fragment
/// and require reification into an auxiliary enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlFlowKind {
    /// Early `return` from the enclosing function.
    Return,
    /// `break` targeting a loop outside the selected range.
    Break,
    /// `continue` targeting a loop outside the selected range.
    Continue,
    /// `?` / `Try` propagation across the boundary.
    Try,
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Lightweight Rust identifier validation (ASCII subset).
fn is_valid_rust_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}
