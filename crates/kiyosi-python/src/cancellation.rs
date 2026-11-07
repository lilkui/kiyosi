use std::{sync::mpsc, thread, time::Duration};

use pyo3::prelude::*;

pub(crate) fn run<T: Send>(
    py: Python<'_>,
    operation: impl FnOnce() -> kiyosi::Result<T> + Send,
) -> PyResult<kiyosi::Result<T>> {
    py.check_signals()?;
    let token = kiyosi::CancellationToken::default();
    thread::scope(|scope| {
        let (sender, mut receiver) = mpsc::sync_channel(1);
        let worker_token = token.clone();
        let worker = thread::Builder::new()
            .name("kiyosi-request".into())
            .spawn_scoped(scope, move || {
                let result = worker_token.run(operation);
                let _ = sender.send(());
                result
            })?;
        let mut interrupted = None;
        loop {
            let finished = py.detach({
                let receiver = &mut receiver;
                move || receiver.recv_timeout(Duration::from_millis(10))
            });
            match finished {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(error) = py.check_signals() {
                        token.cancel();
                        interrupted = Some(error);
                        break;
                    }
                }
            }
        }
        let result = py
            .detach(|| worker.join())
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        if let Some(error) = interrupted {
            return Err(error);
        }
        py.check_signals()?;
        Ok(result)
    })
}
