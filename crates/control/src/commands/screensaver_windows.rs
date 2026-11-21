use super::{Command, Record, needs_restore};
use autogui::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authentication::Identity::{
    LsaFreeReturnBuffer, LsaGetLogonSessionData, LsaNtStatusToWinError,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics,
};
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, SetThreadExecutionState,
};
use windows_sys::Win32::System::StationsAndDesktops::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn win_error(operation: &'static str) -> Error {
    let code = unsafe { GetLastError() };
    if code == ERROR_ACCESS_DENIED {
        Error::PermissionDenied(operation)
    } else if code == ERROR_OPERATION_IN_PROGRESS {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "{operation}: Windows reports a power-saving or lock transition (ERROR_OPERATION_IN_PROGRESS, {code})"
            ),
        ))
    } else {
        Error::Io(std::io::Error::other(format!(
            "{operation}: {}",
            std::io::Error::from_raw_os_error(code as i32)
        )))
    }
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct Desktop(HDESK);

impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            CloseDesktop(self.0);
        }
    }
}

fn setting(action: u32) -> Result<i32> {
    let mut value = 0i32;
    if unsafe { SystemParametersInfoW(action, 0, (&mut value as *mut i32).cast(), 0) } == 0 {
        return Err(win_error("SystemParametersInfoW(read screensaver)"));
    }
    Ok(value)
}

fn set_enabled(enabled: bool) -> Result<()> {
    // fWinIni=0 changes only the current session value. It does not update the
    // user's profile, password requirement, timeout or screen-lock policy.
    if unsafe { SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE, u32::from(enabled), null_mut(), 0) }
        == 0
    {
        return Err(win_error(
            "cannot change screensaver state; unlock Windows or check policy",
        ));
    }
    if (setting(SPI_GETSCREENSAVEACTIVE)? != 0) != enabled {
        return Err(Error::PermissionDenied(
            "Windows did not retain the requested screensaver state",
        ));
    }
    Ok(())
}

struct Context {
    logon: String,
    journal: PathBuf,
    mutex_name: Vec<u16>,
    event_name: Vec<u16>,
}

impl Context {
    fn new() -> Result<Self> {
        let mut raw_token = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
            return Err(win_error("OpenProcessToken"));
        }
        let token = Handle(raw_token);
        let mut statistics = TOKEN_STATISTICS::default();
        let mut length = 0;
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenStatistics,
                (&mut statistics as *mut TOKEN_STATISTICS).cast(),
                size_of::<TOKEN_STATISTICS>() as u32,
                &mut length,
            )
        } == 0
        {
            return Err(win_error("GetTokenInformation"));
        }
        // LUIDs can be reused after reboot. Include the logon timestamp so an
        // old recovery record cannot be applied to a subsequent Windows login.
        let mut session = null_mut();
        let status = unsafe { LsaGetLogonSessionData(&statistics.AuthenticationId, &mut session) };
        if status != 0 {
            let code = unsafe { LsaNtStatusToWinError(status) };
            return Err(Error::Io(std::io::Error::from_raw_os_error(code as i32)));
        }
        if session.is_null() {
            return Err(Error::PermissionDenied(
                "screensaver controls require an interactive user logon",
            ));
        }
        let logon_time = unsafe { (*session).LogonTime };
        unsafe {
            LsaFreeReturnBuffer(session.cast());
        }
        if logon_time <= 0 {
            return Err(Error::PermissionDenied(
                "screensaver controls require a user logon timestamp",
            ));
        }
        let logon = format!(
            "{:08x}{:08x}-{:016x}",
            statistics.AuthenticationId.HighPart as u32,
            statistics.AuthenticationId.LowPart,
            logon_time
        );
        let local = std::env::var_os("LOCALAPPDATA")
            .ok_or(Error::InvalidArgument("LOCALAPPDATA is unavailable"))?;
        let journal = PathBuf::from(local)
            .join("autogui")
            .join(format!("screensaver-pause-{logon}.state"));
        Ok(Self {
            mutex_name: wide(&format!("Local\\AutoGui.ScreenSaver.{logon}.Mutex")),
            event_name: wide(&format!("Local\\AutoGui.ScreenSaver.{logon}.Resume")),
            logon,
            journal,
        })
    }

    fn record(&self) -> Result<Option<Record>> {
        let file = match File::open(&self.journal) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut text = String::new();
        file.take(513).read_to_string(&mut text)?;
        if text.len() > 512 {
            return Err(Error::InvalidArgument(
                "screensaver recovery record is too large",
            ));
        }
        Record::decode(&text, &self.logon).map(Some)
    }

    fn save(&self, record: &Record) -> Result<()> {
        let parent = self
            .journal
            .parent()
            .ok_or(Error::InvalidArgument("invalid recovery directory"))?;
        fs::create_dir_all(parent)?;
        // An interrupted write is kept for diagnosis; settings have not yet
        // been touched. Never replace a pre-existing recovery record.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.journal)?;
        file.write_all(record.encode(&self.logon).as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    fn restore(&self) -> Result<bool> {
        let Some(record) = self.record()? else {
            return Ok(false);
        };
        if needs_restore(&record, setting(SPI_GETSCREENSAVEACTIVE)? != 0) {
            set_enabled(true)?;
        }
        // Only remove the journal after readback/restoration has succeeded.
        fs::remove_file(&self.journal)?;
        Ok(true)
    }
}

struct MutexGuard(Handle);

impl MutexGuard {
    fn acquire(context: &Context, milliseconds: u32) -> Result<Option<Self>> {
        let raw = unsafe { CreateMutexW(null(), 0, context.mutex_name.as_ptr()) };
        if raw.is_null() {
            return Err(win_error("CreateMutexW"));
        }
        let handle = Handle(raw);
        match unsafe { WaitForSingleObject(handle.0, milliseconds) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(Self(handle))),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(win_error("WaitForSingleObject(screensaver mutex)")),
        }
    }
}

