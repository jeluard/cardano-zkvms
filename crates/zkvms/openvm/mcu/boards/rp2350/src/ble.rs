use alloc::{format, string::String, vec::Vec};
use core::{fmt::Write, ptr::addr_of_mut};

use bt_hci::{cmd::le::LeReadLocalSupportedFeatures, controller::ControllerCmdSync};
use cyw43::aligned_bytes;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use embassy_executor::Executor;
use embassy_futures::{
    join::join,
    select::{select, select3, Either, Either3},
};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::multicore::{self, Stack as Core1Stack};
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::{bind_interrupts, dma};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
};
use heapless::Vec as HVec;
use static_cell::StaticCell;
use trouble_host::prelude::*;

use crate::proof_app::{self, Event, ProofApp};

const MAX_WRITE_BYTES: usize = 244;
const MAX_STATUS_BYTES: usize = 244;
const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 2;
const RECEIVE_IDLE_RENDER_INTERVAL_US: i64 = 1_000_000;
const RECEIVE_PROGRESS_STEP_PERCENT: usize = 5;
const BLE_TARGET_CONN_INTERVAL_MS: u64 = 15;
const BLE_SUPERVISION_TIMEOUT_SECS: u64 = 4;
const CORE1_STACK_BYTES: usize = 96 * 1024;
const BLE_ADDRESS: [u8; 6] = [0x5a, 0x95, 0x1d, 0x1f, 0x6f, 0x7b];
const SERVICE_UUID_LE: [u8; 16] = [
    0x00, 0xa1, 0x95, 0x1d, 0x1f, 0x6f, 0x29, 0x8b, 0x9a, 0x4f, 0xf1, 0x78, 0x01, 0x00, 0x7c,
    0x7b,
];

type WriteValue = HVec<u8, MAX_WRITE_BYTES>;
type StatusValue = HVec<u8, MAX_STATUS_BYTES>;

struct VerifyRequest {
    key_bytes: Vec<u8>,
    proof_bytes: Vec<u8>,
    proof_sha: String,
}

enum VerifyOutcome {
    Probe(openvm_mcu_device_app::ProofProbe),
    DecodeFailed,
}

impl VerifyOutcome {
    fn into_result(self) -> Result<openvm_mcu_device_app::ProofProbe, ()> {
        match self {
            Self::Probe(probe) => Ok(probe),
            Self::DecodeFailed => Err(()),
        }
    }
}

impl From<openvm_mcu_device_app::transfer::VerifyArtifacts> for VerifyRequest {
    fn from(artifacts: openvm_mcu_device_app::transfer::VerifyArtifacts) -> Self {
        Self {
            key_bytes: artifacts.key_bytes,
            proof_bytes: artifacts.proof_bytes,
            proof_sha: artifacts.proof_sha,
        }
    }
}

static VERIFY_REQUESTS: Channel<CriticalSectionRawMutex, VerifyRequest, 1> = Channel::new();
static VERIFY_RESULTS: Channel<CriticalSectionRawMutex, VerifyOutcome, 1> = Channel::new();
static VERIFY_PROGRESS: Channel<CriticalSectionRawMutex, (u8, &'static str), 8> = Channel::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
static mut CORE1_STACK: Core1Stack<CORE1_STACK_BYTES> = Core1Stack::new();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
});

#[gatt_server]
struct Server {
    proof: ProofService,
}

#[gatt_service(uuid = "7b7c0001-78f1-4f9a-8b29-6f1f1d95a100")]
struct ProofService {
    #[characteristic(uuid = "7b7c0002-78f1-4f9a-8b29-6f1f1d95a100", write, write_without_response)]
    control: WriteValue,
    #[characteristic(uuid = "7b7c0003-78f1-4f9a-8b29-6f1f1d95a100", write, write_without_response)]
    data: WriteValue,
    #[characteristic(uuid = "7b7c0004-78f1-4f9a-8b29-6f1f1d95a100", read, notify)]
    status: StatusValue,
}

