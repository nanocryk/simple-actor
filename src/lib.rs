//! Provides an [`Actor`] type that wraps a state and allows mutating it
//! in turns using [`invoke`].
//!
//! [`invoke`]: Actor::invoke
//!
//! # Example
//!
//! It is recommended to create a wrapper type around the [`Actor`],
//! and implement async functions that use [`invoke`] to interact with
//! the inner private state.
//!
//! ```
//! use simple_actor::Actor;
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
//!         let _ = self.0.invoke(move |state| *state += x).await;
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
//!     adder.shutdown();
//!     assert_eq!(adder.result().await, None);
//! }
//! ```

use futures::{
    channel::{mpsc, oneshot},
    Future, FutureExt, SinkExt, StreamExt,
};

type InnerInvoke<T> = Box<dyn FnOnce(&mut T) + 'static + Send>;
type SendInvoke<T> = mpsc::Sender<InnerInvoke<T>>;

/// Actor wrapping a state.
#[derive(Clone)]
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
        let (invoke_tx, invoke_rx) = mpsc::channel::<InnerInvoke<T>>(capacity);

        let driver = FutureExt::boxed(async move {
            let mut invoke_rx = StreamExt::ready_chunks(invoke_rx, 1024);

            while let Some(invokes) = invoke_rx.next().await {
                for invoke in invokes {
                    invoke(&mut state);
                }
            }
        });

        (Self(invoke_tx), driver)
    }

    /// Interact with the state.
    /// Returns `None` if the actor is no longer running.
    pub fn invoke<F, R>(&self, invoke: F) -> impl Future<Output = Option<R>>
    where
        F: FnOnce(&mut T) -> R + 'static + Send,
        R: 'static + Send,
    {
        let mut invoke_tx = self.0.clone();

        async move {
            let (response_tx, response_rx) = oneshot::channel();

            let real_invoke: InnerInvoke<T> = Box::new(move |state: &mut T| {
                let res = invoke(state);
                let _ = response_tx.send(res);
            });

            invoke_tx.send(real_invoke).await.ok()?;

            response_rx.await.ok()
        }
    }

    /// Interact with the state then awaits a Future.
    /// This Future cannot hold the reference to the state.
    pub async fn invoke_future<Fun, Fut, R>(&self, invoke: Fun) -> Option<R>
    where
        Fun: FnOnce(&mut T) -> Fut + 'static + Send,
        Fut: Future<Output = R> + 'static + Send,
        R: 'static + Send,
    {
        Some(self.invoke(invoke).await?.await)
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
