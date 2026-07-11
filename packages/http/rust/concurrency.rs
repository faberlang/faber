//! Async handler workers with explicit, race-safe application state.
//!
//! TARGET: G9 API4 framework concurrency boundary.
//! WHY: handlers share only `ApplicationState`; response tickets own correlated
//! completion and cancellation, so late work cannot publish a success.

#![allow(dead_code)] // API4 surface is shipped from the package before all product callers exist.

use faber::Valor;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::oneshot;

/// Explicit shared state cloned into each handler worker.
#[derive(Clone, Default)]
pub struct ApplicationState {
    values: Arc<Mutex<BTreeMap<String, Valor>>>,
}

impl ApplicationState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Result<Option<Valor>, StateError> {
        self.values
            .lock()
            .map(|values| values.get(key).cloned())
            .map_err(|_| StateError::Poisoned)
    }

    pub fn set(&self, key: impl Into<String>, value: Valor) -> Result<(), StateError> {
        self.values
            .lock()
            .map(|mut values| {
                values.insert(key.into(), value);
            })
            .map_err(|_| StateError::Poisoned)
    }

    /// Increment a numeric value in one critical section and return the new value.
    pub fn increment(&self, key: impl Into<String>) -> Result<i64, StateError> {
        let key = key.into();
        let mut values = self.values.lock().map_err(|_| StateError::Poisoned)?;
        let current = match values.get(&key) {
            Some(Valor::Numerus(value)) => *value,
            Some(_) => return Err(StateError::NotNumeric(key)),
            None => 0,
        };
        let next = current.checked_add(1).ok_or(StateError::Overflow)?;
        values.insert(key, Valor::Numerus(next));
        Ok(next)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateError {
    Poisoned,
    NotNumeric(String),
    Overflow,
}

/// Multi-thread worker runtime owned by one HTTP application.
pub struct HandlerWorkers {
    runtime: Runtime,
    state: ApplicationState,
}

impl HandlerWorkers {
    pub fn new(worker_threads: usize, state: ApplicationState) -> Result<Self, std::io::Error> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(worker_threads.max(1))
            .enable_time()
            .build()?;
        Ok(Self { runtime, state })
    }

    /// Spawn one async handler with a cloned state handle and correlated response ticket.
    pub fn spawn<F, Fut>(&self, request_id: impl Into<String>, handler: F) -> ResponseTicket
    where
        F: FnOnce(ApplicationState) -> Fut + Send + 'static,
        Fut: Future<Output = Valor> + Send + 'static,
    {
        let request_id = request_id.into();
        let state = self.state.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = oneshot::channel();

        self.runtime.spawn(async move {
            let response = handler(state).await;
            if !worker_cancelled.load(Ordering::Acquire) {
                let _ = sender.send(response);
            }
        });

        ResponseTicket {
            request_id,
            cancelled,
            receiver: Some(receiver),
        }
    }
}

/// One request's completion channel. Dropping or cancelling it suppresses late success.
pub struct ResponseTicket {
    request_id: String,
    cancelled: Arc<AtomicBool>,
    receiver: Option<oneshot::Receiver<Valor>>,
}

impl ResponseTicket {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub async fn complete(mut self) -> ResponseCompletion {
        if self.cancelled.load(Ordering::Acquire) {
            return ResponseCompletion::Cancelled;
        }
        let Some(receiver) = self.receiver.take() else {
            return ResponseCompletion::Cancelled;
        };
        match receiver.await {
            Ok(value) if !self.cancelled.load(Ordering::Acquire) => {
                ResponseCompletion::Completed(value)
            }
            Ok(_) | Err(_) => ResponseCompletion::Cancelled,
        }
    }
}

impl Drop for ResponseTicket {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResponseCompletion {
    Completed(Valor),
    Cancelled,
}

#[cfg(test)]
#[path = "concurrency_test.rs"]
mod concurrency_test;
