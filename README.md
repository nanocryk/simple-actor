# simple-actor

[![License](https://img.shields.io/crates/l/simple-actor)](./LICENSE)
[![Version](https://img.shields.io/crates/v/simple-actor)](https://crates.io/crates/simple-actor)
[![Documentation](https://img.shields.io/docsrs/simple-actor)](https://docs.rs/simple-actor/)

Helper to write actor-based async code.

```rust
use futures::FutureExt;
use simple_actor::Actor;
use std::time::Duration;

#[derive(Clone)]
pub struct Adder(Actor<u32>);

impl Adder {
    pub fn new(initial_value: u32) -> Self {
        let (actor, driver) = Actor::new(initial_value);
        tokio::spawn(driver);
        Self(actor)
    }

    pub async fn add(&self, x: u32) -> Option<()> {
        self.0.queue(move |state| *state += x).await
    }

    pub async fn add_delayed(&self, x: u32) -> Option<()> {
        self.0.queue_blocking(move |state| async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            *state += x
        }.boxed()).await
    }

    pub async fn get(&self) -> Option<u32> {
        self.0.query(move |state| *state).await
    }

    pub async fn get_delayed(&self) -> Option<u32> {
        self.0.query_blocking(move |state| async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            *state
        }.boxed()).await
    }
}

#[tokio::main]
async fn main() {
    let adder = Adder::new(5);

    assert_eq!(adder.add(2).await, Some(()));
    assert_eq!(adder.get().await, Some(7));

    assert_eq!(adder.add_delayed(3).await, Some(()));
    assert_eq!(adder.get_delayed().await, Some(10));

    assert!(adder.0.is_active());
    adder.0.shutdown();
    assert!(!adder.0.is_active());

    assert_eq!(adder.add(2).await, None);
    assert_eq!(adder.get().await, None);

    assert_eq!(adder.add_delayed(2).await, None);
    assert_eq!(adder.get_delayed().await, None);
}
```

## Inspiration

This crate is inspired by [`ghost_actor`], with a simpler implementation and
API.

This crate functions returns `None` or `false` if the actor is down, which
avoids dealing with error type conversions.

This crate also allows to use futures that can hold the state across
`.await`.

[`ghost_actor`]: https://github.com/holochain/ghost_actor
