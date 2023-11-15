use std::sync::{RwLock, RwLockWriteGuard, RwLockReadGuard};

pub trait HandyRwLock<T> {
    fn wl(&self) -> RwLockWriteGuard<'_, T>;
    fn rl(&self) -> RwLockReadGuard<'_, T>;
}

impl<T> HandyRwLock<T> for RwLock<T> {
    fn wl(&self) -> RwLockWriteGuard<'_, T> {  //9 NodeIndex(16), Local: _0
        self.write().unwrap()
    }

    fn rl(&self) -> RwLockReadGuard<'_, T> { //13 NodeIndex(18), Local: _0
        self.read().unwrap()
    }
}
