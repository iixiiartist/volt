use volt::channels::webhook::WebhookChannel;

#[test]
fn test_webhook_channel_new() {
    let channel = WebhookChannel::new(8080, "test-secret".into());
    assert_eq!(channel.port, 8080);
}

#[test]
fn test_webhook_channel_clone() {
    let channel = WebhookChannel::new(8080, "test-secret".into());
    let cloned = channel.clone();
    assert_eq!(cloned.port, 8080);
    assert_eq!(cloned.secret, "test-secret");
}

#[test]
fn test_webhook_channel_trait_exists() {
    // Compile-time check that WebhookChannel implements Clone
    fn assert_clone<T: Clone>(_: &T) {}
    let channel = WebhookChannel::new(8080, "test".into());
    assert_clone(&channel);
}
