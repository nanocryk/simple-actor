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

    pub async fn add(&self, x: u32) {
        let _ = self.0.invoke(move |state| *state += x).await;
    }

    pub async fn add_twice_after_delay(&self, x: u32) {
        let actor = self.clone();
        let _ = self
            .0
            .invoke_future(move |state| {
                *state += x;

                async move {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    actor.add(x).await
                }
            })
            .await;
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

    adder.add_twice_after_delay(3).await;
    assert_eq!(adder.result().await, Some(16));

    adder.shutdown();
    assert_eq!(adder.result().await, None);
}
