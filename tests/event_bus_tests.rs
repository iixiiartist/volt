use volt::events::{Event, EventBus};

#[test]
fn test_event_bus_publish_and_receive() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    
    let event = Event::JobCompleted {
        job_id: uuid::Uuid::new_v4(),
        output: "test".into(),
    };
    bus.publish(event.clone());
    
    let received = rx.try_recv().expect("should receive event");
    match (received, event) {
        (Event::JobCompleted { output: out1, .. }, Event::JobCompleted { output: out2, .. }) => {
            assert_eq!(out1, out2);
        }
        _ => panic!("event types mismatch"),
    }
}

#[test]
fn test_event_bus_multiple_subscribers() {
    let bus = EventBus::new();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    
    let event = Event::MemoryWrite { path: "test.md".into() };
    bus.publish(event);
    
    assert!(rx1.try_recv().is_ok());
    assert!(rx2.try_recv().is_ok());
}
