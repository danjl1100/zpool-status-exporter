//! Authentication rules for access to web resources

// Auth prefix clarifies types for parent module
#![allow(clippy::module_name_repetitions)]

pub(crate) use file::Error as FileError;
use std::sync::OnceLock;

static HEADER_AUTHORIZATION: OnceLock<tiny_http::HeaderField> = OnceLock::new();
fn get_header_authorization() -> &'static tiny_http::HeaderField {
    HEADER_AUTHORIZATION
        .get_or_init(|| tiny_http::HeaderField::from_bytes("Authorization").expect("ascii"))
}

static HEADER_AUTHENTICATE: OnceLock<tiny_http::Header> = OnceLock::new();
#[allow(clippy::missing_panics_doc)]
pub(crate) fn get_header_www_authenticate() -> tiny_http::Header {
    HEADER_AUTHENTICATE
        .get_or_init(|| {
            let field = tiny_http::HeaderField::from_bytes("WWW-Authenticate").expect("ascii");
            let value = ascii::AsciiString::from_ascii("Basic").expect("ascii");
            tiny_http::Header { field, value }
        })
        .clone()
}

/// Configuration for authentication rules
pub(crate) struct AuthRules {
    entries_sorted: Box<[String]>,
}
mod file {
    use super::AuthRules;

    impl AuthRules {
        /// Attempt to construct rules from a plaintext file
        ///
        /// # Errors
        /// Returns an error if the file IO fails
        pub fn from_file(file: impl AsRef<std::path::Path>) -> Result<Self, Error> {
            let file = file.as_ref();
            let make_error = |kind| Error {
                file: file.to_owned(),
                kind,
            };

            let content = std::fs::read_to_string(file)
                .map_err(ErrorKind::IO)
                .map_err(make_error)?;
            let lines = content.lines().map(String::from);
            Self::from_entries(lines)
                .ok_or(ErrorKind::NoEntries)
                .map_err(make_error)
        }
        /// Constructs rules from the specified entries
        ///
        /// Returns `None` if no entries are specified
        pub fn from_entries(entries: impl Iterator<Item = String>) -> Option<Self> {
            let entries_sorted = {
                let mut entries: Vec<_> = entries.collect();
                entries.sort();
                entries.into_boxed_slice()
            };
            (!entries_sorted.is_empty()).then_some(Self { entries_sorted })
        }
    }
    #[derive(Debug)]
    pub(crate) struct Error {
        file: std::path::PathBuf,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        IO(std::io::Error),
        NoEntries,
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::IO(error) => Some(error),
                ErrorKind::NoEntries => None,
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { file, kind } = self;
            let description = match kind {
                ErrorKind::IO(_error) => "failed to read",
                ErrorKind::NoEntries => "no entries in",
            };
            write!(f, "{description} file {}", file.display())
        }
    }
}

mod header {
    use super::{AuthResult, AuthRules, DebugUserString, get_header_authorization};
    use base64::Engine;

    impl AuthRules {
        /// Prints startup message(s) to stdout
        pub fn print_start_message(&self) {
            let count = self.entries_sorted.len();
            let entry_plural = if count == 1 { "entry" } else { "entries" };
            println!("Allow-list configured with {count} {entry_plural}");
            println!(
                "!!!!!! WARNING: HTTP transmits authentication in plaintext, use a HTTPS-proxy on the local machine!!!!!!!"
            );
        }
        /// Evalutes the request against the rules
        ///
        /// # Errors
        ///
        /// Returns an error when the "Authorization" header is present, but does not contain a valid
        /// UTF-8 authentication string
        pub(crate) fn query(&self, request: &tiny_http::Request) -> Result<AuthResult, Error> {
            let header_authorization = get_header_authorization();
            let Some(auth_value) = request
                .headers()
                .iter()
                .find(|header| header.field == *header_authorization)
                .map(|header| header.value.clone())
            else {
                return Ok(AuthResult::MissingAuthHeader);
            };

            let auth_str = parse_authorization_value(auth_value.as_str())?;

            if self.entries_sorted.binary_search(&auth_str).is_ok() {
                Ok(AuthResult::Accept)
            } else {
                let who = DebugUserString::from(auth_str);

                Ok(AuthResult::Deny(who))
            }
        }
    }

