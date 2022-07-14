# simple-actor

[![License](https://img.shields.io/crates/l/simple-actor)](./LICENSE)
[![Version](https://img.shields.io/crates/v/simple-actor)](https://crates.io/crates/simple-actor)
[![Documentation](https://img.shields.io/docsrs/simple-actor)](https://docs.rs/simple-actor/)

Provides an `Actor` type that wraps a state and allows mutating it in turns
using `invoke` and `invoke_async`.

# Example

It is recommended to create a wrapper type around the `Actor`, and implement
async functions that use `invoke`/`invoke_async` to interact with the inner
private state.

```rust
use std::time::Duration;
use simple_actor::Actor;
use futures::FutureExt;

#[derive(Clone)]
pub struct Adder(Actor<u32>);

impl Adder {
    pub fn new(initial_value: u32) -> Self {
        let (actor, driver) = Actor::new(initial_value);
        tokio::spawn(driver);
        Self(actor)
    }

    pub async fn add(&self, x: u32) {
        let _ = self.0.invoke(move |state| {
            // We can update the state.
            *state += x
        }).await;
    }

    pub async fn add_twice_with_delay(&self, x: u32) -> Option<u32> {
        self.0
            .invoke_async(move |state| {
                async move {
                    *state += x;
                    // We can .await while holding the state.
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    *state += x;
                    // We can return a value at the end.
                    *state
                }
                .boxed()
            })
            .await
    }

    pub async fn result(&self) -> Option<u32> {
        self.0.invoke(move |state| *state).await
    }

    pub fn shutdown(&self) {
        self.0.shutdown()
    }
}

#[tokio::main]
async fn main() {
    let adder = Adder::new(5);

    adder.add(3).await;
    assert_eq!(adder.result().await, Some(8));

    adder.add(2).await;
    assert_eq!(adder.result().await, Some(10));

    assert_eq!(adder.add_twice_with_delay(3).await, Some(16));
    assert_eq!(adder.result().await, Some(16));

    adder.shutdown();
    assert_eq!(adder.result().await, None);
}
```

## Inspiration

This crate is inspired by [`ghost_actor`], with a simpler implementation and
API.

This crate [`invoke`] function returns `None` if the actor is down, which avoids
dealing with error type conversions.

It also allows to hold the state in [`invoke_async`] and thus use async-based
state.

[`ghost_actor`]: https://github.com/holochain/ghost_actor
