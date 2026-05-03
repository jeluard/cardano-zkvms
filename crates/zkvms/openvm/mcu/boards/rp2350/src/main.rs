#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

extern crate alloc;

#[cfg(target_arch = "arm")]
use cortex_m as _;

#[cfg(target_arch = "arm")]
use core::ptr::read_volatile;
#[cfg(target_arch = "arm")]
use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(all(target_arch = "arm", feature = "bare-metal"))]
use panic_halt as _;

#[cfg(target_arch = "arm")]
#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

#[cfg(target_arch = "arm")]
static LAST_PROOF_STATUS: AtomicU32 = AtomicU32::new(0);

#[cfg(target_arch = "arm")]
#[used]
#[link_section = ".rp2350_image_def"]
static RP2350_IMAGE_DEF: [u32; 5] = [
    0xffff_ded3,
    0x1021_0142,
    0x0000_01ff,
    0x0000_0000,
    0xab12_3579,
];

mod embedded_artifacts {
    include!(concat!(env!("OUT_DIR"), "/embedded_artifacts.rs"));
}

mod proof_app;
#[cfg(target_arch = "arm")]
mod tufty_display;
#[cfg(target_arch = "arm")]
mod ui;

#[cfg(target_arch = "arm")]
fn init_heap() {
    use core::{mem::MaybeUninit, ptr::addr_of_mut};

    const HEAP_SIZE: usize = 320 * 1024;
    static mut HEAP_MEMORY: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        HEAP.init(addr_of_mut!(HEAP_MEMORY).cast::<u8>() as usize, HEAP_SIZE);
    }
}

#[cfg(target_arch = "arm")]
#[cortex_m_rt::entry]
fn main() -> ! {
    init_heap();
    tufty_display::init();

    if option_env!("OPENVM_MCU_RP2350_VISUAL_ONLY").is_some() {
        loop {
            tufty_display::visual_test_cycle();
        }
    }

    let mut app = proof_app::ProofApp::default();
    render_event(app.ready());

    if option_env!("OPENVM_MCU_RP2350_EMBEDDED_SELF_TEST").is_some() {
        let event = match embedded_artifacts::artifacts() {
            Some((key, proof, proof_sha)) => app.self_test(key, proof, proof_sha, now_us()),
            None => app.self_test(&[], &[], "missing", now_us()),
        };
        render_event(event);
    }

    loop {
        tufty_display::idle_heartbeat(LAST_PROOF_STATUS.load(Ordering::Relaxed));
    }
}

#[cfg(target_arch = "arm")]
fn render_event(event: proof_app::Event) {
    match event {
        proof_app::Event::Status { status, detail } => {
            let _ = ui::render_status(&status, &detail, proof_app::DEVICE_NAME);
        }
        proof_app::Event::Verified { probe, detail } => {
            let status_code = proof_status_code(probe.status);
            LAST_PROOF_STATUS.store(status_code, Ordering::Relaxed);
            tufty_display::set_status_leds(status_code);
            let _ = ui::render_probe(&probe, &detail);
        }
    }
}

#[cfg(target_arch = "arm")]
fn proof_status_code(status: openvm_mcu_device_app::ProofStatus) -> u32 {
    match status {
        openvm_mcu_device_app::ProofStatus::Verified => 1,
        openvm_mcu_device_app::ProofStatus::Rejected => 2,
        openvm_mcu_device_app::ProofStatus::CryptoBackendUnavailable => 3,
    }
}

#[cfg(target_arch = "arm")]
fn now_us() -> i64 {
    const TIMER0_BASE: usize = 0x400b_0000;
    const TIMERAWH: usize = TIMER0_BASE + 0x24;
    const TIMERAWL: usize = TIMER0_BASE + 0x28;

    loop {
        let hi0 = unsafe { read_volatile(TIMERAWH as *const u32) };
        let lo = unsafe { read_volatile(TIMERAWL as *const u32) };
        let hi1 = unsafe { read_volatile(TIMERAWH as *const u32) };
        if hi0 == hi1 {
            let value = ((hi0 as u64) << 32) | lo as u64;
            return value.min(i64::MAX as u64) as i64;
        }
    }
}

#[cfg(not(target_arch = "arm"))]
#[allow(dead_code)]
fn now_us() -> i64 {
    0
}

#[cfg(not(target_arch = "arm"))]
fn main() {
    let mut transfer = openvm_mcu_device_app::transfer::TransferState::default();
    let start = transfer
        .start_from_command("START 1 1 host-smoke", 1_000)
        .expect("valid common transfer START command");
    println!(
        "rp2350 board crate: common transfer app ready proof={}",
        start.proof_sha_prefix
    );
    let probe = openvm_mcu_device_app::no_std_halo2_probe();
    let embedded = embedded_artifacts::artifacts().is_some();
    println!(
        "rp2350 board crate: verifier probe status={} backend={} detail={} embedded_artifacts={}",
        probe.status.label(),
        probe.host.status,
        probe.host.detail,
        embedded
    );
    println!("rp2350 board crate: build with an RP2350 target to run the Cortex-M entry point");
}
