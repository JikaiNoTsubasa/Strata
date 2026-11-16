// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory secret (password) handling.

use std::fmt;

use zeroize::Zeroizing;

/// A password held in memory.
///
/// The buffer is wiped when dropped, and the value never appears in `Debug`
/// output (hence never in logs). There is deliberately no `Display` and no
/// serialisation.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wraps a password.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Exposes the password, to be handed to a driver or the keyring only.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the password is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_reveals_the_value() {
        let secret = Secret::new("hunter2");
        assert_eq!(format!("{secret:?}"), "Secret(***)");
        assert_eq!(secret.expose(), "hunter2");
    }
}