    fn parse_authorization_value(auth_value: &str) -> Result<String, Error> {
        const BASIC_PREFIX: &str = "Basic ";

        let make_error = |kind| Error {
            auth_value: auth_value.to_owned().into(),
            kind,
        };

        let auth_base64 = auth_value
            .strip_prefix(BASIC_PREFIX)
            .ok_or(ErrorKind::MissingBasic)
            .map_err(make_error)?;

        let auth_bytes = base64::prelude::BASE64_STANDARD
            .decode(auth_base64)
            .map_err(ErrorKind::Base64)
            .map_err(make_error)?;

        let auth_str = String::from_utf8(auth_bytes)
            .map_err(ErrorKind::Utf8)
            .map_err(make_error)?;

        Ok(auth_str)
    }

    #[derive(Debug)]
    pub(crate) struct Error {
        auth_value: DebugUserString,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        MissingBasic,
        Base64(base64::DecodeError),
        Utf8(std::string::FromUtf8Error),
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::MissingBasic => None,
                ErrorKind::Base64(error) => Some(error),
                ErrorKind::Utf8(error) => Some(error),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { auth_value, kind } = self;
            let description = match kind {
                ErrorKind::MissingBasic => "missing basic-authentication prefix",
                ErrorKind::Base64(_error) => "invalid base64",
                ErrorKind::Utf8(_error) => "non-UTF8 string",
            };
            write!(
                f,
                "{description} in authorization header value: {auth_value:?}"
            )
        }
    }
}

/// Result of parsing a request
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum AuthResult {
    /// Provided authentication matches allow list rules
    Accept,
    /// Provided authentication failed the rules
    Deny(DebugUserString),
    /// No authentication provided
    MissingAuthHeader,
    /// No rules configured
    NoneConfigured,
}

pub(crate) use debug_user_string::{DebugUserString, DebugUserStringRef};
mod debug_user_string {
    const MAX_LEN: usize = 80;

    /// Trace of authorization contents (of a maximum length)
    #[allow(missing_docs)]
    #[derive(Clone, PartialEq, Eq)]
    pub(crate) enum DebugUserString {
        Unchanged { value: Box<str> },
        Truncated { value: Box<str>, orig_len: usize },
    }
    impl From<String> for DebugUserString {
        fn from(mut value: String) -> Self {
            let orig_len = value.len();
            if orig_len > MAX_LEN {
                value.truncate(MAX_LEN);

                let value = value.into_boxed_str();
                Self::Truncated { value, orig_len }
            } else {
                let value = value.into_boxed_str();
                Self::Unchanged { value }
            }
        }
    }
    impl<'a> From<&'a str> for DebugUserStringRef<'a> {
        fn from(value: &'a str) -> Self {
            let orig_len = value.len();
            if orig_len > MAX_LEN {
                let value = &value[..MAX_LEN];

                Self::Truncated { value, orig_len }
            } else {
                Self::Unchanged { value }
            }
        }
    }
    /// Trace of authorization contents (of a maximum length)
    #[allow(missing_docs)]
    #[derive(Clone, PartialEq, Eq)]
    pub enum DebugUserStringRef<'a> {
        Unchanged { value: &'a str },
        Truncated { value: &'a str, orig_len: usize },
    }
    impl std::fmt::Display for DebugUserStringRef<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Unchanged { value } => {
                    write!(f, "{value:?}") //
                }
                Self::Truncated { value, orig_len } => {
                    write!(f, "{value:?}... (len {orig_len})")
                }
            }
        }
    }

    impl<'a> From<&'a DebugUserString> for DebugUserStringRef<'a> {
        fn from(value: &'a DebugUserString) -> Self {
            match *value {
                DebugUserString::Unchanged { ref value } => DebugUserStringRef::Unchanged { value },
                DebugUserString::Truncated {
                    ref value,
                    orig_len,
                } => DebugUserStringRef::Truncated { value, orig_len },
            }
        }
    }

    // delegate impls

    impl std::fmt::Display for DebugUserString {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let this = self.into();
            <DebugUserStringRef as std::fmt::Display>::fmt(&this, f)
        }
    }

    impl std::fmt::Debug for DebugUserString {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            <Self as std::fmt::Display>::fmt(self, f)
        }
    }
    impl std::fmt::Debug for DebugUserStringRef<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            <Self as std::fmt::Display>::fmt(self, f)
        }
    }
}