impl Drop for MutexGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0.0);
        }
    }
}

fn resume_event(context: &Context) -> Result<Handle> {
    // Consume a resume signal when the sole pause owner observes it. Do not
    // reset at startup: that would discard a concurrent resume request.
    let raw = unsafe { CreateEventW(null(), 0, 0, context.event_name.as_ptr()) };
    if raw.is_null() {
        return Err(win_error("CreateEventW"));
    }
    Ok(Handle(raw))
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn console_handler(event: u32) -> i32 {
    if event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT {
        INTERRUPTED.store(true, Ordering::SeqCst);
        1
    } else {
        // Force-close, logoff and shutdown can terminate the process before
        // cleanup. The durable journal is recovered by `screensaver resume`.
        0
    }
}

struct ConsoleGuard(bool);

impl ConsoleGuard {
    fn install() -> Result<Self> {
        INTERRUPTED.store(false, Ordering::SeqCst);
        if unsafe { SetConsoleCtrlHandler(Some(console_handler), 1) } != 0 {
            return Ok(Self(true));
        }
        if unsafe { GetLastError() } == ERROR_INVALID_HANDLE {
            // A detached process has no console; timer and resume still work.
            return Ok(Self(false));
        }
        Err(win_error("SetConsoleCtrlHandler"))
    }
}

impl Drop for ConsoleGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                SetConsoleCtrlHandler(Some(console_handler), 0);
            }
        }
    }
}

struct PauseGuard<'a> {
    context: &'a Context,
    restore_on_drop: bool,
}

struct DisplayGuard(u32);

impl DisplayGuard {
    fn acquire() -> Result<Self> {
        // This only holds the display awake. Screensaver suppression still
        // requires SPI_SETSCREENSAVEACTIVE; neither operation unlocks Windows.
        let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) };
        if previous == 0 {
            return Err(win_error("SetThreadExecutionState(display awake)"));
        }
        Ok(Self(previous))
    }
}

impl Drop for DisplayGuard {
    fn drop(&mut self) {
        unsafe {
            SetThreadExecutionState(self.0 | ES_CONTINUOUS);
        }
    }
}

impl PauseGuard<'_> {
    fn finish(&mut self) -> Result<()> {
        self.restore_on_drop = false;
        self.context.restore()?;
        Ok(())
    }
}

impl Drop for PauseGuard<'_> {
    fn drop(&mut self) {
        if self.restore_on_drop
            && let Err(error) = self.context.restore()
        {
            let _ = writeln!(
                std::io::stderr(),
                "還原失敗：{error}；請解除鎖定後執行 screensaver resume。紀錄：{}",
                self.context.journal.display()
            );
        }
    }
}

