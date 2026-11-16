// SPDX-License-Identifier: GPL-3.0-only
//! The few Win32 calls the window app needs, declared directly (like `stats.rs` does)
//! instead of adding a crate: find our own window, hide it, show it again; make sure
//! only one Smorzo runs; and the optional "Start with Windows" entry.
//!
//! Nothing here touches audio or any system setting. The only thing written outside
//! Smorzo's own window is its own value in the current user's Run key, and only when
//! the user turns "Start with Windows" on.

use std::ffi::c_void;

use smorzo::instance::{self, Claim, Outcome};

type Hwnd = *mut c_void;

#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn GetClassNameW(hwnd: Hwnd, buf: *mut u16, len: i32) -> i32;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn CreateEventW(
        attr: *const c_void,
        manual_reset: i32,
        initial: i32,
        name: *const u16,
    ) -> Handle;
    fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn SetEvent(h: Handle) -> i32;
    fn WaitForMultipleObjects(count: u32, handles: *const Handle, all: i32, ms: u32) -> u32;
    fn CloseHandle(h: Handle) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attr: u32, value: *const c_void, size: u32) -> i32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegSetKeyValueW(
        key: Hkey,
        subkey: *const u16,
        value: *const u16,
        kind: u32,
        data: *const c_void,
        len: u32,
    ) -> i32;
    fn RegDeleteKeyValueW(key: Hkey, subkey: *const u16, value: *const u16) -> i32;
    fn RegGetValueW(
        key: Hkey,
        subkey: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        len: *mut u32,
    ) -> i32;
}

type Handle = *mut c_void;
type Hkey = *mut c_void;

const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;
/// The window class GPUI registers for its windows.
const GPUI_CLASS: &str = "Zed::Window";

/// A window handle stored as a plain number (a raw pointer can't be shared freely in
/// Rust; the number is only ever handed back to Windows).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WindowId(isize);

impl WindowId {
    fn hwnd(self) -> Hwnd {
        self.0 as Hwnd
    }
}

extern "system" fn collect(hwnd: Hwnd, lparam: isize) -> i32 {
    // SAFETY: `lparam` is the `&mut Vec` passed by `gpui_windows` below, alive for the
    // whole (synchronous) EnumWindows call.
    let found = unsafe { &mut *(lparam as *mut Vec<WindowId>) };
    let mut pid = 0u32;
    let mut buf = [0u16; 64];
    // SAFETY: plain queries on a handle Windows just gave us; the buffer length is
    // passed correctly.
    let ours = unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid == GetCurrentProcessId()
    };
    if ours {
        let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == GPUI_CLASS {
            found.push(WindowId(hwnd as isize));
        }
    }
    1 // keep enumerating
}

/// All GPUI windows of this process (Smorzo has exactly one).
pub fn gpui_windows() -> Vec<WindowId> {
    let mut found: Vec<WindowId> = Vec::new();
    // SAFETY: the callback only uses `found` during this call.
    unsafe { EnumWindows(collect, &mut found as *mut Vec<WindowId> as isize) };
    found
}

/// Draw our own window's title bar dark or light, to match the theme. Only this
/// window's frame changes; no Windows setting is touched.
pub fn set_dark_title_bar(w: WindowId, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    let value: i32 = dark.into();
    // SAFETY: our own window; the value outlives the call.
    unsafe {
        DwmSetWindowAttribute(
            w.hwnd(),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const value).cast(),
            size_of::<i32>() as u32,
        )
    };
}

pub fn exists(w: WindowId) -> bool {
    // SAFETY: IsWindow accepts any value and just answers yes/no.
    unsafe { IsWindow(w.hwnd()) != 0 }
}

pub fn hide(w: WindowId) {
    // SAFETY: our own window; SW_HIDE only changes its visibility.
    unsafe { ShowWindow(w.hwnd(), SW_HIDE) };
}

/// Show it again; if it was minimized before being closed, restore it too.
pub fn show(w: WindowId) {
    // SAFETY: our own window.
    unsafe {
        ShowWindow(w.hwnd(), SW_SHOW);
        if IsIconic(w.hwnd()) != 0 {
            ShowWindow(w.hwnd(), SW_RESTORE);
        }
        SetForegroundWindow(w.hwnd());
    }
}

