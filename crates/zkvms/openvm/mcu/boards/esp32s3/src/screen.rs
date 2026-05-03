use embedded_graphics::{pixelcolor::Rgb565, prelude::*};
use mipidsi::{interface::SpiInterface, models::ST7789, options::ColorOrder, Builder};

use crate::status::ProofProbe;

#[path = "../../common/ratatui_ui.rs"]
mod ratatui_ui;

const BOARD_LABEL: &str = "Waveshare ESP32-S3";
const PROOF_LABEL: &str = "native Halo2/KZG";

#[allow(dead_code)]
mod lcd_pins {
    include!(concat!(env!("OUT_DIR"), "/lcd_pins.rs"));
}

static mut DISPLAY_BUFFER: [u8; 1024] = [0; 1024];

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
pub fn render_probe(spi2: esp_hal::peripherals::SPI2, probe: ProofProbe) -> Result<(), ()> {
    use embedded_hal_bus::spi::ExclusiveDevice;
    use esp_hal::{
        delay::Delay,
        gpio::{Level, Output},
        spi::{master::Config, master::Spi, Mode},
        time::RateExtU32,
    };

    let mut delay = Delay::new();
    let _backlight = Output::new(pin(lcd_pins::BL), Level::High);
    let dc = Output::new(pin(lcd_pins::DC), Level::Low);
    let rst = Output::new(pin(lcd_pins::RST), Level::High);
    let cs = Output::new(pin(lcd_pins::CS), Level::High);

    let spi = Spi::new(
        spi2,
        Config::default()
            .with_frequency(40_u32.MHz())
            .with_mode(Mode::_0),
    )
    .map_err(|_| ())?
    .with_sck(pin(lcd_pins::SCLK))
    .with_mosi(pin(lcd_pins::MOSI));

    let spi_device = ExclusiveDevice::new(spi, cs, Delay::new()).map_err(|_| ())?;
    let display_interface = SpiInterface::new(spi_device, dc, display_buffer());
    let mut display = Builder::new(ST7789, display_interface)
        .display_size(lcd_pins::WIDTH, lcd_pins::HEIGHT)
        .color_order(ColorOrder::Bgr)
        .reset_pin(rst)
        .init(&mut delay)
        .map_err(|_| ())?;

    draw_probe(&mut display, probe)
}

#[cfg(target_os = "espidf")]
pub fn render_probe(probe: ProofProbe) -> Result<(), ()> {
    let mut delay = espidf::EspDelay;
    let _backlight = espidf::OutputPin::new(lcd_pins::BL, true).map_err(|_| ())?;
    let dc = espidf::OutputPin::new(lcd_pins::DC, false).map_err(|_| ())?;
    let rst = espidf::OutputPin::new(lcd_pins::RST, true).map_err(|_| ())?;
    let spi = espidf::SpiDevice::new().map_err(|_| ())?;

    let display_interface = SpiInterface::new(spi, dc, display_buffer());
    let mut display = Builder::new(ST7789, display_interface)
        .display_size(lcd_pins::WIDTH, lcd_pins::HEIGHT)
        .color_order(ColorOrder::Bgr)
        .reset_pin(rst)
        .init(&mut delay)
        .map_err(|_| ())?;

    draw_probe(&mut display, probe)
}

#[cfg(target_os = "espidf")]
pub fn render_ble_status(status: &str, detail: &str) -> Result<(), ()> {
    let mut delay = espidf::EspDelay;
    let _backlight = espidf::OutputPin::new(lcd_pins::BL, true).map_err(|_| ())?;
    let dc = espidf::OutputPin::new(lcd_pins::DC, false).map_err(|_| ())?;
    let rst = espidf::OutputPin::new(lcd_pins::RST, true).map_err(|_| ())?;
    let spi = espidf::SpiDevice::new().map_err(|_| ())?;

    let display_interface = SpiInterface::new(spi, dc, display_buffer());
    let mut display = Builder::new(ST7789, display_interface)
        .display_size(lcd_pins::WIDTH, lcd_pins::HEIGHT)
        .color_order(ColorOrder::Bgr)
        .reset_pin(rst)
        .init(&mut delay)
        .map_err(|_| ())?;

    draw_ble_status(&mut display, status, detail)
}

fn draw_probe<D>(display: &mut D, probe: ProofProbe) -> Result<(), ()>
where
    D: DrawTarget<Color = Rgb565> + Dimensions + 'static,
{
    ratatui_ui::render_with_ratatui(display, |frame| {
        ratatui_ui::draw_probe_frame(frame, &probe, BOARD_LABEL, "")
    })
}

fn draw_ble_status<D>(display: &mut D, status: &str, detail: &str) -> Result<(), ()>
where
    D: DrawTarget<Color = Rgb565> + Dimensions + 'static,
{
    ratatui_ui::render_with_ratatui(display, |frame| {
        ratatui_ui::draw_ble_status_frame(frame, status, detail, BOARD_LABEL, PROOF_LABEL)
    })
}

