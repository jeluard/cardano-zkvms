#![cfg_attr(all(target_arch = "xtensa", not(target_os = "espidf")), no_std)]
#![cfg_attr(all(target_arch = "xtensa", not(target_os = "espidf")), no_main)]

extern crate alloc;

mod status;

#[cfg(all(target_os = "espidf", feature = "ble-transfer"))]
mod ble;

#[cfg(all(target_arch = "xtensa", feature = "display-st7789"))]
mod screen;

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use core::{mem::MaybeUninit, ptr::addr_of_mut};

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use embassy_time::{Duration, Timer};
#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use esp_println::println;

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use esp_alloc as _;
#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
use esp_backtrace as _;

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
#[repr(C)]
struct EspAppDesc {
    magic_word: u32,
    secure_version: u32,
    reserved1: [u32; 2],
    version: [u8; 32],
    project_name: [u8; 32],
    build_time: [u8; 16],
    build_date: [u8; 16],
    idf_version: [u8; 32],
    app_elf_sha256: [u8; 32],
    min_efuse_blk_rev_full: u16,
    max_efuse_blk_rev_full: u16,
    mmu_page_size_log2: u8,
    reserved3: [u8; 3],
    reserved2: [u32; 18],
}

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
#[used]
#[unsafe(export_name = "esp_app_desc")]
#[unsafe(link_section = ".flash.appdesc")]
static ESP_APP_DESC: EspAppDesc = EspAppDesc {
    magic_word: 0xABCD_5432,
    secure_version: 0,
    reserved1: [0; 2],
    version: cstr(env!("CARGO_PKG_VERSION")),
    project_name: cstr(env!("CARGO_PKG_NAME")),
    build_time: cstr("00:00:00"),
    build_date: cstr("1970-01-01"),
    idf_version: cstr("esp-idf-compatible"),
    app_elf_sha256: [0; 32],
    min_efuse_blk_rev_full: 0,
    max_efuse_blk_rev_full: u16::MAX,
    mmu_page_size_log2: 16,
    reserved3: [0; 3],
    reserved2: [0; 18],
};

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
const fn cstr<const SIZE: usize>(value: &str) -> [u8; SIZE] {
    let bytes = value.as_bytes();
    let mut output = [0; SIZE];
    let mut index = 0;

    while index < bytes.len() && index + 1 < SIZE {
        output[index] = bytes[index];
        index += 1;
    }

    output
}

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
#[esp_hal_embassy::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    init_heap();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timers = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timers.timer0);

    println!("openvm-mcu-esp32s3: boot");
    let probe = status::run_probe();
    println!("openvm-mcu-esp32s3: proof_kind={}", probe.proof_kind);
    println!("openvm-mcu-esp32s3: host_evm_status={}", probe.host.status);
    println!("openvm-mcu-esp32s3: host_evm_detail={}", probe.host.detail);
    println!(
        "openvm-mcu-esp32s3: host_evm_proof_sha={}",
        probe.host.proof_sha
    );
    println!("openvm-mcu-esp32s3: proof_status={}", probe.status.label());
    println!("openvm-mcu-esp32s3: detail={}", probe.status.detail());

    #[cfg(feature = "display-st7789")]
    match screen::render_probe(peripherals.SPI2, probe.clone()) {
        Ok(()) => println!("openvm-mcu-esp32s3: display updated"),
        Err(()) => println!("openvm-mcu-esp32s3: display update failed"),
    }

    #[cfg(not(feature = "display-st7789"))]
    println!("openvm-mcu-esp32s3: display disabled; enable feature display-st7789 with ESP32S3_LCD_* pins");

    loop {
        Timer::after(Duration::from_secs(5)).await;
        println!("openvm-mcu-esp32s3: proof_status={}", probe.status.label());
    }
}

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
fn init_heap() {
    const HEAP_SIZE: usize = 64 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            addr_of_mut!(HEAP).cast::<u8>(),
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}

#[cfg(target_os = "espidf")]
fn main() {
    use std::{thread, time::Duration};

    esp_idf_sys::link_patches();
    println!("openvm-mcu-esp32s3-espidf: boot");

    #[cfg(feature = "ble-transfer")]
    {
        match ble::start() {
            Ok(()) => println!("openvm-mcu-esp32s3-espidf: ble ready"),
            Err(()) => {
                println!("openvm-mcu-esp32s3-espidf: ble failed");
                #[cfg(feature = "display-st7789")]
                {
                    let _ = screen::render_ble_status("error ble failed", "advertising failed");
                }
            }
        }

        loop {
            thread::sleep(Duration::from_secs(5));
            println!("openvm-mcu-esp32s3-espidf: ble_waiting");
        }
    }

    #[cfg(not(feature = "ble-transfer"))]
    {
        espidf_allow_long_crypto();
        println!("openvm-mcu-esp32s3-espidf: verifier_start");
        let probe = status::run_probe();
        println!("openvm-mcu-esp32s3-espidf: proof_kind={}", probe.proof_kind);
        println!(
            "openvm-mcu-esp32s3-espidf: host_evm_status={}",
            probe.host.status
        );
        println!(
            "openvm-mcu-esp32s3-espidf: host_evm_detail={}",
            probe.host.detail
        );
        println!(
            "openvm-mcu-esp32s3-espidf: host_evm_proof_sha={}",
            probe.host.proof_sha
        );
        println!(
            "openvm-mcu-esp32s3-espidf: proof_status={}",
            probe.status.label()
        );
        println!(
            "openvm-mcu-esp32s3-espidf: detail={}",
            probe.status.detail()
        );

        #[cfg(feature = "display-st7789")]
        match screen::render_probe(probe.clone()) {
            Ok(()) => println!("openvm-mcu-esp32s3-espidf: display updated"),
            Err(()) => println!("openvm-mcu-esp32s3-espidf: display update failed"),
        }

        #[cfg(not(feature = "display-st7789"))]
        println!("openvm-mcu-esp32s3-espidf: display disabled; enable feature display-st7789");

        loop {
            thread::sleep(Duration::from_secs(5));
            println!(
                "openvm-mcu-esp32s3-espidf: proof_status={}",
                probe.status.label()
            );
        }
    }
}

#[cfg(target_os = "espidf")]
fn espidf_allow_long_crypto() {
    unsafe {
        let _ = esp_idf_sys::esp_task_wdt_deinit();
        let config = esp_idf_sys::esp_task_wdt_config_t {
            timeout_ms: 600_000,
            idle_core_mask: 0,
            trigger_panic: false,
        };
        let _ = esp_idf_sys::esp_task_wdt_reconfigure(&config);
    }
}

#[cfg(all(not(target_arch = "xtensa"), not(target_os = "espidf")))]
fn main() {
    let probe = status::run_probe();
    println!(
        "esp32s3 board crate: build with target xtensa-esp32s3-none-elf; proof_status={}",
        probe.status.label()
    );
}
