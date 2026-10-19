//! A lock that outlives a panic elsewhere: what it guards stays consistent
//! (every holder finishes its change or leaves it untouched), so a poisoned
//! lock is taken anyway rather than failing every later caller.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
