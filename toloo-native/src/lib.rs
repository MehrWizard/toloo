//! Toloo native client: Slint UI over the native toloo-lib engine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, VecModel};

mod engine;
mod platform;

slint::include_modules!();

#[cfg(target_os = "android")]
static JAVA_VM: std::sync::atomic::AtomicPtr<std::ffi::c_void> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "android")]
fn call_activity(method: &str) {
    use std::sync::atomic::Ordering;
    let vm = JAVA_VM.load(Ordering::Acquire);
    if vm.is_null() {
        return;
    }
    // SAFETY: the VM pointer comes from AndroidApp and remains valid for the
    // lifetime of the process.
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm.cast::<jni::sys::JavaVM>()) }) else {
        return;
    };
    if let Ok(mut env) = vm.attach_current_thread() {
        let _ = env.call_static_method("com/irandawn/toloo/TolooActivity", method, "()V", &[]);
    };
}

#[cfg(target_os = "android")]
fn android_call_string(method: &str, value: &str) {
    use jni::objects::JValue;
    use std::sync::atomic::Ordering;
    let vm = JAVA_VM.load(Ordering::Acquire);
    if vm.is_null() {
        return;
    }
    // SAFETY: the VM pointer comes from AndroidApp and remains valid for the
    // lifetime of the process.
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm.cast::<jni::sys::JavaVM>()) }) else {
        return;
    };
    if let Ok(mut env) = vm.attach_current_thread() {
        if let Ok(value) = env.new_string(value) {
            let _ = env.call_static_method(
                "com/irandawn/toloo/TolooActivity",
                method,
                "(Ljava/lang/String;)V",
                &[JValue::Object(&value)],
            );
        }
    }
}

#[cfg(target_os = "android")]
fn android_call_strings(method: &str, first: &str, second: &str) {
    use jni::objects::JValue;
    use std::sync::atomic::Ordering;
    let vm = JAVA_VM.load(Ordering::Acquire);
    if vm.is_null() {
        return;
    }
    // SAFETY: the VM pointer comes from AndroidApp and remains valid for the
    // lifetime of the process.
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm.cast::<jni::sys::JavaVM>()) }) else {
        return;
    };
    if let Ok(mut env) = vm.attach_current_thread() {
        if let (Ok(first), Ok(second)) = (env.new_string(first), env.new_string(second)) {
            let _ = env.call_static_method(
                "com/irandawn/toloo/TolooActivity",
                method,
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&first), JValue::Object(&second)],
            );
        }
    }
}

#[cfg(target_os = "android")]
static FRAME_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "android")]
static CONTENT_SNAPSHOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "android")]
static CONTENT_NOTIFIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "android")]
fn maybe_notify_content() {
    use std::sync::atomic::Ordering;
    if FRAME_READY.load(Ordering::Acquire)
        && CONTENT_SNAPSHOT.load(Ordering::Acquire)
        && !CONTENT_NOTIFIED.swap(true, Ordering::AcqRel)
    {
        call_activity("contentReady");
    }
}

fn notify_native_ready() {
    #[cfg(target_os = "android")]
    {
        use std::sync::atomic::Ordering;
        static NOTIFIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if NOTIFIED.swap(true, Ordering::Relaxed) {
            return;
        }
        FRAME_READY.store(true, Ordering::Release);
        call_activity("nativeReady");
        maybe_notify_content();
    }
}

fn notify_content_snapshot() {
    #[cfg(target_os = "android")]
    {
        use std::sync::atomic::Ordering;
        CONTENT_SNAPSHOT.store(true, Ordering::Release);
        maybe_notify_content();
    }
}

