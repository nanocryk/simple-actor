# simple-actor

Provides an `Actor` type that wraps a state and allows mutating it
in turns using `invoke`.

## Example

It is recommended to create a wrapper type around the `Actor`,
and implement async functions that use `invoke` to interact with
the inner private state.

```rust
use simple_actor::Actor;

#[derive(Clone)]
pub struct Adder(Actor<u32>);

impl Adder {
    pub fn new(initial_value: u32) -> Self {
        let (actor, driver) = Actor::new(initial_value);
        tokio::spawn(driver);
        Self(actor)
    }

    pub async fn add(&self, x: u32) {
        let _ = self.0.invoke(move |state| *state += x).await;
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

    adder.shutdown();
    assert_eq!(adder.result().await, None);
}
```

## Inspiration

This crate is inspired by [`ghost_actor`], with a simpler implementation and
API. This crate `invoke` function returns `None` if the actor is down, which
avoids dealing with error type conversions.
 
[`ghost_actor`]: https://github.com/holochain/ghost_actor