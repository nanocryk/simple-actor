//! Helper to write actor-based async code.
//!
//! ```rust
//! use futures::FutureExt;
//! use simple_actor::Actor;
//! use std::time::Duration;
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
//!     pub async fn add(&self, x: u32) -> bool {
//!         self.0.queue(move |state| *state += x).await
//!     }
//!
//!     pub async fn add_delayed(&self, x: u32) -> bool {
//!         self.0.queue_blocking(move |state| async move {
//!             tokio::time::sleep(Duration::from_millis(500)).await;
//!             *state += x
//!         }.boxed()).await
//!     }
//!
//!     pub async fn get(&self) -> Option<u32> {
//!         self.0.query(move |state| *state).await
//!     }
//!
//!     pub async fn get_delayed(&self) -> Option<u32> {
//!         self.0.query_blocking(move |state| async move {
//!             tokio::time::sleep(Duration::from_millis(500)).await;
//!             *state
//!         }.boxed()).await
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let adder = Adder::new(5);
//!
//!     assert_eq!(adder.add(2).await, true);
//!     assert_eq!(adder.get().await, Some(7));
//!
//!     assert_eq!(adder.add_delayed(3).await, true);
//!     assert_eq!(adder.get_delayed().await, Some(10));
//!
//!     assert!(adder.0.is_active());
//!     adder.0.shutdown();
//!     assert!(!adder.0.is_active());
//!
//!     assert_eq!(adder.add(2).await, false);
//!     assert_eq!(adder.get().await, None);
//!
//!     assert_eq!(adder.add_delayed(2).await, false);
//!     assert_eq!(adder.get_delayed().await, None);
//! }
//! ```
//!
//! ## Inspiration
//!
//! This crate is inspired by [`ghost_actor`], with a simpler implementation and
//! API.
//!
//! This crate functions returns `None` or `false` if the actor is down, which
//! avoids dealing with error type conversions.
//!
//! This crate also allows to use futures that can hold the state across
//! `.await`.
//!  
//! [`ghost_actor`]: https://github.com/holochain/ghost_actor

use futures::{
    channel::{mpsc, oneshot},
    Future, FutureExt, SinkExt, StreamExt,
};
use std::{hash::Hash, pin::Pin};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a + Send>>;
type Blocking<T> = Box<dyn for<'a> FnOnce(&'a mut T) -> BoxFuture<'a, ()> + Send>;
type NonBlocking<T> = Box<dyn FnOnce(&mut T) + 'static + Send>;

enum StateChange<T> {
    Async(Blocking<T>),
    Sync(NonBlocking<T>),
}

type StateChangeSender<T> = mpsc::Sender<StateChange<T>>;

/// Actor wrapping a state.
///
/// Cloning the actor provides an handle to the same actor.
pub struct Actor<T: 'static + Send>(StateChangeSender<T>);
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
        let (send, recv) = mpsc::channel::<StateChange<T>>(capacity);

        let driver = FutureExt::boxed(async move {
            let mut recv = StreamExt::ready_chunks(recv, 1024);

            while let Some(changes) = recv.next().await {
                for change in changes {
                    match change {
                        StateChange::Async(f) => f(&mut state).await,
                        StateChange::Sync(f) => f(&mut state),
                    }
                }
            }
        });

        (Self(send), driver)
    }

    /// Queue an async function on the state. The future that this function
    /// returns can hold the state across await points, meaning it will prevent
    /// other functions to be processed until the future is complete.
    ///
    /// [`queue_blocking`] resolves once the order is sent to the actor, and
    /// doesn't wait for it to be processed by the actor, but cannot have
    /// an output value.
    ///
    /// To wait for the order to be processed and get an output, use
    /// [`query_blocking`].
    ///
    /// [`queue_blocking`]: Actor::queue_blocking
    /// [`query_blocking`]: Actor::query_blocking
    pub async fn queue_blocking<F>(&self, f: F) -> bool
    where
        F: for<'a> FnOnce(&'a mut T) -> BoxFuture<'a, ()> + Send + 'static,
    {
        let mut send = self.0.clone();

        let f: Blocking<T> = Box::new(move |state: &mut T| {
            async move {
                f(state).await;
            }
            .boxed()
        });

        send.send(StateChange::Async(f)).await.is_ok()
    }

    /// Queue a function on the state. It is more performant to have multiple
    /// [`queue`]/[`query`] in a row, as it can avoid using `.await` on the internal channel
    /// or on a future-based change ([`queue_blocking`]/[`query_blocking`]).
    ///
    /// [`queue`] resolves once the order is sent to the actor, and doesn't wait
    /// for it to be processed by the actor, but cannot have an output value.
    ///
    /// To wait for the order to be processed and get an output, use [`query`].
    ///
    /// [`queue`]: Actor::queue
    /// [`query`]: Actor::query
    /// [`queue_blocking`]: Actor::queue_blocking
    /// [`query_blocking`]: Actor::query_blocking
    pub async fn queue<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut T) + 'static + Send,
    {
        let mut send = self.0.clone();

        send.send(StateChange::Sync(Box::new(f))).await.is_ok()
    }

    /// Queue an async function on the state. The future that this function
    /// returns can hold the state across await points, meaning it will prevent
    /// other functions to be processed until the future is complete.
    ///
    /// [`query_blocking`] resolves once the order as been processed by the actor,
    /// which allows it to return an output.
    ///
    /// If an output is not needed and it is not needed to wait for the order
    /// to be processed, use [`queue_blocking`].
    ///
    /// [`query_blocking`]: Actor::query_blocking
    /// [`queue_blocking`]: Actor::queue_blocking
    pub async fn query_blocking<F, R>(&self, f: F) -> Option<R>
    where
        F: for<'a> FnOnce(&'a mut T) -> BoxFuture<'a, R> + Send + 'static,
        R: 'static + Send,
    {
        let mut send = self.0.clone();
        let (output_send, output_recv) = oneshot::channel();

        let f: Blocking<T> = Box::new(move |state: &mut T| {
            async move {
                let output = f(state).await;
                let _ = output_send.send(output);
            }
            .boxed()
        });

        send.send(StateChange::Async(f)).await.ok()?;

        output_recv.await.ok()
    }

    /// Queue a function on the state. It is more performant to have multiple
    /// [`queue`]/[`query`] in a row, as it can avoid using `.await` on the internal channel
    /// or on a future-based change ([`queue_blocking`]/[`query_blocking`]).
    ///
    /// [`query_blocking`] resolves once the order as been processed by the actor,
    /// which allows it to return an output.
    ///
    /// If an output is not needed and it is not needed to wait for the order
    /// to be processed, use [`queue_blocking`].
    ///
    ///
    /// [`queue`]: Actor::queue
    /// [`query`]: Actor::query
    /// [`queue_blocking`]: Actor::queue_blocking
    /// [`query_blocking`]: Actor::query_blocking
    pub async fn query<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R + 'static + Send,
        R: 'static + Send,
    {
        let mut invoke_tx = self.0.clone();
        let (response_tx, response_rx) = oneshot::channel();

        invoke_tx
            .send(StateChange::Sync(Box::new(move |state| {
                let output = f(state);
                let _ = response_tx.send(output);
            })))
            .await
            .ok()?;

        response_rx.await.ok()
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
