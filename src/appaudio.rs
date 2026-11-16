// SPDX-License-Identifier: GPL-3.0-only
//! "App through mic": mix one app's sound (Spotify, a browser, a soundboard…) into
//! what Smorzo sends to CABLE Input, so friends hear it along with your voice.
//!
//! Windows 10 2004+ can capture the audio of **one process** (and its children) with a
//! "process loopback": `ActivateAudioInterfaceAsync` on the virtual device
//! `VAD\Process_Loopback`. It is **read-only**: it opens no stream on any audio device,
//! plays nothing anywhere and changes no setting. The app keeps playing to your
//! headphones as usual (that's how you hear it); Smorzo only gets a copy.
//!
//! ```text
//!  app (e.g. Spotify) ─► process loopback ─(smorzo-app thread)─► mono ─► ring ─┐
//!                                                                             ▼
//!  voice: … → gate → auto gain ──────────────────────────► + app × volume → limiter
//! ```
//!
//! What the copy contains follows the app's own volume and its slider in the Windows
//! Volume Mixer (measured: +20 dB from Spotify's slider, −6 dB from the mixer), so the
//! mix has its own volume in Smorzo.
//!
//! Python analogy: `AppCapture` is a `threading.Thread` filling a `queue.Queue`;
//! `AppMix` is the consumer that takes 10 ms at a time inside the audio worker.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    IAgileObject, IAgileObject_Impl,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentThread, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, SetEvent,
    SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL, WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, PWSTR, Ref, implement};

use crate::denoise::FRAME;
use crate::engine::SAMPLE_RATE;

/// Never mixed in, by exe name (lower case): Smorzo itself (it would feed back into its
/// own output) and voice chat (friends would hear themselves).
const SMORZO: [&str; 4] = ["smorzo.exe", "smorzo-cli.exe", "sotto.exe", "sotto-cli.exe"];
const VOICE_CHAT: [&str; 6] = [
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "ms-teams.exe",
    "teams.exe",
    "zoom.exe",
];
/// System processes that start other apps. The capture includes the picked app's child
/// processes, so these would pull in whatever they started (possibly Smorzo or Discord).
const LAUNCHERS: [&str; 9] = [
    "explorer.exe",
    "svchost.exe",
    "audiodg.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
];

/// Ring capacity: 200 ms (a hard ceiling; the oldest is dropped beyond it).
const RING: usize = SAMPLE_RATE as usize / 5;
/// Frames kept queued when (re)starting, so thread jitter doesn't starve the mix.
const START_FRAMES: usize = 2;
/// More than this queued (the app's clock runs a little fast vs the mic) → drop the
/// oldest down to `START_FRAMES`, so the app never lags far behind your voice.
const MAX_FRAMES: usize = 8;

const NO_SMORZO: &str = "Smorzo can't mix its own sound back in";
const NO_CHAT: &str =
    "voice chat (Discord, Teams, Zoom) can't go through your mic: the others would hear themselves";

/// Why this exe can't be picked, if it can't.
pub fn blocked_reason(exe: &str) -> Option<&'static str> {
    let e = exe.to_ascii_lowercase();
    if SMORZO.contains(&e.as_str()) {
        Some(NO_SMORZO)
    } else if VOICE_CHAT.contains(&e.as_str()) {
        Some(NO_CHAT)
    } else if LAUNCHERS.contains(&e.as_str()) {
        Some("that's a part of Windows that starts other apps, so it can't be picked")
    } else {
        None
    }
}

/// Running processes: (pid, parent pid, lower-case exe name). Read-only snapshot.
fn process_table() -> Result<Vec<(u32, u32, String)>, String> {
    let mut out = Vec::new();
    // SAFETY: a ToolHelp snapshot handle owned by `Handle`; entries are plain data.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| format!("couldn't check the app's processes: {e}"))?;
        let snap = Handle(snap);
        let mut e = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snap.0, &mut e).is_ok();
        while ok {
            let n = e
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(e.szExeFile.len());
            let exe = String::from_utf16_lossy(&e.szExeFile[..n]).to_ascii_lowercase();
            out.push((e.th32ProcessID, e.th32ParentProcessID, exe));
            ok = Process32NextW(snap.0, &mut e).is_ok();
        }
    }
    Ok(out)
}

