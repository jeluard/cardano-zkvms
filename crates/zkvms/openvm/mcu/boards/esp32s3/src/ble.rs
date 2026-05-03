use std::{
    ffi::c_void,
    ptr,
    str,
    sync::{
        atomic::{AtomicPtr, Ordering},
        Arc, Mutex as StdMutex,
    },
};

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use esp32_nimble::{
    utilities::{mutex::Mutex as NimbleMutex, BleUuid},
    uuid128, BLEAdvertisementData, BLECharacteristic, BLEDevice, NimbleProperties,
};
use openvm_mcu_device_app::transfer::{TransferError, TransferState};

const SERVICE_UUID: BleUuid = uuid128!("7b7c0001-78f1-4f9a-8b29-6f1f1d95a100");
const CONTROL_UUID: BleUuid = uuid128!("7b7c0002-78f1-4f9a-8b29-6f1f1d95a100");
const DATA_UUID: BleUuid = uuid128!("7b7c0003-78f1-4f9a-8b29-6f1f1d95a100");
const STATUS_UUID: BleUuid = uuid128!("7b7c0004-78f1-4f9a-8b29-6f1f1d95a100");
const DEVICE_NAME: &str = "ZKMCU";
const VERIFY_THREAD_STACK_BYTES: usize = 256 * 1024;
const VERIFY_TASK_PRIORITY: u32 = 5;
const VERIFY_TASK_CORE_ID: i32 = 1;

// Raw pointer to the status characteristic held during verification.
// Safe: only one verify task runs at a time; the Arc in VerifyTask keeps the
// NimbleMutex<BLECharacteristic> alive for the full duration of run_verify_task.
static STEP_CHAR_PTR: AtomicPtr<NimbleMutex<BLECharacteristic>> =
    AtomicPtr::new(ptr::null_mut());

fn report_verify_step(step: u8, label: &'static str) {
    let ptr = STEP_CHAR_PTR.load(Ordering::Acquire);
    if !ptr.is_null() {
        let msg = format!("verifying {} {}", step, label);
        // SAFETY: ptr is valid — the verify task's Arc<NimbleMutex<BLECharacteristic>>
        // keeps this allocation alive for the entire duration of verify_received_artifacts.
        unsafe { &*ptr }.lock().set_value(msg.as_bytes()).notify();
    }
}

struct VerifyTask {
    key_bytes: Vec<u8>,
    proof_bytes: Vec<u8>,
    proof_sha: String,
    status_characteristic: Arc<NimbleMutex<BLECharacteristic>>,
    state: Arc<StdMutex<TransferState>>,
}