#[embassy_executor::task]
async fn cyw43_task(runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn verifier_core1_task() -> ! {
    loop {
        let request = VERIFY_REQUESTS.receive().await;
        let outcome = match openvm_mcu_device_app::verify_received_artifacts(
            &request.key_bytes,
            &request.proof_bytes,
            request.proof_sha,
            report_verify_step,
        ) {
            Ok(probe) => VerifyOutcome::Probe(probe),
            Err(()) => VerifyOutcome::DecodeFailed,
        };
        VERIFY_RESULTS.send(outcome).await;
    }
}

fn report_verify_step(step: u8, label: &'static str) {
    VERIFY_PROGRESS.try_send((step, label)).ok();
}

pub async fn main(spawner: embassy_executor::Spawner) -> ! {
    let embassy_rp::Peripherals {
        CORE1,
        PIN_23,
        PIN_24,
        PIN_25,
        PIN_29,
        PIO0,
        DMA_CH0,
        ..
    } = embassy_rp::init(Default::default());

    spawn_verifier_core1(CORE1);

    crate::tufty_display::init();

    if option_env!("OPENVM_MCU_RP2350_VISUAL_ONLY").is_some() {
        loop {
            crate::tufty_display::visual_test_cycle();
        }
    }

    let pwr = Output::new(PIN_23, Level::Low);
    let cs = Output::new(PIN_25, Level::High);
    let mut pio = Pio::new(PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        PIN_24,
        PIN_29,
        dma::Channel::new(DMA_CH0, Irqs),
    );

    let mut app = ProofApp::default();
    crate::render_event(app.ready());

    let (fw, clm, btfw, nvram) = firmware();
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (_net_device, bt_device, mut control, runner) =
        cyw43::new_with_bluetooth(state, pwr, spi, fw, btfw, nvram).await;

    spawner.spawn(cyw43_task(runner).expect("CYW43439 task token"));

    control.init(clm).await;

    if option_env!("OPENVM_MCU_RP2350_EMBEDDED_SELF_TEST").is_some() {
        let event = match crate::embedded_artifacts::artifacts() {
            Some((key, proof, proof_sha)) => app.self_test(key, proof, proof_sha, crate::now_us()),
            None => app.self_test(&[], &[], "missing", crate::now_us()),
        };
        crate::render_event(event);
    }

    let controller: ExternalController<_, 10> = ExternalController::new(bt_device);
    run_gatt(controller, app).await
}

fn spawn_verifier_core1(core1: embassy_rp::Peri<'static, embassy_rp::peripherals::CORE1>) {
    multicore::spawn_core1(core1, unsafe { &mut *addr_of_mut!(CORE1_STACK) }, move || {
        let executor = EXECUTOR1.init(Executor::new());
        executor.run(|spawner| spawner.spawn(verifier_core1_task().expect("core1 verifier task token")));
    });
}

#[cfg(feature = "skip-cyw43-firmware")]
fn firmware() -> (
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
) {
    static EMPTY: &cyw43::Aligned<cyw43::A4, [u8]> = &cyw43::Aligned([0u8; 0]);
    (EMPTY, EMPTY, EMPTY, EMPTY)
}

#[cfg(not(feature = "skip-cyw43-firmware"))]
fn firmware() -> (
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
    &'static cyw43::Aligned<cyw43::A4, [u8]>,
) {
    let fw = aligned_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/cyw43-firmware/43439A0.bin"));
    let clm = aligned_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/cyw43-firmware/43439A0_clm.bin"));
    let btfw = aligned_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/cyw43-firmware/43439A0_btfw.bin"));
    let nvram = aligned_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/cyw43-firmware/nvram_rp2040.bin"));
    (fw, clm, btfw, nvram)
}

async fn run_gatt<C>(controller: C, mut app: ProofApp) -> !
where
    C: Controller + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    let address = Address::random(BLE_ADDRESS);
    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);
    let host = stack.build();
    let runner = host.runner;
    let mut peripheral = host.peripheral;
    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: proof_app::DEVICE_NAME,
        appearance: &appearance::power_device::GENERIC_POWER_DEVICE,
    }))
    .expect("GATT server");

    join(ble_task(runner), async {
        loop {
            match advertise(&mut peripheral, &server).await {
                Ok(conn) => {
                    let _ = configure_connection(&stack, &conn).await;
                    let connected = app.connected(crate::now_us());
                    publish_event(&server, &conn, connected, true).await;
                    let _ = gatt_events_task(&stack, &server, &conn, &mut app).await;
                }
                Err(_) => {
                    crate::render_event(Event::Status {
                        status: String::from("error advertising"),
                        detail: String::from("CYW43439 BLE peripheral"),
                    });
                    embassy_time::Timer::after_secs(1).await;
                }
            }
        }
    })
    .await;

    loop {
        crate::tufty_display::idle_heartbeat(0);
    }
}

async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) {
    let _ = runner.run().await;
}

