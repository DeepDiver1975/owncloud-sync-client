use oc_dbus_service::emblem::status_to_emblem;

#[test]
fn ok_maps_to_default() {
    assert_eq!(status_to_emblem("OK"), "emblem-default");
}

#[test]
fn sync_maps_to_synchronizing() {
    assert_eq!(status_to_emblem("SYNC"), "emblem-synchronizing");
}

#[test]
fn warning_maps_to_important() {
    assert_eq!(status_to_emblem("WARNING"), "emblem-important");
}

#[test]
fn error_maps_to_problem() {
    assert_eq!(status_to_emblem("ERROR"), "emblem-problem");
}

#[test]
fn excluded_maps_to_readonly() {
    assert_eq!(status_to_emblem("EXCLUDED"), "emblem-readonly");
}

#[test]
fn none_maps_to_empty() {
    assert_eq!(status_to_emblem("NONE"), "");
}

#[test]
fn unknown_tag_maps_to_empty() {
    assert_eq!(status_to_emblem("WHATEVER"), "");
}