/// A string as Windows wants it: UTF-16 with a terminating zero.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---- One Smorzo at a time ---------------------------------------------------------
//
// Two copies would both write to CABLE Input. The first copy holds the shared lock
// (`smorzo::instance`, also used by `smorzo-cli`) and a "show" event. A second app copy
// signals that event (so the first one's window comes up) and exits. If the lock is
// held but there is no show event, `smorzo-cli` is the one running: say so and exit.

const SHOW_EVENT: &str = r"Local\Smorzo.Show";
/// The show event of a running Sotto (Smorzo's old name, v0.4.0 and older).
const OLD_SHOW_EVENT: &str = r"Local\Sotto.Show";
const EVENT_MODIFY_STATE: u32 = 0x0002;
const WAIT_OBJECT_0: u32 = 0;
const INFINITE: u32 = u32::MAX;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONINFORMATION: u32 = 0x0000_0040;

/// The "show yourself" events of the first copy, as plain numbers (sendable): Smorzo's
/// own, and the old Sotto one (so an old Sotto started later brings this window up).
#[derive(Clone, Copy)]
pub struct ShowSignal([isize; 2], u32);

pub enum Instance {
    /// We are the only Smorzo. Keep the claim alive while running. The signal is `None`
    /// if the show event couldn't be made (a second copy still exits; it just can't
    /// bring this window up).
    First(Option<Claim>, Option<ShowSignal>),
    /// Another Smorzo is running; it has been asked to show its window, or the user
    /// has been told that smorzo-cli is running.
    Second,
}

pub fn claim_instance() -> Instance {
    let claim = match instance::claim() {
        Outcome::Alone(c) => Some(c),
        Outcome::Unknown => None, // can't tell; don't block starting
        Outcome::Taken => {
            let (ev_name, old_name) = (wide(SHOW_EVENT), wide(OLD_SHOW_EVENT));
            // SAFETY: plain Win32 calls with valid zero-terminated strings.
            unsafe {
                let mut ev = OpenEventW(EVENT_MODIFY_STATE, 0, ev_name.as_ptr());
                if ev.is_null() {
                    // An old Sotto is running: bring its window up instead.
                    ev = OpenEventW(EVENT_MODIFY_STATE, 0, old_name.as_ptr());
                }
                if ev.is_null() {
                    let text = wide(
                        "smorzo-cli (or an old sotto-cli) is running and using your microphone \
                         and CABLE Input.\n\nClose it first, then open Smorzo again.",
                    );
                    let title = wide("Smorzo");
                    MessageBoxW(
                        std::ptr::null_mut(),
                        text.as_ptr(),
                        title.as_ptr(),
                        MB_OK | MB_ICONINFORMATION,
                    );
                } else {
                    SetEvent(ev);
                    CloseHandle(ev);
                }
            }
            return Instance::Second;
        }
    };
    let mut events = [0isize; 2];
    let mut n = 0;
    for name in [SHOW_EVENT, OLD_SHOW_EVENT] {
        let name = wide(name);
        // SAFETY: valid zero-terminated name. The event stays open for the life of the
        // process; Windows closes it when we exit. Auto-reset: each SetEvent wakes the
        // waiter once.
        let ev = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if !ev.is_null() {
            events[n] = ev as isize;
            n += 1;
        }
    }
    Instance::First(claim, (n > 0).then_some(ShowSignal(events, n as u32)))
}

impl ShowSignal {
    /// Block until a second copy asks us to show. Run it off the UI thread.
    pub fn wait(self) -> bool {
        let handles = self.0.map(|h| h as Handle);
        // SAFETY: the first `self.1` handles are open events that stay open for the life
        // of the process.
        let r = unsafe { WaitForMultipleObjects(self.1, handles.as_ptr(), 0, INFINITE) };
        // WAIT_OBJECT_0 is 0: results 0..n mean "event i was signaled".
        (WAIT_OBJECT_0..WAIT_OBJECT_0 + self.1).contains(&r)
    }
}