pub fn start() -> Result<(), ()> {
    let state = Arc::new(StdMutex::new(TransferState::default()));
    let ble_device = BLEDevice::take();
    BLEDevice::set_device_name(DEVICE_NAME).map_err(|_| ())?;
    let advertising = ble_device.get_advertising();
    let server = ble_device.get_server();

    server.on_connect(|server, desc| {
        println!(
            "openvm-mcu-esp32s3-espidf: ble_connected handle={}",
            desc.conn_handle()
        );
        let _ = server.update_conn_params(desc.conn_handle(), 12, 24, 0, 600);
    });
    server.on_disconnect(|desc, reason| {
        println!(
            "openvm-mcu-esp32s3-espidf: ble_disconnected handle={} reason={:?}",
            desc.conn_handle(),
            reason
        );
        match advertising.lock().start() {
            Ok(()) => println!(
                "openvm-mcu-esp32s3-espidf: ble_advertising_restarted name={}",
                DEVICE_NAME
            ),
            Err(err) => println!(
                "openvm-mcu-esp32s3-espidf: ble_advertising_restart_failed err={:?}",
                err
            ),
        }
    });

    advertising.lock().on_complete(|reason| {
        println!(
            "openvm-mcu-esp32s3-espidf: ble_advertising_complete reason={}",
            reason
        );
        match advertising.lock().start() {
            Ok(()) => println!(
                "openvm-mcu-esp32s3-espidf: ble_advertising_restarted name={}",
                DEVICE_NAME
            ),
            Err(err) => println!(
                "openvm-mcu-esp32s3-espidf: ble_advertising_restart_failed err={:?}",
                err
            ),
        }
    });

    let service = server.create_service(SERVICE_UUID);
    let status_characteristic = service.lock().create_characteristic(
        STATUS_UUID,
        NimbleProperties::READ | NimbleProperties::NOTIFY,
    );
    status_characteristic.lock().set_value(b"ready");

    let control_characteristic = service.lock().create_characteristic(
        CONTROL_UUID,
        NimbleProperties::WRITE | NimbleProperties::WRITE_NO_RSP,
    );
    let data_characteristic = service.lock().create_characteristic(
        DATA_UUID,
        NimbleProperties::WRITE | NimbleProperties::WRITE_NO_RSP,
    );

    {
        let state = Arc::clone(&state);
        let status_characteristic = Arc::clone(&status_characteristic);
        control_characteristic.lock().on_write(move |args| {
            let command = str::from_utf8(args.recv_data()).unwrap_or("").trim();
            if command.starts_with("START ") {
                match state.lock().unwrap().start_from_command(command, now_us()) {
                    Ok(start) => {
                        send_status_with_detail(
                            &status_characteristic,
                            "receiving",
                            &format!("rx 0ms proof {}", start.proof_sha_prefix),
                        );
                    }
                    Err(error) => send_status(&status_characteristic, &format!("error {error}")),
                }
            } else if command == "COMMIT" {
                let mut state_guard = state.lock().unwrap();
                let artifacts = match state_guard.begin_verify(now_us()) {
                    Ok(artifacts) => artifacts,
                    Err(TransferError::AlreadyVerifying) => {
                        drop(state_guard);
                        send_status(&status_characteristic, "verifying");
                        return;
                    }
                    Err(error) => {
                        drop(state_guard);
                        send_status(&status_characteristic, &format!("error {error}"));
                        return;
                    }
                };
                drop(state_guard);

                send_status_with_detail(&status_characteristic, "verifying", &artifacts.detail);
                let verify_task = VerifyTask {
                    key_bytes: artifacts.key_bytes,
                    proof_bytes: artifacts.proof_bytes,
                    proof_sha: artifacts.proof_sha,
                    status_characteristic: Arc::clone(&status_characteristic),
                    state: Arc::clone(&state),
                };
                if spawn_verify_task(verify_task).is_err() {
                    send_status(&status_characteristic, "error verifier thread spawn failed");
                    if let Ok(mut state) = state.lock() {
                        state.verifying = false;
                    }
                    println!(
                        "openvm-mcu-esp32s3-espidf: ble_verify_spawn_failed stack_bytes={}",
                        VERIFY_THREAD_STACK_BYTES
                    );
                }
            } else {
                send_status(&status_characteristic, "error unknown command");
            }
        });
    }

    {
        let state = Arc::clone(&state);
        let status_characteristic = Arc::clone(&status_characteristic);
        data_characteristic.lock().on_write(move |args| {
            let packet = args.recv_data();
            match state.lock().unwrap().ingest_packet(packet, now_us()) {
                Ok(Some(detail)) => {
                    send_status_with_detail(&status_characteristic, "received", &detail)
                }
                Ok(None) => {}
                Err(error) => send_status(&status_characteristic, &format!("error {error}")),
            }
        });
    }

    advertising
        .lock()
        .scan_response(true)
        .set_data(
            BLEAdvertisementData::new()
                .name(DEVICE_NAME)
                .add_service_uuid(SERVICE_UUID),
        )
        .map_err(|_| ())?;
    advertising.lock().start().map_err(|_| ())?;
    println!(
        "openvm-mcu-esp32s3-espidf: ble_advertising name={}",
        DEVICE_NAME
    );
    render_status_screen("ready", "waiting for proof");
    Ok(())
}

fn send_status(characteristic: &Arc<NimbleMutex<BLECharacteristic>>, message: &str) {
    send_status_with_detail(characteristic, message, status_detail(message));
}

fn send_status_with_detail(
    characteristic: &Arc<NimbleMutex<BLECharacteristic>>,
    message: &str,
    detail: &str,
) {
    notify_status(characteristic, message);
    render_status_screen(message, detail);
}

fn notify_status(characteristic: &Arc<NimbleMutex<BLECharacteristic>>, message: &str) {
    characteristic.lock().set_value(message.as_bytes()).notify();
}

fn status_detail(message: &str) -> &'static str {
    if message == "ready" {
        "waiting for proof"
    } else if message == "receiving" {
        "streaming key/proof"
    } else if message == "received" {
        "transfer complete"
    } else if message == "verifying" {
        "spawning verifier"
    } else if message == "verifying halo2" {
        "native KZG check"
    } else if message == "verified" {
        "proof accepted"
    } else if message == "rejected" {
        "proof rejected"
    } else if message.starts_with("error") {
        "see BLE/serial status"
    } else {
        "BLE proof transfer"
    }
}

#[cfg(feature = "display-st7789")]
fn render_status_screen(status: &str, detail: &str) {
    let _ = crate::screen::render_ble_status(status, detail);
}

#[cfg(not(feature = "display-st7789"))]
fn render_status_screen(_status: &str, _detail: &str) {}

fn now_us() -> i64 {
    unsafe { esp_idf_sys::esp_timer_get_time() }
}

