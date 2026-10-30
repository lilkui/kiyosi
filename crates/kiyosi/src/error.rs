use std::{borrow::Cow, fmt, io};

/// Describes an input rejection, computation failure or native I/O failure.
///
/// Variants identify failures that callers can handle differently; diagnostic
/// messages are for humans. Errors own their data and are `Send + Sync`.
/// Native I/O causes remain available through [`Self::Io`] and
/// [`std::error::Error::source`]. Internal panics and allocator failures retain
/// their native behavior.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{Date, Error};
///
/// assert!(matches!(Date::new(2025, 2, 29), Err(Error::InvalidInput(_))));
/// ```
#[derive(Debug)]
pub enum Error {
    /// Rejects invalid parameters, dates, schedules or contract states.
    ///
    /// See [`Error`] for an example.
    InvalidInput(Cow<'static, str>),
    /// Rejects an unsupported operation or an unavailable result measure.
    ///
    /// See [`crate::PricingResult::require`].
    UnsupportedOperation(Cow<'static, str>),
    /// Reports invalid numerical results or unavailable computation state.
    ///
    /// See [`crate::MonteCarloVanillaEngine::price`].
    ComputationFailure(Cow<'static, str>),
    /// Reports a supplied interval that does not bracket a root in the engine's domain.
    ///
    /// See [`crate::implied_volatility`].
    RootNotBracketed(Cow<'static, str>),
    /// Reports an iteration or depth limit reached without numerical convergence.
    ///
    /// See [`crate::implied_volatility`].
    NonConvergence(Cow<'static, str>),
    /// Preserves a native I/O failure, including entropy-source or thread-creation errors.
    ///
    /// See [`crate::MonteCarloVanillaEngine::price`].
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message)
            | Self::UnsupportedOperation(message)
            | Self::ComputationFailure(message)
            | Self::RootNotBracketed(message)
            | Self::NonConvergence(message) => f.write_str(message),
            Self::Io(_) => f.write_str("I/O operation failed"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Represents a value, a domain rejection or a reported operational failure.
///
/// Native I/O details remain available through [`Error::Io`] and the standard
/// error source. This alias does not guarantee recovery from allocator failures
/// or internal panics. See [`crate::MonteCarloVanillaEngine::price`] for an example.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn finite(value: f64, positive: bool) -> Result<()> {
    if !value.is_finite() || (positive && value <= 0.0) {
        return Err(Error::InvalidInput(
            "value must be finite and in its valid range".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn domain_variants_preserve_diagnostics() {
        let message = "invalid contract";
        let errors = [
            Error::InvalidInput(message.into()),
            Error::UnsupportedOperation(message.to_owned().into()),
            Error::ComputationFailure(message.into()),
            Error::RootNotBracketed(message.into()),
            Error::NonConvergence(message.into()),
        ];
        for error in errors {
            assert_eq!(error.to_string(), message);
            assert!(error.source().is_none());
        }
    }

    #[test]
    fn io_error_preserves_native_source_and_os_code() {
        let native = io::Error::from_raw_os_error(13);
        let kind = native.kind();
        let error = Error::from(native);
        let source = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
        assert_eq!(source.kind(), kind);
        assert_eq!(source.raw_os_error(), Some(13));
        let Error::Io(native) = error else {
            panic!("native I/O cause was lost")
        };
        assert_eq!(native.raw_os_error(), Some(13));
    }

    #[test]
    fn io_conversion_preserves_custom_payload() {
        #[derive(Debug)]
        struct EntropyFailure(u64);
        impl fmt::Display for EntropyFailure {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "entropy source {} interrupted", self.0)
            }
        }
        impl std::error::Error for EntropyFailure {}
        let native = io::Error::new(io::ErrorKind::Interrupted, EntropyFailure(17));
        let error = Error::from(native);
        let Error::Io(native) = error else {
            panic!("native I/O cause was lost")
        };
        assert_eq!(native.kind(), io::ErrorKind::Interrupted);
        assert_eq!(native.to_string(), "entropy source 17 interrupted");
        let payload = native
            .get_ref()
            .unwrap()
            .downcast_ref::<EntropyFailure>()
            .unwrap();
        assert_eq!(payload.0, 17);
    }

    #[test]
    fn errors_are_send_sync_and_static() {
        fn require<T: Send + Sync + 'static>() {}
        require::<Error>();
    }
}
