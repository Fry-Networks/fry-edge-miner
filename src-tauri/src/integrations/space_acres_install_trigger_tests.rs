use super::*;
use crate::elevation_gate::ElevationTrigger;

#[test]
fn a_user_gesture_maps_to_user_click() {
    assert_eq!(install_trigger_for(true), ElevationTrigger::UserClick);
}

#[test]
fn no_user_gesture_maps_to_automatic() {
    assert_eq!(install_trigger_for(false), ElevationTrigger::Automatic);
}

#[test]
fn the_precheck_applies_only_to_automatic() {
    assert!(precheck_applies(ElevationTrigger::Automatic));
    assert!(!precheck_applies(ElevationTrigger::UserClick));
}
