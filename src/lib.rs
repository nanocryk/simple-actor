//! Provides an [`Actor`] type that wraps a state and allows mutating it
//! in turns using [`invoke`] and [`invoke_async`].
//!
//! [`invoke`]: Actor::invoke
//! [`invoke_async`]: Actor::invoke_async
//!
//! # Example
//!
//! It is recommended to create a wrapper type around the [`Actor`], and
//! implement async functions that use [`invoke`]/[`invoke_async`] to interact
//! with the inner private state.
//!
//! ```rust
//! use std::time::Duration;
//! use simple_actor::Actor;
//! use futures::FutureExt;
//!
//! #[derive(Clone)]
//! pub struct Adder(Actor<u32>);
//!
//! impl Adder {
//!     pub fn new(initial_value: u32) -> Self {
//!         let (actor, driver) = Actor::new(initial_value);
//!         tokio::spawn(driver);
//!         Self(actor)
//!     }
//!
//!     pub async fn add(&self, x: u32) {
//!         let _ = self.0.invoke(move |state| {
//!             // We can update the state.
//!             *state += x
//!         }).await;
//!     }
//!
//!     pub async fn add_twice_with_delay(&self, x: u32) -> Option<u32> {
//!         self.0
//!             .invoke_async(move |state| {
//!                 async move {
//!                     *state += x;
//!                     // We can .await while holding the state.
//!                     tokio::time::sleep(Duration::from_millis(500)).await;
//! 
//!                     *state += x;
//!                     // We can return a value at the end.
//!                     *state
//!                 }
//!                 .boxed()
//!             })
//!             .await
//!     }
//!
//!     pub async fn result(&self) -> Option<u32> {
//!         self.0.invoke(move |state| *state).await
//!     }
//!
//!     pub fn shutdown(&self) {
//!         self.0.shutdown()
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let adder = Adder::new(5);
//!
//!     adder.add(3).await;
//!     assert_eq!(adder.result().await, Some(8));
//!
//!     adder.add(2).await;
//!     assert_eq!(adder.result().await, Some(10));
//!
//!     assert_eq!(adder.add_twice_with_delay(3).await, Some(16));
//!     assert_eq!(adder.result().await, Some(16));
//!
//!     adder.shutdown();
//!     assert_eq!(adder.result().await, None);
//! }
//! ```
//!
//! ## Inspiration
//!
//! This crate is inspired by [`ghost_actor`], with a simpler implementation and
//! API.
//!
//! This crate [`invoke`] function returns `None` if the actor is down, which
//! avoids dealing with error type conversions.
//!
//! It also allows to hold the state in [`invoke_async`] and thus use
//! async-based state.
//!  
//! [`ghost_actor`]: https://github.com/holochain/ghost_actor

use futures::{
    channel::{mpsc, oneshot},
    Future, FutureExt, SinkExt, StreamExt,
};
use std::{hash::Hash, pin::Pin};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a + Send>>;
type InnerInvoke<T> = Box<dyn for<'a> FnOnce(&'a mut T) -> BoxFuture<'a, ()> + Send>;
type SendInvoke<T> = mpsc::Sender<InnerInvoke<T>>;

/// Actor wrapping a state.
///
/// Cloning the actor provides an handle to the same actor.
pub struct Actor<T: 'static + Send>(SendInvoke<T>);
impl<T: 'static + Send> Actor<T> {
    /// Creates a new `Actor` with default inbound channel capacity (1024).
    ///
    /// Returned future must be spawned in an async executor.
    #[must_use]
    pub fn new(state: T) -> (Self, impl Future<Output = ()>) {
        Self::new_with_capacity(state, 1024)
    }

    /// Creates a new `Actor` with given capacity for its inbound channel.
    ///
    /// Returned future must be spawned in an async executor.
    #[must_use]
    pub fn new_with_capacity(mut state: T, capacity: usize) -> (Self, impl Future<Output = ()>) {
        let (invoke_tx, mut invoke_rx) = mpsc::channel::<InnerInvoke<T>>(capacity);

        let driver = FutureExt::boxed(async move {
            while let Some(invoke) = invoke_rx.next().await {
                invoke(&mut state).await;
            }
        });

        (Self(invoke_tx), driver)
    }

    /// Interacts with the state using a closure returning a future.
    /// This future holds the mutable reference to the state, and prevents the
    /// actor to process further invokes until this future ends.
    ///
    /// The future needs to be boxed using [`futures::FutureExt::boxed`].
    ///
    /// Returns `None` if the actor is no longer running.
    pub fn invoke_async<F, R>(&self, invoke: F) -> impl Future<Output = Option<R>>
    where
        F: for<'a> FnOnce(&'a mut T) -> BoxFuture<'a, R> + Send + 'static,
        R: 'static + Send,
    {
        let mut invoke_tx = self.0.clone();

        async move {
            let (response_tx, response_rx) = oneshot::channel();

            let real_invoke: InnerInvoke<T> = Box::new(move |state: &mut T| {
                async move {
                    let res = invoke(state).await;
                    let _ = response_tx.send(res);
                }
                .boxed()
            });

            invoke_tx.send(real_invoke).await.ok()?;

            response_rx.await.ok()
        }
    }

    /// Interact with the state using a closure.
    ///
    /// Returns `None` if the actor is no longer running.
    pub async fn invoke<F, R>(&self, invoke: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R + 'static + Send,
        R: 'static + Send,
    {
        self.invoke_async(|state| async move { invoke(state) }.boxed())
            .await
    }

    /// Tells if the actor still accepts new invokes.
    pub fn is_active(&self) -> bool {
        !self.0.is_closed()
    }

    /// Stop the actor, which will process every already queued invokes
    /// before really stopping.
    pub fn shutdown(&self) {
        self.0.clone().close_channel()
    }
}

impl<T: 'static + Send> Clone for Actor<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: 'static + Send> PartialEq for Actor<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_receiver(&other.0)
    }
}

impl<T: 'static + Send> Eq for Actor<T> {}

impl<T: 'static + Send> Hash for Actor<T> {
    fn hash<Hasher: std::hash::Hasher>(&self, hasher: &mut Hasher) {
        self.0.hash_receiver(hasher);
    }
}
