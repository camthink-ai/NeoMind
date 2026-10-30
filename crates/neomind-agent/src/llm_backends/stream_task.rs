//! Cancel the upstream HTTP request when its stream consumer goes away.

pub(crate) fn spawn_stream<T: Send + 'static>(
    sender: tokio::sync::mpsc::Sender<T>,
    request: impl std::future::Future<Output = ()> + Send + 'static,
) {
    tokio::spawn(async move {
        tokio::select! {
            () = sender.closed() => {},
            () = request => {},
        }
    });
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn dropping_a_silent_stream_cancels_the_request() {
        struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                // Drop notification only; the request never produces a token.
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(1);
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let marker = Dropped(Some(dropped_tx));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        super::spawn_stream(tx, async move {
            let _marker = marker;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.unwrap();
        drop(rx);
        tokio::time::timeout(std::time::Duration::from_secs(1), dropped_rx)
            .await
            .expect("upstream request must be dropped promptly")
            .expect("drop notification");
    }
}