static NOTIFIED_ROOMS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn maybe_notify(
    room_id: &str,
    room_name: &str,
    messages: &[engine::MessageView],
    enabled: bool,
) {
    if !enabled || room_id.is_empty() {
        return;
    }
    let Some(latest) = messages.iter().rev().find(|message| !message.mine) else {
        return;
    };
    let Ok(mut guard) = NOTIFIED_ROOMS.lock() else {
        return;
    };
    let seen = guard.get_or_insert_with(HashMap::new);
    if let Some(previous) = seen.get(room_id) {
        if previous != &latest.eid {
            platform::notify_message(
                &format!("Toloo · {room_name}"),
                &latest.body,
            );
        }
    }
    seen.insert(room_id.to_owned(), latest.eid.clone());
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
    let notify_room_id = snapshot.active_room_id.clone();
    let notify_room_name = snapshot.active_room.clone();
    maybe_notify(
        &notify_room_id,
        &notify_room_name,
        &snapshot.messages,
        snapshot.notifications_enabled,
    );
    ui.set_has_identity(snapshot.has_identity);
    ui.set_identity_label(snapshot.identity_label.into());
    ui.set_identity_initial(snapshot.identity_initial.into());
    ui.set_identity_hue(snapshot.identity_hue);
    ui.set_identity_sig_pub(snapshot.identity_sig_pub.into());
    ui.set_identity_enc_pub(snapshot.identity_enc_pub.into());
    ui.set_status(snapshot.status.into());
    ui.set_network_label(snapshot.network_label.into());
    ui.set_active_room(snapshot.active_room.into());
    ui.set_active_room_id(snapshot.active_room_id.into());
    ui.set_room_key(snapshot.room_key.into());
    ui.set_room_initial(snapshot.room_initial.into());
    ui.set_room_hue(snapshot.room_hue);
    ui.set_room_has_key(snapshot.room_has_key);
    ui.set_members_label(snapshot.members_label.into());
    if ui.get_sheet() == 6 && ui.get_selected_room_id().is_empty() {
        ui.set_selected_room_id(ui.get_active_room_id());
        ui.set_selected_room_name(ui.get_active_room());
        ui.set_selected_room_has_key(ui.get_room_has_key());
    }
    ui.set_rtl(snapshot.rtl);
    ui.set_notifications_enabled(snapshot.notifications_enabled);
    ui.set_host_running(snapshot.host_running);
    ui.set_host_address(snapshot.host_address.into());
    ui.set_host_connections(snapshot.host_connections);
    ui.set_host_port(snapshot.host_port.into());
    ui.set_warming(snapshot.warming);
    ui.set_export_text(snapshot.export_text.into());
    ui.set_export_kind(snapshot.export_kind.into());

    let relays: Vec<RelayRow> = snapshot
        .relays
        .into_iter()
        .map(|relay| RelayRow {
            url: relay.url.into(),
        })
        .collect();
    ui.set_relays(ModelRc::new(VecModel::from(relays)));

    let blocked: Vec<BlockedRow> = snapshot
        .blocked
        .into_iter()
        .map(|blocked| BlockedRow {
            node_id: blocked.node_pub.into(),
            short: blocked.short.into(),
            kind: blocked.kind.into(),
            reason: blocked.reason.into(),
        })
        .collect();
    ui.set_blocked(ModelRc::new(VecModel::from(blocked)));

    let rooms: Vec<RoomRow> = snapshot
        .rooms
        .into_iter()
        .map(|room| RoomRow {
            room_id: room.room_id.into(),
            display_name: room.display_name.into(),
            initial: room.initial.into(),
            avatar_hue: room.avatar_hue,
            selected: room.selected,
            has_key: room.has_key,
            preview: room.preview.into(),
            time_label: room.time_label.into(),
            unread: room.unread,
            member_count: room.member_count,
        })
        .collect();
    ui.set_rooms(ModelRc::new(VecModel::from(rooms)));

    if ui.get_sheet() == 7 && ui.get_selected_message_eid().is_empty() {
        if let Some(message) = snapshot.messages.last() {
            ui.set_selected_message_eid(message.eid.clone().into());
            ui.set_selected_message_author(message.author.clone().into());
            ui.set_selected_message_author_id(message.author_id.clone().into());
            ui.set_selected_message_body(message.body.clone().into());
            ui.set_selected_message_mine(message.mine);
        }
    }

    let messages: Vec<MessageRow> = snapshot
        .messages
        .into_iter()
        .map(|message| MessageRow {
            eid: message.eid.into(),
            author: message.author.into(),
            author_id: message.author_id.into(),
            author_initial: message.author_initial.into(),
            avatar_hue: message.avatar_hue,
            body: message.body.into(),
            clock: message.clock.into(),
            mine: message.mine,
            committed: message.committed,
            status: message.status.into(),
            day_label: message.day_label.into(),
            new_day: message.new_day,
            grouped: message.grouped,
        })
        .collect();
    ui.set_messages(ModelRc::new(VecModel::from(messages)));

    if !ui.get_warming() {
        notify_content_snapshot();
    }
}

fn start_engine(
    data_dir: PathBuf,
) -> Result<(
    mpsc::Sender<engine::Command>,
    mpsc::Receiver<engine::Snapshot>,
)> {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("toloo-engine".to_owned())
        .spawn(move || engine::run(data_dir, command_rx, update_tx))?;
    Ok((command_tx, update_rx))
}

