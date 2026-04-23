// TODO: full implementation in Task 9
#[derive(Debug)]
pub struct TrayHandle;

impl Clone for TrayHandle {
    fn clone(&self) -> Self {
        panic!("TrayHandle must not be cloned")
    }
}
