use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("vault already exists: {}", .0.display())]
    AlreadyExists(PathBuf),
    #[error("not a paddy vault (or unsupported schema version {0})")]
    UnsupportedVersion(i64),
    #[error("no such {what} with id {id}")]
    NotFound { what: &'static str, id: i64 },
    #[error("name can't be empty")]
    EmptyName,
    #[error("vault changed on disk (another paddy running?)")]
    ChangedOnDisk(PathBuf),
    #[error("this vault already has the maximum of {0} templates")]
    TooManyTemplates(usize),
    #[error("vault file is damaged or not a valid paddy vault")]
    Corrupt,
    #[error("vault file is larger than the {0} MB limit")]
    TooLarge(u64),
    #[error("vault has no file path yet")]
    NoPath,
}

pub type Result<T> = std::result::Result<T, Error>;
