//! Capability-aware platform integration for the native client.

use anyhow::Result;

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
    #[cfg(not(target_os = "android"))]
    {
        let _ = (title, body);
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
