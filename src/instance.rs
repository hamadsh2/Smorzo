// SPDX-License-Identifier: GPL-3.0-only
//! "Is another Smorzo already running?" — shared by the window app and `smorzo-cli`.
//!
//! Two copies would both write to CABLE Input, so whichever starts first holds a named
//! Windows mutex for as long as it runs, and the other one refuses to start its audio.
//! "Local\" names are per Windows session (per signed-in user). Windows releases the
//! mutex by itself when the process ends, even after a crash.
//!
//! Python analogy: a lock file that the OS deletes for you when the program exits.

use std::ffi::c_void;

type Handle = *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(attr: *const c_void, initial_owner: i32, name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn CloseHandle(h: Handle) -> i32;
}

const MUTEX: &str = r"Local\Smorzo.Instance";
/// The lock Smorzo used when it was still called Sotto (v0.4.0 and older). A running
/// Sotto counts as "another copy" too: both would write to CABLE Input.
const OLD_MUTEX: &str = r"Local\Sotto.Instance";
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_ACCESS_DENIED: u32 = 5;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What creating one named lock told us.
enum Lock {
    /// We hold it now.
    Ours(Handle),
    /// Someone else holds it (or Windows won't let us open it: then it isn't ours).
    Theirs,
    /// Windows couldn't make it for another reason.
    Failed,
}

fn lock(name: &str) -> Lock {
    let name = wide(name);
    // SAFETY: plain Win32 calls with a valid zero-terminated name.
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        // Read the error at once, before any other call can change it.
        let err = GetLastError();
        if h.is_null() {
            return if err == ERROR_ACCESS_DENIED {
                Lock::Theirs
            } else {
                Lock::Failed
            };
        }
        if err == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return Lock::Theirs;
        }
        Lock::Ours(h)
    }
}

/// Proof that this process is the only Smorzo. Keep it alive for the whole run; dropping
/// it lets another copy start. Holds both the new lock and the old Sotto one, so an
/// old Sotto started later sees "already running" too.
pub struct Claim(Handle, Option<Handle>);

// SAFETY: the handle is only closed in `drop`; Windows handles may be used from any
// thread.
unsafe impl Send for Claim {}

impl Drop for Claim {
    fn drop(&mut self) {
        // SAFETY: our own handles, each closed exactly once.
        unsafe {
            CloseHandle(self.0);
            if let Some(old) = self.1 {
                CloseHandle(old);
            }
        }
    }
}

pub enum Outcome {
    /// We are the only one.
    Alone(Claim),
    /// Another Smorzo (the app or `smorzo-cli`), or an old Sotto, is running.
    Taken,
    /// Windows couldn't tell us (very unlikely); don't block starting.
    Unknown,
}

pub fn claim() -> Outcome {
    claim_names(MUTEX, OLD_MUTEX)
}

fn claim_names(new_name: &str, old_name: &str) -> Outcome {
    let new = match lock(new_name) {
        Lock::Ours(h) => h,
        Lock::Theirs => return Outcome::Taken,
        Lock::Failed => return Outcome::Unknown,
    };
    match lock(old_name) {
        Lock::Ours(old) => Outcome::Alone(Claim(new, Some(old))),
        Lock::Theirs => {
            // SAFETY: our own handle from `lock`, closed once.
            unsafe { CloseHandle(new) };
            Outcome::Taken
        }
        // Can't make the old lock: still the only Smorzo, so carry on with the new one.
        Lock::Failed => Outcome::Alone(Claim(new, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test uses its own lock names, so tests and a running Smorzo can't collide.
    fn names(test: &str) -> (String, String) {
        let id = std::process::id();
        (
            format!(r"Local\SmorzoTest.{test}.{id}.New"),
            format!(r"Local\SmorzoTest.{test}.{id}.Old"),
        )
    }

    #[test]
    fn a_running_old_sotto_counts_as_taken() {
        let (new, old) = names("old");
        // Pretend to be an old Sotto: hold only the old lock.
        let Lock::Ours(h) = lock(&old) else {
            panic!("a fresh test lock must be ours");
        };
        assert!(matches!(claim_names(&new, &old), Outcome::Taken));
        // SAFETY: our own handle.
        unsafe { CloseHandle(h) };
        assert!(matches!(claim_names(&new, &old), Outcome::Alone(_)));
    }

    #[test]
    fn only_one_claim_at_a_time() {
        let (new, old) = names("one");
        let Outcome::Alone(first) = claim_names(&new, &old) else {
            panic!("a fresh test lock must be free");
        };
        assert!(matches!(claim_names(&new, &old), Outcome::Taken));
        // An old Sotto started now would see its own lock taken too.
        assert!(matches!(lock(&old), Lock::Theirs));
        drop(first);
        assert!(matches!(claim_names(&new, &old), Outcome::Alone(_)));
    }
}
