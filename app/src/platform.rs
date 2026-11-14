//! Capability-aware platform integration for the native client.

use std::sync::Mutex;

use anyhow::{bail, Context, Result};

const MAX_TEXT_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug)]
pub enum FileEvent {
    Opened(String),
    Status(String),
}

static FILE_EVENT: Mutex<Option<FileEvent>> = Mutex::new(None);

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn set_file_event(event: FileEvent) {
    if let Ok(mut slot) = FILE_EVENT.lock() {
        *slot = Some(event);
    }
}

pub fn take_file_event() -> Option<FileEvent> {
    FILE_EVENT.lock().ok().and_then(|mut slot| slot.take())
}

pub fn copy_text(text: &str) -> Result<()> {
    #[cfg(target_os = "android")]
    {
        crate::android_call_string("copyToClipboard", text);
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut clipboard = arboard::Clipboard::new()?;
        clipboard.set_text(text.to_owned())?;
        Ok(())
    }
}

pub fn open_text_file() -> Result<Option<String>> {
    #[cfg(target_os = "android")]
    {
        crate::call_activity("openTextFile");
        Ok(None)
    }
    #[cfg(not(target_os = "android"))]
    {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Toloo files", &["toloo", "json", "txt"])
            .pick_file()
        else {
            return Ok(None);
        };
        let size = std::fs::metadata(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .len();
        if size > MAX_TEXT_FILE_BYTES {
            bail!("Selected file is too large");
        }
        Ok(Some(
            std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?,
        ))
    }
}

pub fn save_text_file(name: &str, text: &str) -> Result<Option<String>> {
    #[cfg(target_os = "android")]
    {
        crate::android_call_strings("saveTextFile", name, text);
        Ok(None)
    }
    #[cfg(not(target_os = "android"))]
    {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter("Toloo files", &["toloo", "json", "txt", "key"])
            .save_file()
        else {
            return Ok(None);
        };
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(Some(path.display().to_string()))
    }
}

pub fn host_started(address: &str) {
    #[cfg(target_os = "android")]
    crate::android_call_string("startRelayService", address);
    #[cfg(not(target_os = "android"))]
    let _ = address;
}

pub fn host_stopped() {
    #[cfg(target_os = "android")]
    crate::call_activity("stopRelayService");
}

pub fn enable_notifications() {
    #[cfg(target_os = "android")]
    crate::call_activity("requestNotifications");
}

pub fn notify_message(title: &str, body: &str) {
    #[cfg(target_os = "android")]
    crate::android_call_strings("notifyMessage", title, body);
    #[cfg(all(unix, not(target_os = "android")))]
    {
        let _ = notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .show();
    }
    #[cfg(target_os = "windows")]
    {
        use tauri_winrt_notification::Toast;
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(body)
            .show();
    }
}

pub fn share_text(text: &str, title: &str) -> Result<()> {
    #[cfg(target_os = "android")]
    {
        crate::android_call_strings("shareText", text, title);
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = title;
        copy_text(text)
    }
}
