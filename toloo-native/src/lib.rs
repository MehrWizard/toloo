//! Toloo native client: Slint UI over the native toloo-lib engine.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, TimerMode, VecModel};

mod engine;

slint::include_modules!();

#[cfg(target_os = "android")]
static JAVA_VM: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

fn notify_native_ready() {
    #[cfg(target_os = "android")]
    {
        use std::sync::atomic::Ordering;
        static NOTIFIED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if NOTIFIED.swap(true, Ordering::Relaxed) {
            return;
        }
        let vm = JAVA_VM.load(Ordering::Acquire);
        if vm.is_null() {
            return;
        }
        // SAFETY: the VM pointer comes from AndroidApp and remains valid for
        // the lifetime of the process.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm.cast::<jni::sys::JavaVM>()) })
        else {
            return;
        };
        if let Ok(mut env) = vm.attach_current_thread() {
            let _ = env.call_static_method(
                "com/irandawn/toloo/TolooActivity",
                "nativeReady",
                "()V",
                &[],
            );
        }
    }
}


pub fn default_data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("TOLOO_NATIVE_DATA_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("toloo-native");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("toloo-native");
        }
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data_home).join("toloo-native");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("toloo-native");
        }
    }
    PathBuf::from(".toloo-native")
}

fn apply_snapshot(ui: &AppWindow, snapshot: engine::Snapshot) {
    notify_native_ready();
    ui.set_has_identity(snapshot.has_identity);
    ui.set_identity_label(snapshot.identity_label.into());
    ui.set_status(snapshot.status.into());
    ui.set_active_room(snapshot.active_room.into());
    ui.set_members_label(snapshot.members_label.into());

    let rooms: Vec<RoomRow> = snapshot
        .rooms
        .into_iter()
        .map(|room| RoomRow {
            room_id: room.room_id.into(),
            display_name: room.display_name.into(),
            selected: room.selected,
            has_key: room.has_key,
        })
        .collect();
    ui.set_rooms(ModelRc::new(VecModel::from(rooms)));

    let messages: Vec<MessageRow> = snapshot
        .messages
        .into_iter()
        .map(|message| MessageRow {
            author: message.author.into(),
            body: message.body.into(),
            clock: message.clock.into(),
            mine: message.mine,
            committed: message.committed,
        })
        .collect();
    ui.set_messages(ModelRc::new(VecModel::from(messages)));
}

pub fn run(data_dir: PathBuf) -> Result<()> {
    let ui = AppWindow::new()?;

    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("toloo-engine".to_owned())
        .spawn(move || engine::run(data_dir, command_rx, update_tx))?;

    let tx = command_tx.clone();
    ui.on_create_identity(move || {
        let _ = tx.send(engine::Command::CreateIdentity);
    });
    let tx = command_tx.clone();
    ui.on_restore_identity(move |sig_seed, enc_priv| {
        let _ = tx.send(engine::Command::RestoreIdentity {
            sig_seed: sig_seed.to_string(),
            enc_priv: enc_priv.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_add_relay(move |url| {
        let _ = tx.send(engine::Command::AddRelay(url.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_create_room(move |name| {
        let _ = tx.send(engine::Command::CreateRoom(name.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_join_room(move |invite| {
        let _ = tx.send(engine::Command::JoinRoom(invite.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_select_room(move |room_id| {
        let _ = tx.send(engine::Command::SelectRoom(room_id.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_sync_now(move || {
        let _ = tx.send(engine::Command::Sync);
    });
    let tx = command_tx;
    ui.on_send_message(move |body| {
        let _ = tx.send(engine::Command::SendMessage(body.to_string()));
    });

    let updates = Arc::new(Mutex::new(update_rx));
    let ui_weak = ui.as_weak();
    let update_timer = slint::Timer::default();
    let timer_updates = Arc::clone(&updates);
    update_timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
        let mut latest = None;
        if let Ok(receiver) = timer_updates.lock() {
            while let Ok(snapshot) = receiver.try_recv() {
                latest = Some(snapshot);
            }
        }
        if let (Some(snapshot), Some(ui)) = (latest, ui_weak.upgrade()) {
            apply_snapshot(&ui, snapshot);
        }
    });

    ui.run()?;
    drop(update_timer);
    Ok(())
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    JAVA_VM.store(
        app.vm_as_ptr(),
        std::sync::atomic::Ordering::Release,
    );
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("."));
    if let Err(error) = slint::android::init(app) {
        eprintln!("Slint Android init failed: {error}");
        return;
    }
    if let Err(error) = run(data_dir) {
        eprintln!("Toloo failed to start: {error:#}");
    }
}