/// Whether `anc` is `pid` or one of its ancestors in `table`.
fn is_ancestor_or_self(anc: u32, mut pid: u32, table: &[(u32, u32, String)]) -> bool {
    for _ in 0..64 {
        if pid == anc {
            return true;
        }
        match table.iter().find(|(p, _, _)| *p == pid) {
            Some(&(_, parent, _)) if parent != 0 && parent != pid => pid = parent,
            _ => return false,
        }
    }
    false
}

/// Why capturing `pid` together with its child processes would also capture Smorzo or
/// voice chat (e.g. the picked app started Smorzo, or Discord), if it would.
fn tree_conflict(pid: u32, table: &[(u32, u32, String)]) -> Option<&'static str> {
    if is_ancestor_or_self(pid, std::process::id(), table) {
        return Some(NO_SMORZO);
    }
    table.iter().find_map(|(p, _, exe)| {
        let what = if SMORZO.contains(&exe.as_str()) {
            NO_SMORZO
        } else if VOICE_CHAT.contains(&exe.as_str()) {
            NO_CHAT
        } else {
            return None;
        };
        is_ancestor_or_self(pid, *p, table).then_some(what)
    })
}

/// Whether `app` may be captured right now: not blocked by name, and capturing it with
/// its child processes wouldn't also capture Smorzo or Discord.
/// Called before every start and about once a second while capturing: the app may
/// start Discord (or another Smorzo) at any time, and the capture includes it.
pub fn check(app: &AudioApp) -> Result<(), String> {
    check_with(app, &process_table()?)
}

fn check_with(app: &AudioApp, table: &[(u32, u32, String)]) -> Result<(), String> {
    if let Some(why) = blocked_reason(&app.exe) {
        return Err(why.into());
    }
    match tree_conflict(app.pid, table) {
        Some(why) => Err(format!("{why} ({} started it)", app.exe)),
        None => Ok(()),
    }
}

/// The apps from `apps` that may be picked (one process snapshot for all of them).
pub fn allowed(apps: Vec<AudioApp>) -> Result<Vec<AudioApp>, String> {
    let table = process_table()?;
    Ok(apps
        .into_iter()
        .filter(|a| check_with(a, &table).is_ok())
        .collect())
}

/// An app with an audio session right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioApp {
    /// The process that owns the session.
    pub pid: u32,
    /// Executable name, e.g. "Spotify.exe".
    pub exe: String,
    /// Playing sound at this moment (vs. just having played before).
    pub playing: bool,
}

