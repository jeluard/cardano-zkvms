#[cfg(target_arch = "arm")]
use core::ptr::{read_volatile, write_volatile};

#[cfg(target_arch = "arm")]
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Dimensions, Point, Size},
    pixelcolor::{IntoStorage, Rgb565},
    primitives::Rectangle,
    Pixel,
};

#[cfg(target_arch = "arm")]
const IO_BANK0_BASE: usize = 0x4002_8000;
#[cfg(target_arch = "arm")]
const PADS_BANK0_BASE: usize = 0x4003_8000;
#[cfg(target_arch = "arm")]
const SIO_BASE: usize = 0xd000_0000;

#[cfg(target_arch = "arm")]
const GPIO_OUT_SET: usize = SIO_BASE + 0x18;
#[cfg(target_arch = "arm")]
const GPIO_OUT_CLR: usize = SIO_BASE + 0x20;
#[cfg(target_arch = "arm")]
const GPIO_OE_SET: usize = SIO_BASE + 0x38;
#[cfg(target_arch = "arm")]
const GPIO_HI_OUT_SET: usize = SIO_BASE + 0x1c;
#[cfg(target_arch = "arm")]
const GPIO_HI_OUT_CLR: usize = SIO_BASE + 0x24;
#[cfg(target_arch = "arm")]
const GPIO_HI_OE_SET: usize = SIO_BASE + 0x3c;

#[cfg(target_arch = "arm")]
const PADS_BANK0_GPIO_OFFSET: usize = 0x04;
#[cfg(target_arch = "arm")]
const PADS_BANK0_GPIO_STRIDE: usize = 0x04;
#[cfg(target_arch = "arm")]
const PAD_ISO: u32 = 1 << 8;
#[cfg(target_arch = "arm")]
const PAD_OD: u32 = 1 << 7;
#[cfg(target_arch = "arm")]
const PAD_IE: u32 = 1 << 6;

#[cfg(target_arch = "arm")]
const PIN_BL: u8 = 26;
#[cfg(target_arch = "arm")]
const PIN_SW_POWER_EN: u8 = 41;
#[cfg(target_arch = "arm")]
const PIN_CS: u8 = 27;
#[cfg(target_arch = "arm")]
const PIN_DC: u8 = 28;
#[cfg(target_arch = "arm")]
const PIN_WR: u8 = 30;
#[cfg(target_arch = "arm")]
const PIN_RD: u8 = 31;
#[cfg(target_arch = "arm")]
const PIN_D0: u8 = 32;

#[cfg(target_arch = "arm")]
const LED_PINS: [u8; 4] = [0, 1, 2, 3];

#[cfg(target_arch = "arm")]
const WIDTH: u16 = 240;
#[cfg(target_arch = "arm")]
const HEIGHT: u16 = 320;

#[cfg(target_arch = "arm")]
const LOGICAL_WIDTH: u16 = HEIGHT;
#[cfg(target_arch = "arm")]
const LOGICAL_HEIGHT: u16 = WIDTH;

#[cfg(target_arch = "arm")]
const BLUE: u16 = 0x0317;
#[cfg(target_arch = "arm")]
const GREEN: u16 = 0x0565;
#[cfg(target_arch = "arm")]
const AMBER: u16 = 0xfd20;
#[cfg(target_arch = "arm")]
const RED: u16 = 0xe104;

#[cfg(target_arch = "arm")]
pub fn set_status_leds(status_code: u32) {
    let pattern = match status_code {
        1 => 0b0011,
        2 => 0b0101,
        3 => 0b1001,
        _ => 0b0001,
    };
    set_leds(pattern);
}

#[cfg(target_arch = "arm")]
pub struct TuftyDisplay;

#[cfg(target_arch = "arm")]
impl TuftyDisplay {
    pub const fn new() -> Self {
        Self
    }
}

#[cfg(target_arch = "arm")]
impl Dimensions for TuftyDisplay {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(
            Point::zero(),
            Size::new(LOGICAL_WIDTH as u32, LOGICAL_HEIGHT as u32),
        )
    }
}

