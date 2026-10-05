// SPDX-License-Identifier: GPL-2.0-or-later
// Copyright © 2026 Jim Collier [ID: 2უNაɘ«҂թȹɤξπ๙¿ձϖ]

//! The one way a std lock is taken here.
//!
//! A lock is poisoned only by a panic while it is held, and the release
//! profile builds with `panic = "abort"`, so a shipped build never sees one.
//! Tests and debug builds unwind, and there a poisoned lock is taken as it
//! stands: every value behind one is set or taken whole, so a panicking
//! holder cannot leave it half written, and one failed test should not fail
//! every test after it. The engine's term lock is a `FairMutex`, which has no
//! poison, and is not taken through here (G2).

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn read<T: ?Sized>(rw: &RwLock<T>) -> RwLockReadGuard<'_, T> {
	rw.read().unwrap_or_else(PoisonError::into_inner)
}

pub fn write<T: ?Sized>(rw: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
	rw.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
	use super::*;

	// A holder that panicked leaves its last whole write behind, and the next
	// taker gets it rather than a second panic.
	// Test ID: Erm4AZ1
	#[test]
	fn a_poisoned_lock_is_taken_as_it_stands() {
		let mutex = Mutex::new(1);
		let rw = RwLock::new(1);
		let _ = crate::testdir::catch_expected_panic(|| {
			*lock(&mutex) = 2;
			*write(&rw) = 2;
			let _held = (lock(&mutex), write(&rw));
			panic!("poison both");
		});
		assert!(mutex.is_poisoned() && rw.is_poisoned());
		assert_eq!(*lock(&mutex), 2);
		assert_eq!(*read(&rw), 2);
		*write(&rw) = 3;
		assert_eq!(*read(&rw), 3);
	}
}
