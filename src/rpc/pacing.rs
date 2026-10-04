//! Avoid registering a coarse timer for packets that are already due.

pub(crate) async fn wait_until(at: std::time::Instant) {
    let deadline = tokio::time::Instant::from_std(at);
    // Tokio rounds timer deadlines to milliseconds. Even a just-expired
    // deadline can otherwise wait for the next tick, once per UDP packet.
    if deadline > tokio::time::Instant::now() {
        tokio::time::sleep_until(deadline).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{task::Poll, time::Duration};
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn due_packets_never_yield_and_future_packets_wait() {
        let now = Instant::now();
        assert_eq!(
            futures::poll!(std::pin::pin!(wait_until(now.into_std()))),
            Poll::Ready(())
        );
        assert_eq!(
            futures::poll!(std::pin::pin!(wait_until(
                (now - Duration::from_micros(1)).into_std()
            ))),
            Poll::Ready(())
        );
        let future = wait_until((now + Duration::from_millis(10)).into_std());
        tokio::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        tokio::time::advance(Duration::from_millis(9)).await;
        assert!(futures::poll!(&mut future).is_pending());
        tokio::time::advance(Duration::from_millis(2)).await;
        assert_eq!(futures::poll!(&mut future), Poll::Ready(()));
    }
}