fn run_ui(
    command_tx: mpsc::Sender<engine::Command>,
    update_rx: mpsc::Receiver<engine::Snapshot>,
) -> Result<()> {
    let ui = AppWindow::new()?;

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
    let tx = command_tx.clone();
    ui.on_send_message(move |body| {
        let _ = tx.send(engine::Command::SendMessage(body.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_search_changed(move |query| {
        let _ = tx.send(engine::Command::SetSearch(query.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_load_identity_json(move |json| {
        let _ = tx.send(engine::Command::LoadIdentityJson(json.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_export_identity(move || {
        let _ = tx.send(engine::Command::ExportIdentity);
    });
    let tx = command_tx.clone();
    ui.on_export_encrypted_identity(move |passphrase| {
        let _ = tx.send(engine::Command::ExportEncryptedIdentity(
            passphrase.to_string(),
        ));
    });
    let tx = command_tx.clone();
    ui.on_load_encrypted_identity(move |blob, passphrase| {
        let _ = tx.send(engine::Command::LoadEncryptedIdentity {
            blob: blob.to_string(),
            passphrase: passphrase.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_remove_identity(move || {
        let _ = tx.send(engine::Command::RemoveIdentity);
    });
    let tx = command_tx.clone();
    ui.on_remove_relay(move |url| {
        let _ = tx.send(engine::Command::RemoveRelay(url.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_import_discovery(move |input| {
        let _ = tx.send(engine::Command::ImportDiscovery(input.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_export_invite(move |room_id| {
        let _ = tx.send(engine::Command::ExportInvite(room_id.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_export_messages(move |room_id| {
        let _ = tx.send(engine::Command::ExportMessages(room_id.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_rename_room(move |room_id, name| {
        let _ = tx.send(engine::Command::RenameRoom {
            room_id: room_id.to_string(),
            name: name.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_leave_room(move |room_id| {
        let _ = tx.send(engine::Command::LeaveRoom(room_id.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_remove_room(move |room_id| {
        let _ = tx.send(engine::Command::RemoveRoom(room_id.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_delete_message(move |room_id, target| {
        let _ = tx.send(engine::Command::DeleteMessage {
            room_id: room_id.to_string(),
            target: target.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_flag_message(move |room_id, target, category| {
        let _ = tx.send(engine::Command::FlagMessage {
            room_id: room_id.to_string(),
            target: target.to_string(),
            category: category.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_block_node(move |node_pub, kind, reason| {
        let _ = tx.send(engine::Command::BlockNode {
            node_pub: node_pub.to_string(),
            kind: kind.to_string(),
            reason: reason.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_unblock_node(move |node_pub| {
        let _ = tx.send(engine::Command::UnblockNode(node_pub.to_string()));
    });
    let tx = command_tx.clone();
    ui.on_attest_node(move |target, level| {
        let _ = tx.send(engine::Command::AttestNode {
            target: target.to_string(),
            level: level.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_start_host(move |port, proto| {
        let _ = tx.send(engine::Command::StartHost {
            port: port.trim().parse().unwrap_or(0),
            proto: proto.to_string(),
        });
    });
    let tx = command_tx.clone();
    ui.on_stop_host(move || {
        let _ = tx.send(engine::Command::StopHost);
    });
    let tx = command_tx.clone();
    ui.on_set_notifications(move |enabled| {
        if enabled {
            platform::enable_notifications();
        }
        let _ = tx.send(engine::Command::SetNotifications(enabled));
    });
    let tx = command_tx;
    ui.on_set_rtl(move |enabled| {
        let _ = tx.send(engine::Command::SetRtl(enabled));
    });

    let status_weak = ui.as_weak();
    ui.on_copy_text(move |text| {
        let message = match platform::copy_text(text.as_str()) {
            Ok(()) => "Copied to clipboard".to_owned(),
            Err(error) => format!("Could not copy: {error:#}"),
        };
        if let Some(ui) = status_weak.upgrade() {
            ui.set_status(message.into());
        }
    });
    let status_weak = ui.as_weak();
    ui.on_share_text(move |text| {
        let message = match platform::share_text(text.as_str(), "Toloo") {
            Ok(()) => "Share opened".to_owned(),
            Err(error) => format!("Could not share: {error:#}"),
        };
        if let Some(ui) = status_weak.upgrade() {
            ui.set_status(message.into());
        }
    });

    if ui
        .window()
        .set_rendering_notifier(|state, _graphics| {
            if matches!(state, slint::RenderingState::AfterRendering) {
                notify_native_ready();
            }
        })
        .is_err()
    {
        notify_native_ready();
    }

    let ui_weak = ui.as_weak();
    std::thread::Builder::new()
        .name("toloo-ui-updates".to_owned())
        .spawn(move || {
            while let Ok(snapshot) = update_rx.recv() {
                let ui_weak = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        apply_snapshot(&ui, snapshot);
                    }
                });
            }
        })?;

    ui.run()?;
    Ok(())
}

pub fn run(data_dir: PathBuf) -> Result<()> {
    let (command_tx, update_rx) = start_engine(data_dir)?;
    run_ui(command_tx, update_rx)
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    JAVA_VM.store(app.vm_as_ptr(), std::sync::atomic::Ordering::Release);
    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("."));
    let started = std::time::Instant::now();
    let (command_tx, update_rx) = match start_engine(data_dir) {
        Ok(channels) => channels,
        Err(error) => {
            eprintln!("Toloo engine failed to start: {error:#}");
            return;
        }
    };
    eprintln!(
        "toloo: engine started at {}ms",
        started.elapsed().as_millis()
    );
    if let Err(error) = slint::android::init(app) {
        eprintln!("Slint Android init failed: {error}");
        return;
    }
    eprintln!(
        "toloo: Slint initialized at {}ms",
        started.elapsed().as_millis()
    );
    if let Err(error) = run_ui(command_tx, update_rx) {
        eprintln!("Toloo failed to start: {error:#}");
    }
}
