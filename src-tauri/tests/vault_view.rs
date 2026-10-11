use shixu_desktop::vault_view::{NavigationState, content_bounds};
#[test]
fn latest_navigation_and_matching_redaction_ack_control_visibility() {
    let mut state = NavigationState::default();
    assert!(state.request(true, 1));
    assert!(state.request(false, 2));
    assert!(!state.request(true, 1));
    assert!(!state.acknowledge(1));
    assert!(!state.acknowledge(2));
    assert!(state.request(true, 3));
    assert!(!state.acknowledge(2));
    assert!(state.acknowledge(3));
}
#[test]
fn fixed_navigation_edge_survives_resize_and_narrow_windows() {
    assert_eq!(content_bounds(1487.0, 1058.0), (214.0, 0.0, 1273.0, 1058.0));
    assert_eq!(content_bounds(760.0, 600.0), (214.0, 0.0, 546.0, 600.0));
    assert_eq!(content_bounds(0.0, 0.0), (214.0, 0.0, 1.0, 1.0));
}