/// COM for the calling thread for as long as this lives. If the thread already uses
/// another COM mode (e.g. a UI thread), COM is usable as it is and nothing is undone.
struct Com(bool);
impl Com {
    fn init() -> Self {
        // SAFETY: plain COM initialisation of this thread; balanced in Drop when it
        // succeeded (S_OK or S_FALSE).
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the successful CoInitializeEx above, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// The full path of a process's executable (read-only query), e.g. for its icon.
pub fn exe_path(pid: u32) -> Option<String> {
    // SAFETY: query-only handle, closed below; the buffer outlives the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// The exe name of a running process ("Spotify.exe"), or `None` if it has exited.
fn exe_name(pid: u32) -> Option<String> {
    let full = exe_path(pid)?;
    Some(full.rsplit('\\').next().unwrap_or(&full).to_string())
}

/// Apps with an audio session on any active playback device, one entry per app (the
/// session that is playing, if any). Read-only: only lists sessions, opens no stream.
/// Smorzo's own process is left out.
pub fn list_apps() -> Result<Vec<AudioApp>, String> {
    let _com = Com::init();
    let own = std::process::id();
    let mut apps: Vec<AudioApp> = Vec::new();
    let err = |e: windows::core::Error| format!("couldn't list apps playing sound: {e}");
    // SAFETY: COM calls on interfaces we own; every result is checked.
    unsafe {
        let en: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
        let devs = en
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(err)?;
        for i in 0..devs.GetCount().map_err(err)? {
            let Ok(dev) = devs.Item(i) else { continue };
            let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(list) = mgr.GetSessionEnumerator() else {
                continue;
            };
            for j in 0..list.GetCount().unwrap_or(0) {
                let Ok(s2) = list
                    .GetSession(j)
                    .and_then(|s| s.cast::<IAudioSessionControl2>())
                else {
                    continue;
                };
                let pid = s2.GetProcessId().unwrap_or(0);
                if pid == 0 || pid == own {
                    continue; // system sounds, or Smorzo itself
                }
                let Some(exe) = exe_name(pid) else { continue };
                let playing = s2.GetState().is_ok_and(|s| s == AudioSessionStateActive);
                match apps.iter_mut().find(|a| a.exe.eq_ignore_ascii_case(&exe)) {
                    Some(a) if playing && !a.playing => {
                        a.pid = pid;
                        a.playing = true;
                    }
                    Some(_) => {}
                    None => apps.push(AudioApp { pid, exe, playing }),
                }
            }
        }
    }
    apps.sort_by_key(|a| a.exe.to_ascii_lowercase());
    Ok(apps)
}

/// Worker side: takes 10 ms of the app's sound per frame. No allocation, no locks.
pub struct AppMix {
    cons: HeapCons<f32>,
    /// Waiting for `START_FRAMES` before consuming (after a gap or at the start).
    priming: bool,
}

impl AppMix {
    pub(crate) fn new(cons: HeapCons<f32>) -> Self {
        Self {
            cons,
            priming: true,
        }
    }

    /// The next 10 ms of the app into `out` (silence if none is due).
    pub fn next(&mut self, out: &mut [f32; FRAME]) {
        let queued = self.cons.occupied_len();
        if queued > MAX_FRAMES * FRAME {
            self.cons.skip(queued - START_FRAMES * FRAME);
        }
        if self.priming && self.cons.occupied_len() >= START_FRAMES * FRAME {
            self.priming = false;
        }
        if !self.priming && self.cons.occupied_len() >= FRAME {
            self.cons.pop_slice(out);
        } else {
            self.priming = true;
            out.fill(0.0);
        }
    }
}

/// Capture-thread side: downmix to mono, sanitize, queue (full ring → drop).
struct Tap {
    prod: HeapProd<f32>,
}

impl Tap {
    fn push(&mut self, data: &[f32], channels: usize) {
        for f in data.chunks_exact(channels) {
            let m = f.iter().sum::<f32>() / channels as f32;
            let m = if m.is_finite() {
                m.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let _ = self.prod.try_push(m);
        }
    }

    fn push_silence(&mut self, frames: usize) {
        for _ in 0..frames {
            let _ = self.prod.try_push(0.0);
        }
    }
}

/// A running capture of one app (dropping it stops the thread and frees everything).
pub struct AppCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub pid: u32,
    /// The captured app's exe name.
    pub exe: String,
}

impl AppCapture {
    /// The captured app, for re-checking it while it runs.
    pub fn app(&self) -> AudioApp {
        AudioApp {
            pid: self.pid,
            exe: self.exe.clone(),
            playing: true,
        }
    }
}

impl Drop for AppCapture {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing `app` (read-only process loopback, including its child processes).
/// Returns the capture and the worker-side mixer. `stats.app_failed` is set if the
/// capture stops on its own (the app exited, or Windows ended the capture).
pub fn start(
    app: &AudioApp,
    stats: Arc<crate::stats::Stats>,
) -> Result<(AppCapture, AppMix), String> {
    check(app)?;
    let (prod, cons) = ringbuf::HeapRb::<f32>::new(RING).split();
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let (pid, exe, st) = (app.pid, app.exe.clone(), Arc::clone(&stop));
    stats.app_failed.store(false, Relaxed);
    let thread = thread::Builder::new()
        .name("smorzo-app".into())
        .spawn(move || {
            // SAFETY: changes only this thread's priority (same as cpal's audio threads).
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            }
            let _com = Com::init();
            let mut tap = Tap { prod };
            if let Err(e) = run(pid, &exe, &mut tap, &st, &ready_tx) {
                // Before "ready" the caller gets the reason; after, nobody listens.
                let _ = ready_tx.send(Err(e));
            }
            if !st.load(Relaxed) {
                // Ended without being asked to (the app exited, or Windows ended it).
                stats.app_failed.store(true, Relaxed);
            }
        })
        .map_err(|e| format!("couldn't start the app capture: {e}"))?;
    let cap = AppCapture {
        stop,
        thread: Some(thread),
        pid,
        exe: app.exe.clone(),
    };
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok((cap, AppMix::new(cons))),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Windows didn't start the app capture in time".into()),
    }
}

/// Signals an event when Windows has finished activating the capture. It shares
/// ownership of the event, so a late completion (after `run` gave up waiting) still
/// signals a live event, never a closed or reused handle.
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(Arc<Handle>);
impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _op: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        // SAFETY: the event stays open for as long as this handler holds it.
        unsafe { SetEvent(self.0.0) }
    }
}
impl IAgileObject_Impl for Activated_Impl {}