async fn gatt_events_task<C, P>(
    stack: &Stack<'_, C, P>,
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    app: &mut ProofApp,
) -> Result<(), Error>
where
    C: Controller + ControllerCmdSync<LeReadLocalSupportedFeatures>,
    P: PacketPool,
{
    let control = &server.proof.control;
    let data = &server.proof.data;
    let mut last_receive_publish_percent = 0;
    let mut last_receive_render_percent = 0;
    let mut last_receive_render_us = 0;
    let reason = loop {
        let next_event = if app.is_receiving() {
            match select(
                conn.next(),
                embassy_time::Timer::after_micros(RECEIVE_IDLE_RENDER_INTERVAL_US as u64),
            )
            .await
            {
                Either::First(event) => Some(event),
                Either::Second(_) => {
                    let now_us = crate::now_us();
                    if let Some(event) = app.receiving_progress(now_us) {
                        maybe_render_receive_idle_progress(
                            &event,
                            now_us,
                            &mut last_receive_render_us,
                        );
                    }
                    None
                }
            }
        } else if app.is_verifying() {
            match select3(conn.next(), VERIFY_RESULTS.receive(), VERIFY_PROGRESS.receive()).await {
                Either3::First(event) => Some(event),
                Either3::Second(outcome) => {
                    let event = app.complete_commit(outcome.into_result(), crate::now_us());
                    publish_event(server, conn, event, true).await;
                    None
                }
                Either3::Third((step, label)) => {
                    let message = format!("verifying {} {}", step, label);
                    let value = status_value(&message);
                    let _ = server.proof.status.notify(conn, &value).await;
                    None
                }
            }
        } else {
            Some(conn.next().await)
        };

        let Some(connection_event) = next_event else {
            continue;
        };

        match connection_event {
            GattConnectionEvent::Disconnected { reason } => break reason,
            GattConnectionEvent::RequestConnectionParams(request) => {
                let _ = request.accept(Some(&preferred_conn_params()), stack).await;
            }
            GattConnectionEvent::Gatt { event } => {
                let event_to_publish = match &event {
                    GattEvent::Write(write) => {
                        let handle = write.handle();
                        let payload = copy_write(write.data());
                        let now_us = crate::now_us();
                        match event.accept() {
                            Ok(reply) => reply.send().await,
                            Err(_) => {}
                        }
                        if is_client_config_write(payload.as_slice()) {
                            None
                        } else if handle == control.handle {
                            let event = control_event(app, payload.as_slice(), now_us);
                            if let Some(Event::Status { status, .. }) = &event {
                                if status == "receiving" {
                                    last_receive_publish_percent = 0;
                                    last_receive_render_percent = 0;
                                    last_receive_render_us = now_us;
                                }
                            }
                            event
                        } else if handle == data.handle {
                            let is_final_chunk = is_final_transfer_chunk(payload.as_slice());
                            let event = app.data_write(payload.as_slice(), now_us);
                            let receive_percent = app.receive_percent();
                            maybe_render_receive_progress(
                                &event,
                                receive_percent,
                                &mut last_receive_render_percent,
                            );
                            should_publish_data_event(
                                &event,
                                receive_percent,
                                &mut last_receive_publish_percent,
                                is_final_chunk,
                            )
                            .then_some(event)
                        } else {
                            None
                        }
                    }
                    _ => {
                        match event.accept() {
                            Ok(reply) => reply.send().await,
                            Err(_) => {}
                        }
                        None
                    }
                };

                if let Some(event) = event_to_publish {
                    let render = render_connected_event(&event);
                    publish_event(server, conn, event, render).await;
                }
            }
            _ => {}
        }
    };

    let mut detail = String::from("BLE disconnected");
    let _ = write!(&mut detail, " {:?}", reason);
    crate::render_event(Event::Status {
        status: String::from("ready"),
        detail,
    });
    Ok(())
}

