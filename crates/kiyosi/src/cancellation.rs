use std::{
    cell::RefCell,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::Result;

thread_local! {
    static TOKENS: RefCell<Vec<CancellationToken>> = const { RefCell::new(Vec::new()) };
}

/// Requests cooperative cancellation of a numerical calculation.
///
/// Clones share a one-way cancellation flag and are `Send + Sync`. Run a request
/// with [`Self::run`] and cancel a clone from another thread. Built-in numerical
/// engines check between work units; allocations and custom engines may take longer.
/// Nested requests observe every enclosing token. Independent requests keep their
/// own tokens, including when they share Monte Carlo workers.
///
/// # Examples
///
/// ```rust
/// use kiyosi::{CancellationToken, Error};
/// let token = CancellationToken::default();
/// token.clone().cancel();
/// assert!(matches!(token.run(|| Ok(42)),
///     Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Cancels this token and every clone; see the [`CancellationToken`] example.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Checks this token, including from a custom engine; see [`Self::run`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Io`] with [`io::ErrorKind::Interrupted`] after cancellation.
    pub fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "calculation cancelled").into());
        }
        Ok(())
    }

    /// Runs a synchronous request under this token; see the [`CancellationToken`] example.
    ///
    /// Restores the enclosing scope on return or panic. The closure runs on the
    /// calling thread; built-in parallel Monte Carlo work inherits its tokens.
    ///
    /// # Errors
    ///
    /// Returns the closure's errors or [`crate::Error::Io`] with [`io::ErrorKind::Interrupted`]
    /// when an enclosing token is cancelled. A cancelled request returns no partial result.
    pub fn run<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut tokens = current();
        tokens.push(self.clone());
        with_tokens(tokens, operation)
    }
}

pub(crate) fn current() -> Vec<CancellationToken> {
    TOKENS.with(|tokens| tokens.borrow().clone())
}

pub(crate) fn check() -> Result<()> {
    TOKENS.with(|tokens| {
        tokens
            .borrow()
            .iter()
            .try_for_each(CancellationToken::check)
    })
}

pub(crate) fn with_tokens<T>(
    tokens: Vec<CancellationToken>,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    struct Restore(Vec<CancellationToken>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TOKENS.with(|tokens| {
                tokens.replace(std::mem::take(&mut self.0));
            });
        }
    }
    let _restore = Restore(TOKENS.with(|current| current.replace(tokens)));
    check()?;
    let result = operation()?;
    check()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cancelled_requests_skip_work_and_leave_other_requests_usable() -> Result<()> {
        let token = CancellationToken::default();
        token.clone().cancel();
        let calls = AtomicUsize::new(0);
        let result = token.run(|| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(42)
        });
        assert!(
            matches!(result, Err(crate::Error::Io(error)) if error.kind() == io::ErrorKind::Interrupted)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(CancellationToken::default().run(|| Ok(42))?, 42);
        check()?;
        Ok(())
    }

    #[test]
    fn nested_scopes_observe_parent_cancellation_and_restore_after_panics() -> Result<()> {
        let outer = CancellationToken::default();
        let inner = CancellationToken::default();
        let result = outer.run(|| {
            inner.run(|| {
                outer.cancel();
                check()
            })
        });
        assert!(
            matches!(result, Err(crate::Error::Io(error)) if error.kind() == io::ErrorKind::Interrupted)
        );
        assert!(
            std::panic::catch_unwind(|| inner.run::<()>(|| {
                inner.cancel();
                panic!("custom engine panic")
            }))
            .is_err()
        );
        check()?;
        assert_eq!(CancellationToken::default().run(|| Ok(7))?, 7);
        Ok(())
    }
}