#[cfg(target_arch = "arm")]
impl DrawTarget for TuftyDisplay {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let mut run: Option<(u16, u16, u16, Rgb565)> = None;
        for Pixel(point, color) in pixels {
            let Some((x, y)) = logical_point(point) else {
                continue;
            };
            match run {
                Some((start_x, run_y, len, run_color))
                    if y == run_y && x == start_x + len && color == run_color =>
                {
                    run = Some((start_x, run_y, len + 1, run_color));
                }
                Some((start_x, run_y, len, run_color)) => {
                    fill_logical_horizontal_run(start_x, run_y, len, rgb565(run_color));
                    run = Some((x, y, 1, color));
                }
                None => run = Some((x, y, 1, color)),
            }
        }
        if let Some((start_x, run_y, len, run_color)) = run {
            fill_logical_horizontal_run(start_x, run_y, len, rgb565(run_color));
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        if let Some((x, y, w, h)) = logical_rect(area) {
            fill_logical_rect(x, y, w, h, rgb565(color));
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let Some((x, y, w, h)) = logical_rect(area) else {
            return Ok(());
        };
        let mut colors = colors.into_iter();
        for row in 0..h {
            let mut run_start = x;
            let mut run_len = 0_u16;
            let mut run_color: Option<Rgb565> = None;
            for col in 0..w {
                let Some(color) = colors.next() else {
                    return Ok(());
                };
                match run_color {
                    Some(current) if current == color => run_len += 1,
                    Some(current) => {
                        fill_logical_horizontal_run(run_start, y + row, run_len, rgb565(current));
                        run_start = x + col;
                        run_len = 1;
                        run_color = Some(color);
                    }
                    None => {
                        run_start = x + col;
                        run_len = 1;
                        run_color = Some(color);
                    }
                }
            }
            if let Some(color) = run_color {
                fill_logical_horizontal_run(run_start, y + row, run_len, rgb565(color));
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        fill_screen(rgb565(color));
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
pub fn init_debug_leds() {
    init_board_power();
    for pin in LED_PINS {
        configure_sio_output(pin);
        set_low(pin);
    }
    set_leds(0b0001);
}

#[cfg(target_arch = "arm")]
pub fn boot_chase(rounds: u8) {
    for _ in 0..rounds {
        for pattern in [0b0001, 0b0010, 0b0100, 0b1000] {
            set_leds(pattern);
            delay_ms(80);
        }
    }
}

#[cfg(target_arch = "arm")]
#[allow(dead_code)]
pub fn led_smoke_test_forever() -> ! {
    init_debug_leds();
    loop {
        for pattern in [0b0001, 0b0010, 0b0100, 0b1000, 0b1111, 0b0000] {
            set_leds(pattern);
            delay_ms(180);
        }
    }
}

#[cfg(target_arch = "arm")]
#[allow(dead_code)]
pub fn visual_smoke_test_forever() -> ! {
    init_debug_leds();
    boot_chase(4);
    init();
    loop {
        visual_test_cycle();
    }
}

#[cfg(target_arch = "arm")]
pub fn init() {
    init_board_power();
    init_debug_leds();

    for pin in [PIN_BL, PIN_CS, PIN_DC, PIN_WR, PIN_RD] {
        configure_sio_output(pin);
    }
    for pin in PIN_D0..PIN_D0 + 8 {
        configure_sio_output(pin);
    }

    set_high(PIN_CS);
    set_high(PIN_DC);
    set_high(PIN_WR);
    set_high(PIN_RD);
    set_high(PIN_BL);

    command(0x01, &[]);
    delay_ms(150);
    command(0x3a, &[0x05]);
    command(0xb2, &[0x0c, 0x0c, 0x00, 0x33, 0x33]);
    command(0xc0, &[0x2c]);
    command(0xc2, &[0x01]);
    command(0xc3, &[0x0f]);
    command(0xc4, &[0x20]);
    command(0xd0, &[0xa4, 0xa1]);
    command(0xc6, &[0x0f]);
    command(0xb0, &[0x00, 0xc0]);
    command(0xb7, &[0x35]);
    command(0xbb, &[0x1b]);
    command(
        0xe0,
        &[
            0xf0, 0x00, 0x06, 0x04, 0x05, 0x05, 0x31, 0x44, 0x48, 0x36, 0x12, 0x12, 0x2b, 0x34,
        ],
    );
    command(
        0xe1,
        &[
            0xf0, 0x0b, 0x0f, 0x0f, 0x0d, 0x26, 0x31, 0x43, 0x47, 0x38, 0x14, 0x14, 0x2c, 0x32,
        ],
    );
    command(0x21, &[]);
    command(0x11, &[]);
    delay_ms(100);
    command(0x36, &[0x10]);
    command(0x2a, &[0x00, 0x00, 0x00, 0xef]);
    command(0x2b, &[0x00, 0x00, 0x01, 0x3f]);
    command(0x35, &[0x00]);
    command(0x44, &[0x00, 0x00]);
    command(0x29, &[]);
    delay_ms(20);
}

#[cfg(target_arch = "arm")]
fn init_board_power() {
    configure_sio_output(PIN_SW_POWER_EN);
    set_high(PIN_SW_POWER_EN);
    delay_ms(20);
}

#[cfg(target_arch = "arm")]
pub fn visual_test_cycle() {
    for (pattern, color) in [
        (0b0001, BLUE),
        (0b0010, GREEN),
        (0b0100, AMBER),
        (0b1000, RED),
    ] {
        set_leds(pattern);
        fill_screen(color);
        delay_ms(450);
    }
}

#[cfg(target_arch = "arm")]
pub fn idle_heartbeat(status_code: u32) {
    let base = match status_code {
        1 => 0b0011,
        2 => 0b0101,
        3 => 0b1001,
        _ => 0b0001,
    };
    set_leds(base);
    delay_ms(350);
    set_leds(base ^ 0b1111);
    delay_ms(350);
}

#[cfg(target_arch = "arm")]
fn set_leds(pattern: u8) {
    for (index, pin) in LED_PINS.iter().enumerate() {
        if pattern & (1 << index) == 0 {
            set_low(*pin);
        } else {
            set_high(*pin);
        }
    }
}

fn fill_screen(color: u16) {
    fill_rect(0, 0, WIDTH, HEIGHT, color);
}

#[cfg(target_arch = "arm")]
fn fill_rect_clipped(x: u16, y: u16, w: u16, h: u16, color: u16) {
    if x >= WIDTH || y >= HEIGHT {
        return;
    }
    let w = w.min(WIDTH - x);
    let h = h.min(HEIGHT - y);
    fill_rect(x, y, w, h, color);
}

#[cfg(target_arch = "arm")]
fn fill_rect(x: u16, y: u16, w: u16, h: u16, color: u16) {
    if w == 0 || h == 0 {
        return;
    }
    set_window(x, y, x + w - 1, y + h - 1);
    set_low(PIN_CS);
    set_high(PIN_DC);
    for _ in 0..(w as u32 * h as u32) {
        write8((color >> 8) as u8);
        write8(color as u8);
    }
    set_high(PIN_CS);
}

#[cfg(target_arch = "arm")]
fn logical_point(point: Point) -> Option<(u16, u16)> {
    if point.x < 0 || point.y < 0 {
        return None;
    }
    let x = point.x as u16;
    let y = point.y as u16;
    if x >= LOGICAL_WIDTH || y >= LOGICAL_HEIGHT {
        return None;
    }
    Some((x, y))
}

#[cfg(target_arch = "arm")]
fn logical_rect(area: &Rectangle) -> Option<(u16, u16, u16, u16)> {
    let bounds = Rectangle::new(
        Point::zero(),
        Size::new(LOGICAL_WIDTH as u32, LOGICAL_HEIGHT as u32),
    );
    let clipped = area.intersection(&bounds);
    if clipped.size.width == 0 || clipped.size.height == 0 {
        return None;
    }
    Some((
        clipped.top_left.x as u16,
        clipped.top_left.y as u16,
        clipped.size.width as u16,
        clipped.size.height as u16,
    ))
}

#[cfg(target_arch = "arm")]
fn fill_logical_rect(x: u16, y: u16, w: u16, h: u16, color: u16) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect_clipped(y, LOGICAL_WIDTH - x - w, h, w, color);
}

#[cfg(target_arch = "arm")]
fn fill_logical_horizontal_run(x: u16, y: u16, len: u16, color: u16) {
    if len == 0 || y >= LOGICAL_HEIGHT || x >= LOGICAL_WIDTH {
        return;
    }
    let len = len.min(LOGICAL_WIDTH - x);
    fill_rect_clipped(y, LOGICAL_WIDTH - x - len, 1, len, color);
}

#[cfg(target_arch = "arm")]
fn rgb565(color: Rgb565) -> u16 {
    color.into_storage()
}

#[cfg(target_arch = "arm")]
fn set_window(x0: u16, y0: u16, x1: u16, y1: u16) {
    command(
        0x2a,
        &[(x0 >> 8) as u8, x0 as u8, (x1 >> 8) as u8, x1 as u8],
    );
    command(
        0x2b,
        &[(y0 >> 8) as u8, y0 as u8, (y1 >> 8) as u8, y1 as u8],
    );
    command(0x2c, &[]);
}

#[cfg(target_arch = "arm")]
fn command(register: u8, data: &[u8]) {
    set_low(PIN_DC);
    set_low(PIN_CS);
    write8(register);
    if !data.is_empty() {
        set_high(PIN_DC);
        for byte in data {
            write8(*byte);
        }
    }
    set_high(PIN_CS);
}

#[cfg(target_arch = "arm")]
fn write8(value: u8) {
    unsafe {
        write_volatile(GPIO_HI_OUT_CLR as *mut u32, 0xff);
        write_volatile(GPIO_HI_OUT_SET as *mut u32, value as u32);
    }
    set_low(PIN_WR);
    cortex_m::asm::nop();
    set_high(PIN_WR);
}

#[cfg(target_arch = "arm")]
fn configure_sio_output(pin: u8) {
    unsafe {
        let pad = (PADS_BANK0_BASE + PADS_BANK0_GPIO_OFFSET + pin as usize * PADS_BANK0_GPIO_STRIDE)
            as *mut u32;
        let mut pad_value = read_volatile(pad);
        pad_value = (pad_value | PAD_IE) & !(PAD_OD | PAD_ISO);
        write_volatile(pad, pad_value);

        let ctrl = (IO_BANK0_BASE + pin as usize * 8 + 4) as *mut u32;
        let mut value = read_volatile(ctrl);
        value = (value & !0x1f) | 0x05;
        write_volatile(ctrl, value);
    }
    set_output_enabled(pin);
}

#[cfg(target_arch = "arm")]
fn set_output_enabled(pin: u8) {
    let bank = pin / 32;
    let bit = 1u32 << (pin % 32);
    unsafe {
        let set = if bank == 0 {
            GPIO_OE_SET
        } else {
            GPIO_HI_OE_SET
        };
        write_volatile(set as *mut u32, bit);
    }
}

#[cfg(target_arch = "arm")]
fn set_high(pin: u8) {
    let bank = pin / 32;
    let bit = 1u32 << (pin % 32);
    unsafe {
        let set = if bank == 0 {
            GPIO_OUT_SET
        } else {
            GPIO_HI_OUT_SET
        };
        write_volatile(set as *mut u32, bit);
    }
}

#[cfg(target_arch = "arm")]
fn set_low(pin: u8) {
    let bank = pin / 32;
    let bit = 1u32 << (pin % 32);
    unsafe {
        let clear = if bank == 0 {
            GPIO_OUT_CLR
        } else {
            GPIO_HI_OUT_CLR
        };
        write_volatile(clear as *mut u32, bit);
    }
}

#[cfg(target_arch = "arm")]
fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..20_000 {
            cortex_m::asm::nop();
        }
    }
}