/// A Win32 handle closed on drop.
struct Handle(HANDLE);
// SAFETY: a kernel handle (event, process, snapshot) may be used and closed from any
// thread; Windows synchronises access to the object itself. The completion handler
// signals the event from a Windows thread pool thread.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Handle {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: an unnamed auto-reset event owned by this struct.
        unsafe { CreateEventW(None, false, false, None) }.map(Self)
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The capture thread's body: activate, initialise, then copy packets until `stop`
/// or the app exits. Reports "ready" (or the reason it can't start) on `ready`.
fn run(
    pid: u32,
    exe: &str,
    tap: &mut Tap,
    stop: &AtomicBool,
    ready: &mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let e = |what: &str| {
        let what = what.to_string();
        move |err: windows::core::Error| format!("{what}: {err}")
    };
    // SAFETY: COM/Win32 calls with checked results; the PROPVARIANT points at `params`,
    // which outlives the activation call; the packet slice is read before ReleaseBuffer.
    unsafe {
        // The app's process, to notice when it exits. Checked by name too: if the app
        // closed and Windows gave its pid to another process meanwhile, that one is
        // never captured.
        let proc = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
        .map_err(|_| "the app has closed".to_string())?;
        let proc = Handle(proc);
        let mut buf = [0u16; 520];
        let mut n = buf.len() as u32;
        QueryFullProcessImageNameW(proc.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut n)
            .map_err(|_| "the app has closed".to_string())?;
        let name = String::from_utf16_lossy(&buf[..n as usize]);
        if !name
            .rsplit('\\')
            .next()
            .unwrap_or("")
            .eq_ignore_ascii_case(exe)
        {
            return Err("the app has closed".into());
        }

        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // ManuallyDrop: dropping a PROPVARIANT calls PropVariantClear, which would try to
        // free the blob — but it points at `params` on this stack, not at COM memory
        // (that crashed the process when the capture ended). Nothing here needs freeing.
        let mut pv = std::mem::ManuallyDrop::new(PROPVARIANT::default());
        {
            let inner = &mut *pv.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob.cbSize = size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
            inner.Anonymous.blob.pBlobData = (&raw mut params).cast::<u8>();
        }
        // Created before the client, so it outlives it on every return path.
        let packet = Handle::new().map_err(e("couldn't capture the app"))?;
        let done = Arc::new(Handle::new().map_err(e("couldn't start the app capture"))?);
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(Arc::clone(&done)).into();
        let op = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&raw const *pv),
            &handler,
        )
        .map_err(e(
            "this Windows can't capture one app's sound (needs Windows 10 2004+)",
        ))?;
        if WaitForSingleObject(done.0, 3000) != WAIT_OBJECT_0 {
            return Err("Windows didn't start the app capture in time".into());
        }
        let mut hr = windows::core::HRESULT(0);
        let mut unk = None;
        op.GetActivateResult(&mut hr, &mut unk)
            .map_err(e("couldn't capture the app"))?;
        hr.ok().map_err(e("couldn't capture the app"))?;
        let client: IAudioClient = unk
            .ok_or_else(|| "couldn't capture the app".to_string())?
            .cast()
            .map_err(e("couldn't capture the app"))?;

        // Ask for 48 kHz float stereo; Windows converts the app's sound to it.
        const CHANNELS: usize = 2;
        let fmt = WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: CHANNELS as u16,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 4 * CHANNELS as u32,
            nBlockAlign: 4 * CHANNELS as u16,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                200_000, // 20 ms, in 100 ns units
                0,
                &fmt,
                None,
            )
            .map_err(e("couldn't capture the app"))?;
        client
            .SetEventHandle(packet.0)
            .map_err(e("couldn't capture the app"))?;
        let cap: IAudioCaptureClient =
            client.GetService().map_err(e("couldn't capture the app"))?;
        client.Start().map_err(e("couldn't capture the app"))?;
        let _ = ready.send(Ok(()));

        let mut result = Ok(());
        while !stop.load(Relaxed) {
            if WaitForSingleObject(proc.0, 0) != WAIT_TIMEOUT {
                break; // the app exited
            }
            // Packets come every 10 ms; the timeout only bounds how late `stop` is seen.
            let _ = WaitForSingleObject(packet.0, 100);
            loop {
                match cap.GetNextPacketSize() {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(err) => {
                        result = Err(format!("the app capture stopped: {err}"));
                        break;
                    }
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                if let Err(err) = cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                    result = Err(format!("the app capture stopped: {err}"));
                    break;
                }
                let n = frames as usize;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    tap.push_silence(n);
                } else {
                    let s = std::slice::from_raw_parts(data as *const f32, n * CHANNELS);
                    tap.push(s, CHANNELS);
                }
                let _ = cap.ReleaseBuffer(frames);
            }
            if result.is_err() {
                break;
            }
        }
        let _ = client.Stop();
        // Release the capture before the event it signals is closed.
        drop(cap);
        drop(client);
        drop(op);
        drop(handler);
        drop(packet);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::HeapRb;

    #[test]
    fn smorzo_and_discord_are_refused() {
        assert!(blocked_reason("Smorzo.exe").is_some());
        // Smorzo's old name: an in-app update from v0.4.0 keeps the file name sotto.exe.
        assert!(blocked_reason("Sotto.exe").is_some());
        assert!(blocked_reason("Discord.exe").is_some());
        assert!(blocked_reason("DiscordPTB.exe").is_some());
        assert!(blocked_reason("Spotify.exe").is_none());
        assert!(blocked_reason("chrome.exe").is_none());
        assert!(blocked_reason("explorer.exe").is_some());
    }

    #[test]
    fn a_parent_of_smorzo_or_discord_is_refused() {
        let own = std::process::id();
        // explorer (10) started a terminal (11) that started Smorzo, and Discord (12).
        // Spotify (20) started its own helper (21).
        let table = vec![
            (10, 1, "explorer.exe".to_string()),
            (11, 10, "windowsterminal.exe".to_string()),
            (own, 11, "smorzo-cli.exe".to_string()),
            (12, 10, "discord.exe".to_string()),
            (20, 10, "spotify.exe".to_string()),
            (21, 20, "spotify.exe".to_string()),
        ];
        assert_eq!(tree_conflict(10, &table), Some(NO_SMORZO));
        assert_eq!(tree_conflict(11, &table), Some(NO_SMORZO));
        assert_eq!(tree_conflict(own, &table), Some(NO_SMORZO));
        assert_eq!(tree_conflict(12, &table), Some(NO_CHAT));
        assert_eq!(tree_conflict(20, &table), None);
        assert_eq!(tree_conflict(21, &table), None);
        // A parent loop (stale pids) ends instead of spinning.
        let looped = vec![(30, 31, "a.exe".to_string()), (31, 30, "b.exe".to_string())];
        assert!(!is_ancestor_or_self(99, 30, &looped));
    }

    #[test]
    fn tap_downmixes_and_sanitizes() {
        let (prod, mut cons) = HeapRb::<f32>::new(16).split();
        let mut tap = Tap { prod };
        tap.push(&[0.5, -0.5, 1.0, 1.0, f32::NAN, 0.0, 4.0, 4.0], 2);
        let mut out = [9.0f32; 4];
        assert_eq!(cons.pop_slice(&mut out), 4);
        assert_eq!(out, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn mix_primes_then_plays_and_fills_gaps_with_silence() {
        let (mut prod, cons) = HeapRb::<f32>::new(RING).split();
        let mut mix = AppMix::new(cons);
        let mut out = [1.0f32; FRAME];
        prod.push_slice(&[0.5; FRAME]); // one frame: still priming
        mix.next(&mut out);
        assert!(out.iter().all(|&x| x == 0.0));
        prod.push_slice(&[0.5; FRAME]); // two frames queued: starts
        mix.next(&mut out);
        assert!(out.iter().all(|&x| x == 0.5));
        mix.next(&mut out);
        assert!(out.iter().all(|&x| x == 0.5));
        mix.next(&mut out); // ran dry: silence, re-primes
        assert!(out.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn mix_drops_the_oldest_when_far_behind() {
        let (mut prod, cons) = HeapRb::<f32>::new(RING).split();
        let mut mix = AppMix::new(cons);
        for k in 0..12 {
            prod.push_slice(&[k as f32 / 100.0; FRAME]);
        }
        let mut out = [0.0f32; FRAME];
        mix.next(&mut out);
        // 12 queued > 8: trimmed to the newest 2, so the first one played is frame 10.
        assert_eq!(out[0], 0.10);
    }
}
