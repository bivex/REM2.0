/// Adapter: OS filesystem → `FileSystemPort`

use rem_domain::{errors::DomainError, ports::filesystem::FileSystemPort};

pub struct OsFileSystemAdapter;

impl FileSystemPort for OsFileSystemAdapter {
    fn read_to_string(&self, path: &str) -> Result<String, DomainError> {
        std::fs::read_to_string(path)
            .map_err(|e| DomainError::InvalidFilePath(format!("read `{path}`: {e}")))
    }

    fn write_string(&self, path: &str, content: &str) -> Result<(), DomainError> {
        std::fs::write(path, content)
            .map_err(|e| DomainError::InvalidFilePath(format!("write `{path}`: {e}")))
    }

    fn path_exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn find_cargo_toml(&self, start_path: &str) -> Result<String, DomainError> {
        let mut dir = std::path::Path::new(start_path);
        // If start_path is a file, begin at its parent.
        if dir.is_file() {
            dir = dir.parent().unwrap_or(dir);
        }
        loop {
            let candidate = dir.join("Cargo.toml");
            if candidate.exists() {
                return Ok(dir.to_string_lossy().into_owned());
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => return Err(DomainError::InvalidFilePath(format!(
                    "no Cargo.toml found above `{start_path}`"
                ))),
            }
        }
    }
}
