#![no_std]
#![no_main]

extern crate alloc;

use core::ptr::read_volatile;
use core::sync::atomic::{AtomicU32, Ordering};

use cortex_m as _;
use {defmt_rtt as _, embassy_time as _, panic_probe as _};

#[global_allocator]
static HEAP: embedded_alloc::LlffHeap = embedded_alloc::LlffHeap::empty();

static LAST_PROOF_STATUS: AtomicU32 = AtomicU32::new(0);

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

#[path = "../../src/ble.rs"]
mod ble;
#[path = "../../src/proof_app.rs"]
mod proof_app;
#[path = "../../src/tufty_display.rs"]
mod tufty_display;
#[path = "../../src/ui.rs"]
mod ui;

fn init_heap() {
    use core::{mem::MaybeUninit, ptr::addr_of_mut};

    const HEAP_SIZE: usize = 320 * 1024;
    static mut HEAP_MEMORY: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        HEAP.init(addr_of_mut!(HEAP_MEMORY).cast::<u8>() as usize, HEAP_SIZE);
    }
}

#[embassy_executor::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    init_heap();
    ble::main(spawner).await
}

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

fn proof_status_code(status: openvm_mcu_device_app::ProofStatus) -> u32 {
    match status {
        openvm_mcu_device_app::ProofStatus::Verified => 1,
        openvm_mcu_device_app::ProofStatus::Rejected => 2,
        openvm_mcu_device_app::ProofStatus::CryptoBackendUnavailable => 3,
    }
}

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
