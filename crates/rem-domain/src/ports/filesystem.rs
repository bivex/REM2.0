use crate::errors::DomainError;

/// **Port**: file-system access — all file I/O goes through here so that
/// domain / application logic stays pure and testable with in-memory fakes.
///
/// Implementors: `rem-infrastructure::adapters::filesystem`.
pub trait FileSystemPort: Send + Sync {
    fn read_to_string(&self, path: &str) -> Result<String, DomainError>;
    fn write_string(&self, path: &str, content: &str) -> Result<(), DomainError>;
    fn path_exists(&self, path: &str) -> bool;
    /// Return the absolute path to the `Cargo.toml` nearest to `start_path`.
    fn find_cargo_toml(&self, start_path: &str) -> Result<String, DomainError>;
}
