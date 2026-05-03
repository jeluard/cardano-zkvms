# ESP32-S3 MCU Target

This crate targets the Waveshare ESP32-S3-Touch-LCD-2 through the Xtensa ESP Rust toolchain.

The ESP-IDF/std firmware prints an honest proof status over USB/JTAG serial and renders proof details on the onboard 2-inch 240x320 ST7789 SPI display. With `halo2-std`, it runs the native OpenVM Halo2/KZG verifier on the ESP32-S3 itself.

The Waveshare schematic wiring is used by default:

- LCD ST7789 SPI: `SCLK=GPIO39`, `MOSI=GPIO38`, `CS=GPIO45`, `DC=GPIO42`, `RST=GPIO0`, `BL=GPIO1`
- Touch CST816 I2C: `SCL=GPIO47`, `SDA=GPIO48`, `INT=GPIO46`

If your board revision differs, override the LCD wiring with environment variables:

```bash
export ESP32S3_LCD_SCLK=39
export ESP32S3_LCD_MOSI=38
export ESP32S3_LCD_CS=45
export ESP32S3_LCD_DC=42
export ESP32S3_LCD_RST=0
export ESP32S3_LCD_BL=1
export ESP32S3_LCD_WIDTH=240
export ESP32S3_LCD_HEIGHT=320
```

Then build with:

```bash
source ~/export-esp.sh
cargo +esp build -p openvm-mcu-esp32s3 -Zbuild-std=core,alloc --target xtensa-esp32s3-none-elf
```

From the repository root, `make mcu-esp32s3-flash-monitor` builds the no-std display/status harness, flashes, resets into the app, and streams USB/JTAG serial output.

To generate a real OpenVM Halo2/KZG proof, embed it, verify it on the ESP32-S3, and draw proof details on the LCD:

```bash
make mcu-esp32s3-flash-halo2-std
```

Expected USB/JTAG serial markers:

```text
openvm-mcu-esp32s3-espidf: proof_status=verified
openvm-mcu-esp32s3-espidf: display updated
```

The display keeps host proof metadata separate from the MCU verifier result and does not draw a green proof state unless the ESP32-S3 firmware verifier itself accepts the proof.

## BLE Proof Transfer

The ESP-IDF/std firmware is built with `display-st7789,ble-transfer` by the repository Makefile. It exposes a BLE GATT service named `OpenVM MCU`:

```text
service  7b7c0001-78f1-4f9a-8b29-6f1f1d95a100
control  7b7c0002-78f1-4f9a-8b29-6f1f1d95a100
data     7b7c0003-78f1-4f9a-8b29-6f1f1d95a100
status   7b7c0004-78f1-4f9a-8b29-6f1f1d95a100
```

The web UI calls `/api/prove/mcu-halo2`, sends `START <key_len> <proof_len> <proof_sha>` to the control characteristic, streams the postcard-encoded verifier key and proof envelope to the data characteristic, then sends `COMMIT`. The MCU returns `verified`, `rejected`, or `error ...` on the status characteristic and redraws the LCD with Ratatui widgets rendered through mousefood's embedded-graphics backend for BLE receive progress, verifier state, and final proof metadata.

Native Halo2/KZG verification needs a large stack on this board. The BLE firmware starts the verifier as an ESP-IDF FreeRTOS task with `xTaskCreatePinnedToCoreWithCaps(..., MALLOC_CAP_SPIRAM)` so the stack lives in PSRAM; do not replace it with a plain Rust `std::thread` unless stack allocation is still explicitly PSRAM-backed.

Web Bluetooth requires Chrome or Edge on `localhost` or HTTPS.

If `espflash` cannot reconnect after a bad image, put the board in ROM download mode manually using the Waveshare recovery sequence: hold `BOOT`, plug in USB or tap `RESET`, then release `BOOT` after the USB port appears. If `espflash monitor --before no-reset-no-sync` reports `Secure Download Mode is enabled on this chip`, the board has not entered the normal writable download mode over the USB/JTAG serial path; reset normally or repeat the recovery sequence before flashing again.