fn pause(seconds: u64, managed: bool) -> Result<()> {
    let context = Context::new()?;
    let _lock = MutexGuard::acquire(&context, 0)?.ok_or(Error::InvalidArgument(
        "another screensaver pause is active; use screensaver resume first",
    ))?;
    if context.record()?.is_some() {
        return Err(Error::InvalidArgument(
            "unfinished screensaver recovery record; run screensaver resume first",
        ));
    }
    let event = resume_event(&context)?;
    let _console = ConsoleGuard::install()?;
    if managed {
        // A pipe owned by the MCP parent is the lease. EOF, including parent
        // termination, restores settings without needing the parent to survive.
        std::thread::spawn(|| {
            let mut buffer = [0u8; 1];
            while let Ok(count) = std::io::stdin().read(&mut buffer) {
                if count == 0 {
                    break;
                }
            }
            INTERRUPTED.store(true, Ordering::SeqCst);
        });
    }
    let _display = DisplayGuard::acquire()?;
    let record = Record {
        original_enabled: setting(SPI_GETSCREENSAVEACTIVE)? != 0,
    };
    context.save(&record)?;
    let mut guard = PauseGuard {
        context: &context,
        restore_on_drop: true,
    };
    if record.original_enabled {
        set_enabled(false)?;
    }
    if managed {
        println!("AUTOGUI_PAUSE_READY");
    }
    println!(
        "已暫停螢幕保護程式自動啟動，{seconds} 秒後還原；Ctrl+C 或另開終端執行 screensaver resume 可提早還原。"
    );
    println!(
        "原本啟用：{}；還原紀錄：{}",
        record.original_enabled,
        context.journal.display()
    );
    std::io::stdout().flush()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) && !INTERRUPTED.load(Ordering::SeqCst) {
        match unsafe { WaitForSingleObject(event.0, 100) } {
            WAIT_OBJECT_0 => break,
            WAIT_TIMEOUT => {}
            _ => return Err(win_error("WaitForSingleObject(screensaver resume)")),
        }
        if setting(SPI_GETSCREENSAVEACTIVE)? != 0 {
            return Err(Error::PermissionDenied(
                "screensaver was re-enabled externally; pause ended",
            ));
        }
    }
    guard.finish()?;
    println!("暫停已結束，已完成原設定還原。");
    Ok(())
}

fn resume() -> Result<()> {
    let context = Context::new()?;
    // Keep the signal alive while waiting for the owner. Creating an event is
    // safe even if the pause is just starting, so a resume request is not lost.
    let event = resume_event(&context)?;
    if unsafe { SetEvent(event.0) } == 0 {
        return Err(win_error("SetEvent"));
    }
    let _lock = MutexGuard::acquire(&context, 5_000)?.ok_or(Error::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "pause did not finish within 5 seconds; recovery record is preserved",
    )))?;
    // Close the signal handle before releasing the mutex for the next pause.
    drop(event);
    if context.restore()? {
        println!("已依暫停紀錄還原原設定。");
    } else {
        println!("目前沒有待還原的暫停紀錄；執行中的暫停已結束。");
    }
    println!(
        "螢幕保護程式啟用：{}",
        setting(SPI_GETSCREENSAVEACTIVE)? != 0
    );
    Ok(())
}

pub(crate) fn input_desktop() -> Result<String> {
    let raw = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    if raw.is_null() {
        return Err(win_error("OpenInputDesktop(read)"));
    }
    let desktop = Desktop(raw);
    let mut name = [0u16; 256];
    let mut needed = 0;
    if unsafe {
        GetUserObjectInformationW(
            desktop.0,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            size_of_val(&name) as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(win_error("GetUserObjectInformationW"));
    }
    let length = name.iter().position(|&ch| ch == 0).unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..length]))
}

fn print_desktop() {
    match input_desktop() {
        Ok(name) => println!("目前輸入桌面：{name}"),
        Err(error) => println!("目前輸入桌面：無法讀取（{error}）"),
    }
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) } == 0 {
        println!(
            "桌面游標存取：不可用（Windows 錯誤 {}）；若已鎖定仍需登入。",
            unsafe { GetLastError() }
        );
    } else {
        println!("桌面游標存取：可用");
    }
}

