# Tufty 2350 MCU Verifier

This firmware is a no_std RP2350 build for Pimoroni Tufty 2350. It boots the Tufty display, shows the MCU verifier receiver state, and uses the same proof-transfer protocol as the ESP32-S3 BLE firmware:

- device name: `ZKMCU`
- service: `7b7c0001-78f1-4f9a-8b29-6f1f1d95a100`
- control: `7b7c0002-78f1-4f9a-8b29-6f1f1d95a100`
- data: `7b7c0003-78f1-4f9a-8b29-6f1f1d95a100`
- status: `7b7c0004-78f1-4f9a-8b29-6f1f1d95a100`

The receiver accepts `START <key_len> <proof_len> <proof_sha>`, packet kind `1` for the postcard verifier key, packet kind `2` for the postcard proof envelope, and `COMMIT`. On `COMMIT`, it runs the portable no_std Halo2/KZG verifier and renders the result on the Tufty LCD.

The Badgeware Tufty 2350 board includes a Raspberry Pi RM2 radio module with CYW43439 WiFi and Bluetooth support. Tufty's RM2 wiring matches the CYW43 PIO-SPI path used by Embassy: `WL_ON=GPIO23`, shared `WL_D=GPIO24`, `WL_CS=GPIO25`, and `WL_CLK=GPIO29`.

The BLE firmware lives in the isolated `ble/` package so it can use the current Embassy `cyw43` + TrouBLE stack without forcing the ESP32 MCU workspace onto the same Embassy timer queue versions. It initializes CYW43439 Bluetooth HCI, advertises as `ZKMCU`, exposes the GATT service above, and feeds control/data writes into the same `ProofApp::control_write` / `ProofApp::data_write` path as the display/self-test firmware.

Build:

```sh
cargo build -p openvm-mcu-rp2350 --target thumbv8m.main-none-eabihf
```

Build the Tufty BLE verifier firmware:

```sh
cargo build --manifest-path crates/zkvms/openvm/mcu/boards/rp2350/ble/Cargo.toml --target thumbv8m.main-none-eabihf
```

The BLE build downloads CYW43439 firmware blobs into `ble/cyw43-firmware` the first time it runs.

Flash from the repository root while the Tufty is in BOOTSEL:

```sh
picotool load --ignore-partitions -v -x crates/zkvms/openvm/mcu/target/thumbv8m.main-none-eabihf/debug/openvm-mcu-rp2350 -t elf
```

Flash the BLE verifier from the repository root while the Tufty is in BOOTSEL:

```sh
picotool load --ignore-partitions -v -x crates/zkvms/openvm/mcu/boards/rp2350/ble/target/thumbv8m.main-none-eabihf/debug/openvm-mcu-rp2350-ble -t elf
```

For an embedded-artifact verifier self-test, rebuild with `OPENVM_MCU_RP2350_EMBEDDED_SELF_TEST=1` and provide `OPENVM_MCU_RP2350_KEY`, `OPENVM_MCU_RP2350_PROOF`, and `OPENVM_MCU_RP2350_PROOF_SHA`.