fn spawn_verify_task(task: VerifyTask) -> Result<(), ()> {
    log_heap("before_verify_task_spawn");
    let task = Box::into_raw(Box::new(task)).cast::<c_void>();
    let mut handle: esp_idf_sys::TaskHandle_t = ptr::null_mut();
    let stack_caps = esp_idf_sys::MALLOC_CAP_SPIRAM | esp_idf_sys::MALLOC_CAP_8BIT;
    let result = unsafe {
        esp_idf_sys::xTaskCreatePinnedToCoreWithCaps(
            Some(verify_task_entry),
            c"openvm-ble-verify".as_ptr(),
            VERIFY_THREAD_STACK_BYTES as u32,
            task,
            VERIFY_TASK_PRIORITY,
            &mut handle,
            VERIFY_TASK_CORE_ID,
            stack_caps,
        )
    };

    if result == 1 && !handle.is_null() {
        println!(
            "openvm-mcu-esp32s3-espidf: ble_verify_task_spawned stack_bytes={} caps=0x{:x}",
            VERIFY_THREAD_STACK_BYTES, stack_caps
        );
        Ok(())
    } else {
        unsafe { drop(Box::from_raw(task.cast::<VerifyTask>())) };
        println!(
            "openvm-mcu-esp32s3-espidf: ble_verify_task_spawn_failed result={} stack_bytes={} caps=0x{:x}",
            result,
            VERIFY_THREAD_STACK_BYTES,
            stack_caps
        );
        log_heap("after_verify_task_spawn_failed");
        Err(())
    }
}

unsafe extern "C" fn verify_task_entry(task: *mut c_void) {
    let task = unsafe { Box::from_raw(task.cast::<VerifyTask>()) };
    run_verify_task(*task);
    unsafe { esp_idf_sys::vTaskDeleteWithCaps(ptr::null_mut()) };
}

fn run_verify_task(task: VerifyTask) {
    crate::espidf_allow_long_crypto();
    println!(
        "openvm-mcu-esp32s3-espidf: ble_verify_start key_bytes={} proof_bytes={} stack_bytes={}",
        task.key_bytes.len(),
        task.proof_bytes.len(),
        VERIFY_THREAD_STACK_BYTES
    );
    log_heap("verify_task_start");
    if let Ok(mut state) = task.state.lock() {
        let detail = state.mark_verify_started(now_us());
        drop(state);
        send_status_with_detail(&task.status_characteristic, "verifying halo2", &detail);
    } else {
        send_status(&task.status_characteristic, "verifying halo2");
    }
    STEP_CHAR_PTR.store(
        Arc::as_ptr(&task.status_characteristic) as *mut _,
        Ordering::Release,
    );
    let result = crate::status::verify_received_artifacts(
        &task.key_bytes,
        &task.proof_bytes,
        task.proof_sha.clone(),
        report_verify_step,
    );
    STEP_CHAR_PTR.store(ptr::null_mut(), Ordering::Release);
    log_heap("verify_task_done");
    let final_detail = if let Ok(mut state) = task.state.lock() {
        state.mark_verify_done(now_us())
    } else {
        "timing unavailable".to_string()
    };
    match result {
        Ok(probe) => {
            let status_msg = if probe.status == crate::status::ProofStatus::Rejected {
                format!("{} {} err={}", probe.status.label(), final_detail, probe.host.detail)
            } else {
                format!("{} {}", probe.status.label(), final_detail)
            };
            notify_status(&task.status_characteristic, &status_msg);
            #[cfg(feature = "display-st7789")]
            {
                render_status_screen(probe.status.label(), &final_detail);
            }
            println!(
                "openvm-mcu-esp32s3-espidf: ble_proof_status={} {} detail={}",
                probe.status.label(),
                final_detail,
                probe.host.detail
            );
        }
        Err(()) => {
            notify_status(
                &task.status_characteristic,
                &format!("error decode failed {}", final_detail),
            );
            render_status_screen("error decode failed", &final_detail);
            println!(
                "openvm-mcu-esp32s3-espidf: ble_proof_status=decode_error {}",
                final_detail
            );
        }
    }
    if let Ok(mut state) = task.state.lock() {
        state.finish_verify();
    }
}

fn log_heap(label: &str) {
    unsafe {
        let internal_caps = esp_idf_sys::MALLOC_CAP_INTERNAL | esp_idf_sys::MALLOC_CAP_8BIT;
        let psram_caps = esp_idf_sys::MALLOC_CAP_SPIRAM | esp_idf_sys::MALLOC_CAP_8BIT;
        println!(
            "openvm-mcu-esp32s3-espidf: heap {} internal_free={} internal_largest={} psram_free={} psram_largest={}",
            label,
            esp_idf_sys::heap_caps_get_free_size(internal_caps),
            esp_idf_sys::heap_caps_get_largest_free_block(internal_caps),
            esp_idf_sys::heap_caps_get_free_size(psram_caps),
            esp_idf_sys::heap_caps_get_largest_free_block(psram_caps)
        );
    }
}
