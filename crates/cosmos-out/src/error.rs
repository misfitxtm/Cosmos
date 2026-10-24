// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum OutError {
    BadName,
    /// Something is already there and `force` was not given.
    Exists(PathBuf),
    /// A path that must be a real directory or file is a symlink or something else.
    Unsafe(PathBuf),
    BadBase(String),
    Template(String),
    Io(std::io::Error),
}

impl fmt::Display for OutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutError::BadName => f.write_str(
                "theme names are 1-64 characters: letters, digits, space, . _ -, starting with a letter or digit",
            ),
            OutError::Exists(p) => write!(f, "{} already exists (use --force to replace it)", p.display()),
            OutError::Unsafe(p) => write!(f, "refusing {}: not a plain file or directory", p.display()),
            OutError::BadBase(why) => write!(f, "base theme rejected: {why}"),
            OutError::Template(why) => write!(f, "template error: {why}"),
            OutError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for OutError {}

impl From<std::io::Error> for OutError {
    fn from(e: std::io::Error) -> Self {
        OutError::Io(e)
    }
}
