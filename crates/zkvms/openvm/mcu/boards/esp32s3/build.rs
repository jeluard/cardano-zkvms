use std::{env, fs, path::PathBuf};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("espidf") {
        embuild::espidf::sysenv::output();
    }

    for key in [
        "ESP32S3_LCD_SCLK",
        "ESP32S3_LCD_MOSI",
        "ESP32S3_LCD_CS",
        "ESP32S3_LCD_DC",
        "ESP32S3_LCD_RST",
        "ESP32S3_LCD_BL",
        "ESP32S3_LCD_WIDTH",
        "ESP32S3_LCD_HEIGHT",
        "ESP32S3_TOUCH_SCL",
        "ESP32S3_TOUCH_SDA",
        "ESP32S3_TOUCH_INT",
        "ESP32S3_HOST_EVM_STATUS",
        "ESP32S3_HOST_EVM_DETAIL",
        "ESP32S3_HOST_EVM_PROOF_SHA",
        "ESP32S3_HOST_EVM_PUBLIC_VALUES",
        "ESP32S3_VERIFIER_KEY",
        "ESP32S3_PROOF_ENVELOPE",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let lcd_pins_path = out_dir.join("lcd_pins.rs");
    let host_status_path = out_dir.join("host_status.rs");

    let display_enabled = env::var_os("CARGO_FEATURE_DISPLAY_ST7789").is_some();
    let source = if display_enabled {
        let sclk = optional_u8("ESP32S3_LCD_SCLK", 39);
        let mosi = optional_u8("ESP32S3_LCD_MOSI", 38);
        let cs = optional_u8("ESP32S3_LCD_CS", 45);
        let dc = optional_u8("ESP32S3_LCD_DC", 42);
        let rst = optional_u8("ESP32S3_LCD_RST", 0);
        let bl = optional_u8("ESP32S3_LCD_BL", 1);
        let width = optional_u16("ESP32S3_LCD_WIDTH", 240);
        let height = optional_u16("ESP32S3_LCD_HEIGHT", 320);
        let touch_scl = optional_u8("ESP32S3_TOUCH_SCL", 47);
        let touch_sda = optional_u8("ESP32S3_TOUCH_SDA", 48);
        let touch_int = optional_u8("ESP32S3_TOUCH_INT", 46);

        format!(
            "pub const SCLK: u8 = {sclk};\n\
             pub const MOSI: u8 = {mosi};\n\
             pub const CS: u8 = {cs};\n\
             pub const DC: u8 = {dc};\n\
             pub const RST: u8 = {rst};\n\
             pub const BL: u8 = {bl};\n\
             pub const WIDTH: u16 = {width};\n\
             pub const HEIGHT: u16 = {height};\n\
             pub const TOUCH_SCL: u8 = {touch_scl};\n\
             pub const TOUCH_SDA: u8 = {touch_sda};\n\
             pub const TOUCH_INT: u8 = {touch_int};\n"
        )
    } else {
        "pub const SCLK: u8 = 0;\n\
         pub const MOSI: u8 = 0;\n\
         pub const CS: u8 = 0;\n\
         pub const DC: u8 = 0;\n\
         pub const RST: u8 = 0;\n\
         pub const BL: u8 = 0;\n\
         pub const WIDTH: u16 = 240;\n\
         pub const HEIGHT: u16 = 320;\n\
         pub const TOUCH_SCL: u8 = 47;\n\
         pub const TOUCH_SDA: u8 = 48;\n\
         pub const TOUCH_INT: u8 = 46;\n"
            .to_string()
    };

    fs::write(lcd_pins_path, source).expect("write generated LCD pin constants");
    fs::write(host_status_path, host_status_source()).expect("write generated host status");
}

fn host_status_source() -> String {
    let embedded = env::var_os("ESP32S3_HOST_EVM_STATUS").is_some();
    let status = optional_string("ESP32S3_HOST_EVM_STATUS", "no host proof flashed");
    let detail = optional_string(
        "ESP32S3_HOST_EVM_DETAIL",
        "run mcu-esp32s3-flash-evm-status",
    );
    let proof_sha = optional_string("ESP32S3_HOST_EVM_PROOF_SHA", "n/a");
    let public_values = optional_string("ESP32S3_HOST_EVM_PUBLIC_VALUES", "n/a");
    let key_path = optional_include_bytes("ESP32S3_VERIFIER_KEY");
    let proof_path = optional_include_bytes("ESP32S3_PROOF_ENVELOPE");
    format!(
        "pub const HOST_EVM_EMBEDDED: bool = {embedded};\n\
         pub const HOST_EVM_STATUS: &str = {status:?};\n\
         pub const HOST_EVM_DETAIL: &str = {detail:?};\n\
         pub const HOST_EVM_PROOF_SHA: &str = {proof_sha:?};\n\
         pub const HOST_EVM_PUBLIC_VALUES: &str = {public_values:?};\n\
         pub const HOST_VERIFIER_KEY: &[u8] = {key_path};\n\
         pub const HOST_PROOF_ENVELOPE: &[u8] = {proof_path};\n"
    )
}

fn optional_string(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn optional_include_bytes(key: &str) -> String {
    match env::var(key) {
        Ok(path) if !path.is_empty() => format!("include_bytes!({path:?})"),
        _ => "&[]".to_owned(),
    }
}

fn optional_u8(key: &str, default: u8) -> u8 {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("{key} must be a GPIO number")),
        Err(_) => default,
    }
}

fn optional_u16(key: &str, default: u16) -> u16 {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("{key} must be a positive integer")),
        Err(_) => default,
    }
}