// ---- Start with Windows ----------------------------------------------------------
//
// One value under the current user's Run key (no admin rights, no other user, no
// system setting): Windows starts `"…\smorzo.exe" --hidden` at sign-in. Off by default;
// only the user's switch adds or removes it.

const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Smorzo";
/// The value Smorzo used when it was still called Sotto (v0.4.0 and older).
const OLD_RUN_VALUE: &str = "Sotto";
const REG_SZ: u32 = 1;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;

/// The command Windows should run at sign-in: this exe, starting in the tray.
fn autostart_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" --hidden", exe.display()))
}

/// Smorzo's current "Start with Windows" command, if it has one.
fn autostart_value() -> Option<String> {
    run_value(RUN_VALUE)
}

/// The command stored under `name` in the Run key, if there is one.
fn run_value(name: &str) -> Option<String> {
    let (key, value) = (wide(RUN_KEY), wide(name));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: reads at most `len` bytes into `buf`; names are zero-terminated.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER as Hkey,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
        )
    };
    if r != 0 {
        // Missing, or (very unlikely) longer than 1024 characters: then treat it as
        // present but unreadable, so a refresh rewrites it.
        return (r == 234).then(String::new); // ERROR_MORE_DATA
    }
    let n = (len as usize / 2).saturating_sub(1); // without the terminator
    Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

/// Whether Smorzo is set to start with Windows (any Smorzo entry counts).
pub fn autostart_enabled() -> bool {
    autostart_value().is_some()
}

/// If "Start with Windows" is on but points somewhere else (Smorzo was moved or
/// updated to a new folder), point it at this exe. Only ever rewrites Smorzo's own
/// value, and only when it is already there.
///
/// Smorzo used to be called Sotto: if the old "Sotto" value is there, "Start with
/// Windows" was on, so it is moved to the "Smorzo" value (pointing at this exe) and the
/// old one is removed. Nothing else in the Run key is touched.
pub fn refresh_autostart() {
    // Only the value Sotto itself wrote (`"…\sotto.exe" --hidden`), never someone
    // else's entry that happens to be called "Sotto".
    let ours = |cmd: &str| cmd.to_ascii_lowercase().ends_with("sotto.exe\" --hidden");
    if run_value(OLD_RUN_VALUE).is_some_and(|c| ours(&c)) && set_autostart(true).is_ok() {
        let _ = delete_run_value(OLD_RUN_VALUE);
    }
    if let (Some(now), Some(want)) = (autostart_value(), autostart_command())
        && now != want
    {
        let _ = set_autostart(true);
    }
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let (key, value) = (wide(RUN_KEY), wide(RUN_VALUE));
    let r = if on {
        let cmd = autostart_command().ok_or("couldn't find Smorzo's own path")?;
        let data = wide(&cmd);
        // SAFETY: data is a zero-terminated UTF-16 string and `len` is its size in
        // bytes, including the terminator, as REG_SZ requires.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER as Hkey,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                data.as_ptr() as *const c_void,
                (data.len() * 2) as u32,
            )
        }
    } else {
        return delete_run_value(RUN_VALUE);
    };
    if r == 0 {
        Ok(())
    } else {
        Err(format!("Windows refused the change (error {r})"))
    }
}

/// Remove one of our own values (`Smorzo`, or the old `Sotto`) from the Run key.
fn delete_run_value(name: &str) -> Result<(), String> {
    let (key, value) = (wide(RUN_KEY), wide(name));
    // SAFETY: deletes only the named value; "not found" counts as done.
    let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER as Hkey, key.as_ptr(), value.as_ptr()) };
    match r {
        0 | 2 => Ok(()), // ERROR_FILE_NOT_FOUND
        r => Err(format!("Windows refused the change (error {r})")),
    }
}

// ---- Taskbar theme ---------------------------------------------------------------

const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

/// Whether the taskbar uses the light theme (read-only; dark if unknown).
pub fn taskbar_is_light() -> bool {
    let (key, value) = (wide(PERSONALIZE_KEY), wide("SystemUsesLightTheme"));
    let mut data = 0u32;
    let mut len = 4u32;
    // SAFETY: reads one DWORD into `data`; `len` is its size in bytes.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER as Hkey,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut len,
        )
    };
    r == 0 && data == 1
}