fn status() -> Result<()> {
    println!(
        "螢幕保護程式啟用：{}",
        setting(SPI_GETSCREENSAVEACTIVE)? != 0
    );
    println!("目前執行中：{}", setting(SPI_GETSCREENSAVERRUNNING)? != 0);
    println!("閒置啟動秒數：{}", setting(SPI_GETSCREENSAVETIMEOUT)?);
    println!("恢復時需要登入：{}", setting(SPI_GETSCREENSAVESECURE)? != 0);
    // Preserve useful diagnostics even when a corrupt recovery record causes
    // the rest of this command to return an error.
    print_desktop();
    let context = Context::new()?;
    let lock = MutexGuard::acquire(&context, 0)?;
    if lock.is_none() {
        println!("本工具暫停：執行中");
    } else if context.record()?.is_some() {
        println!("本工具暫停：有未完成的還原紀錄，請執行 screensaver resume");
    } else {
        println!("本工具暫停：未啟用");
    }
    Ok(())
}

unsafe extern "system" fn collect_windows(hwnd: HWND, data: LPARAM) -> i32 {
    let windows = unsafe { &mut *(data as *mut Vec<HWND>) };
    windows.push(hwnd);
    1
}

fn saver_process(hwnd: HWND) -> Result<Option<(u32, Handle)>> {
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0 {
        return Ok(None);
    }
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw.is_null() {
        return Err(win_error("OpenProcess(screen saver identity)"));
    }
    let process = Handle(raw);
    let mut image = vec![0u16; 32_768];
    let mut length = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, 0, image.as_mut_ptr(), &mut length) } == 0 {
        return Err(win_error("QueryFullProcessImageNameW"));
    }
    let image = String::from_utf16_lossy(&image[..length as usize]);
    if !image.to_ascii_lowercase().ends_with(".scr") {
        return Ok(None);
    }
    Ok(Some((pid, process)))
}

fn stop() -> Result<()> {
    if setting(SPI_GETSCREENSAVERRUNNING)? == 0 {
        println!("目前沒有正在執行的螢幕保護程式。");
        print_desktop();
        return Ok(());
    }
    let name = wide("Screen-saver");
    let raw = unsafe {
        OpenDesktopW(
            name.as_ptr(),
            0,
            0,
            DESKTOP_READOBJECTS | DESKTOP_WRITEOBJECTS,
        )
    };
    if raw.is_null() {
        return Err(win_error(
            "cannot access Screen-saver desktop; unlock Windows or dismiss the screen saver manually",
        ));
    }
    let desktop = Desktop(raw);
    let mut windows = Vec::<HWND>::new();
    if unsafe {
        EnumDesktopWindows(
            desktop.0,
            Some(collect_windows),
            (&mut windows as *mut Vec<HWND>) as isize,
        )
    } == 0
    {
        return Err(win_error("EnumDesktopWindows(Screen-saver)"));
    }
    let mut posted = 0;
    let mut identity_error = None;
    for hwnd in windows {
        match saver_process(hwnd) {
            Ok(Some((pid, _process))) => {
                let mut current_pid = 0;
                if unsafe { GetWindowThreadProcessId(hwnd, &mut current_pid) } == 0
                    || current_pid != pid
                {
                    continue;
                }
                // Only .scr windows on the dedicated saver desktop. Never
                // close foreground/default-desktop applications or kill a process.
                if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } == 0 {
                    return Err(win_error("PostMessageW(screen saver WM_CLOSE)"));
                }
                posted += 1;
            }
            Ok(None) => {}
            Err(error) => {
                identity_error = Some(error);
            }
        }
    }
    if posted == 0 {
        if setting(SPI_GETSCREENSAVERRUNNING)? == 0 {
            println!("螢幕保護程式已自行結束。");
            print_desktop();
            return Ok(());
        }
        return Err(identity_error.unwrap_or(Error::PermissionDenied(
            "no identifiable .scr window on Screen-saver desktop; dismiss it manually",
        )));
    }
    let started = Instant::now();
    while setting(SPI_GETSCREENSAVERRUNNING)? != 0 {
        if started.elapsed() >= Duration::from_secs(2) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "screen saver did not confirm shutdown within 2 seconds; it may require sign-in",
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("已停止目前的螢幕保護程式；自動啟動設定維持原值。");
    print_desktop();
    Ok(())
}

pub(super) fn run(command: Command) -> Result<()> {
    match command {
        Command::Status => status(),
        Command::Pause(seconds) => pause(seconds, false),
        Command::ManagedPause(seconds) => pause(seconds, true),
        Command::Resume => resume(),
        Command::Stop => stop(),
    }
}
