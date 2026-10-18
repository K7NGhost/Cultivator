use std::{
    ops::{Deref, DerefMut},
    process::Child,
};

/// Always reap owned subprocesses, including error and panic paths.
pub(crate) struct ManagedChild(pub Child);
impl Deref for ManagedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