fn display_buffer() -> &'static mut [u8; 1024] {
    unsafe { &mut *core::ptr::addr_of_mut!(DISPLAY_BUFFER) }
}

#[cfg(all(target_arch = "xtensa", not(target_os = "espidf")))]
fn pin(number: u8) -> esp_hal::gpio::AnyPin {
    unsafe { esp_hal::gpio::AnyPin::steal(number) }
}

#[cfg(target_os = "espidf")]
mod espidf {
    use core::{ffi::c_void, ptr};

    use embedded_hal::{
        delay::DelayNs,
        digital,
        spi::{self, Operation},
    };
    use esp_idf_sys as sys;

    use super::lcd_pins;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Error(sys::esp_err_t);

    impl digital::Error for Error {
        fn kind(&self) -> digital::ErrorKind {
            digital::ErrorKind::Other
        }
    }

    impl spi::Error for Error {
        fn kind(&self) -> spi::ErrorKind {
            spi::ErrorKind::Other
        }
    }

    pub struct EspDelay;

    impl DelayNs for EspDelay {
        fn delay_ns(&mut self, ns: u32) {
            let us = ns.saturating_add(999) / 1_000;
            unsafe { sys::ets_delay_us(us.max(1)) };
        }
    }

    pub struct OutputPin {
        pin: sys::gpio_num_t,
    }

    impl OutputPin {
        pub fn new(pin: u8, high: bool) -> Result<Self, Error> {
            let pin = pin as sys::gpio_num_t;
            check(unsafe { sys::gpio_reset_pin(pin) })?;
            check(unsafe { sys::gpio_set_direction(pin, sys::gpio_mode_t_GPIO_MODE_OUTPUT) })?;
            check(unsafe { sys::gpio_set_level(pin, u32::from(high)) })?;
            Ok(Self { pin })
        }
    }

    impl digital::ErrorType for OutputPin {
        type Error = Error;
    }

    impl digital::OutputPin for OutputPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            check(unsafe { sys::gpio_set_level(self.pin, 0) })
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            check(unsafe { sys::gpio_set_level(self.pin, 1) })
        }
    }

    pub struct SpiDevice {
        handle: sys::spi_device_handle_t,
    }

    impl SpiDevice {
        pub fn new() -> Result<Self, Error> {
            let host = sys::spi_host_device_t_SPI2_HOST;
            let mut bus = sys::spi_bus_config_t::default();
            bus.__bindgen_anon_1.mosi_io_num = lcd_pins::MOSI as i32;
            bus.__bindgen_anon_2.miso_io_num = sys::gpio_num_t_GPIO_NUM_NC;
            bus.sclk_io_num = lcd_pins::SCLK as i32;
            bus.__bindgen_anon_3.quadwp_io_num = sys::gpio_num_t_GPIO_NUM_NC;
            bus.__bindgen_anon_4.quadhd_io_num = sys::gpio_num_t_GPIO_NUM_NC;
            bus.max_transfer_sz = 4096;
            bus.flags = sys::SPICOMMON_BUSFLAG_MASTER
                | sys::SPICOMMON_BUSFLAG_SCLK
                | sys::SPICOMMON_BUSFLAG_MOSI;

            check(unsafe {
                sys::spi_bus_initialize(host, &bus, sys::spi_common_dma_t_SPI_DMA_CH_AUTO)
            })?;

            let mut device = sys::spi_device_interface_config_t::default();
            device.mode = 0;
            device.clock_speed_hz = sys::SPI_MASTER_FREQ_40M as i32;
            device.spics_io_num = lcd_pins::CS as i32;
            device.queue_size = 1;

            let mut handle = ptr::null_mut();
            check(unsafe { sys::spi_bus_add_device(host, &device, &mut handle) })?;
            Ok(Self { handle })
        }

        fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
            if bytes.is_empty() {
                return Ok(());
            }
            let mut transaction = sys::spi_transaction_t::default();
            transaction.length = bytes.len() * 8;
            transaction.__bindgen_anon_1.tx_buffer = bytes.as_ptr().cast::<c_void>();
            check(unsafe { sys::spi_device_transmit(self.handle, &mut transaction) })
        }
    }

    impl spi::ErrorType for SpiDevice {
        type Error = Error;
    }

    impl spi::SpiDevice<u8> for SpiDevice {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Write(bytes) => self.write_bytes(bytes)?,
                    Operation::DelayNs(ns) => EspDelay.delay_ns(*ns),
                    Operation::Read(_)
                    | Operation::Transfer(_, _)
                    | Operation::TransferInPlace(_) => {
                        return Err(Error(sys::ESP_FAIL));
                    }
                }
            }
            Ok(())
        }
    }

    impl Drop for SpiDevice {
        fn drop(&mut self) {
            unsafe {
                let _ = sys::spi_bus_remove_device(self.handle);
                let _ = sys::spi_bus_free(sys::spi_host_device_t_SPI2_HOST);
            }
        }
    }

    fn check(err: sys::esp_err_t) -> Result<(), Error> {
        if err == sys::ESP_OK {
            Ok(())
        } else {
            Err(Error(err))
        }
    }
}