async fn configure_connection<C, P>(stack: &Stack<'_, C, P>, conn: &GattConnection<'_, '_, P>) -> Result<(), BleHostError<C::Error>>
where
    C: Controller + ControllerCmdSync<LeReadLocalSupportedFeatures>,
    P: PacketPool,
{
    conn.raw().update_connection_params(stack, &preferred_conn_params()).await
}

fn preferred_conn_params() -> RequestedConnParams {
    RequestedConnParams {
        min_connection_interval: embassy_time::Duration::from_millis(BLE_TARGET_CONN_INTERVAL_MS),
        max_connection_interval: embassy_time::Duration::from_millis(BLE_TARGET_CONN_INTERVAL_MS),
        max_latency: 0,
        min_event_length: embassy_time::Duration::from_secs(0),
        max_event_length: embassy_time::Duration::from_secs(0),
        supervision_timeout: embassy_time::Duration::from_secs(BLE_SUPERVISION_TIMEOUT_SECS),
    }
}

fn should_publish_data_event(
    event: &Event,
    receive_percent: usize,
    last_publish_percent: &mut usize,
    force_publish: bool,
) -> bool {
    match event {
        Event::Status { status, .. } => {
            if status == "receiving" {
                if force_publish
                    || should_emit_receive_progress(receive_percent, last_publish_percent)
                {
                    *last_publish_percent = receive_percent;
                    true
                } else {
                    false
                }
            } else if status == "received" {
                *last_publish_percent = 100;
                true
            } else {
                true
            }
        }
        Event::Verified { .. } => true,
    }
}

fn is_client_config_write(payload: &[u8]) -> bool {
    matches!(payload, [0, 0] | [1, 0] | [2, 0] | [3, 0])
}

fn is_final_transfer_chunk(payload: &[u8]) -> bool {
    payload.len() >= 4 && payload[3] != 0
}

fn control_event(app: &mut ProofApp, payload: &[u8], now_us: i64) -> Option<Event> {
    let command = core::str::from_utf8(payload).ok()?.trim_matches('\0').trim();
    if command.starts_with("START ") {
        Some(app.control_write(command, now_us))
    } else if command == "COMMIT" {
        match app.begin_commit(now_us) {
            Ok(artifacts) => {
                let detail = artifacts.detail.clone();
                if VERIFY_REQUESTS.try_send(artifacts.into()).is_err() {
                    app.cancel_commit();
                    Some(Event::Status {
                        status: String::from("error verifier busy"),
                        detail: String::from("core1 verifier queue"),
                    })
                } else {
                    Some(Event::Status {
                        status: String::from("verifying halo2"),
                        detail,
                    })
                }
            }
            Err(event) => Some(event),
        }
    } else {
        None
    }
}

fn render_connected_event(event: &Event) -> bool {
    match event {
        Event::Status { status, .. } => {
            status == "ready"
                || status.starts_with("verifying")
                || status == "received"
                || status.starts_with("error")
        }
        Event::Verified { .. } => true,
    }
}

fn maybe_render_receive_progress(
    event: &Event,
    receive_percent: usize,
    last_render_percent: &mut usize,
) {
    if let Event::Status { status, .. } = event {
        if status == "receiving" && should_emit_receive_progress(receive_percent, last_render_percent) {
            *last_render_percent = receive_percent;
        }
    }
}

fn maybe_render_receive_idle_progress(event: &Event, now_us: i64, last_render_us: &mut i64) {
    if let Event::Status { status, .. } = event {
        if status == "receiving"
            && (*last_render_us == 0 || now_us - *last_render_us >= RECEIVE_IDLE_RENDER_INTERVAL_US)
        {
            *last_render_us = now_us;
        }
    }
}

fn should_emit_receive_progress(receive_percent: usize, last_percent: &usize) -> bool {
    receive_percent >= 100
        || *last_percent == 0
        || receive_percent >= last_percent.saturating_add(RECEIVE_PROGRESS_STEP_PERCENT)
}

async fn advertise<'values, 'server, C: Controller>(
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut adv_data = [0; 31];
    let service_uuids = [SERVICE_UUID_LE];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::Unknown {
                ty: 0x07,
                data: &service_uuids[0],
            },
            AdStructure::CompleteLocalName(proof_app::DEVICE_NAME.as_bytes()),
        ],
        &mut adv_data,
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &adv_data[..len],
                scan_data: &[],
            },
        )
        .await?;
    Ok(advertiser.accept().await?.with_attribute_server(server)?)
}

async fn publish_event<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    event: Event,
    render: bool,
) {
    let message = event_message(&event);
    let value = status_value(&message);
    let _ = server.proof.status.notify(conn, &value).await;
    if render {
        crate::render_event(event);
    }
}

fn copy_write(data: &[u8]) -> WriteValue {
    let mut out = WriteValue::new();
    let len = data.len().min(MAX_WRITE_BYTES);
    let _ = out.extend_from_slice(&data[..len]);
    out
}

fn status_value(message: &str) -> StatusValue {
    let mut out = StatusValue::new();
    let bytes = message.as_bytes();
    let len = bytes.len().min(MAX_STATUS_BYTES);
    let _ = out.extend_from_slice(&bytes[..len]);
    out
}

fn event_message(event: &Event) -> String {
    match event {
        Event::Status { status, detail } => {
            if detail.is_empty() {
                status.clone()
            } else {
                format!("{} {}", status, detail)
            }
        }
        Event::Verified { probe, detail } => {
            let verdict = match probe.status {
                openvm_mcu_device_app::ProofStatus::Verified => "verified",
                openvm_mcu_device_app::ProofStatus::Rejected => "rejected",
                openvm_mcu_device_app::ProofStatus::CryptoBackendUnavailable => "error crypto backend",
            };
            format!("{} {} {}", verdict, probe.host.detail, detail)
        }
    }
}
