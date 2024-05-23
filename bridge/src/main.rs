// 注意を払わないこと:
// - unsafe をちゃんと扱う・伝搬させる・安全性の保証をする

// esp_idf_svc::sys::* を glob import すると vscode autocomplete が遅くなるので、
// 開発時には個別に import している

// https://github.com/espressif/esp-idf/tree/v5.2.1/examples/bluetooth/bluedroid/ble/gatt_server
// https://www.espressif.com/sites/default/files/documentation/esp32_bluetooth_architecture_en.pdf

#[macro_use]
mod utils;

use std::{thread::sleep, time::Duration};

use esp_idf_svc::sys::*;
use log::info;

use utils::{bt_controller_config_default, initialize_nvs, spawn_heap_logger};

const PROFILE_A_APP_ID: u16 = 0;

fn main() {
  // It is necessary to call this function once. Otherwise some patches to the runtime
  // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
  esp_idf_svc::sys::link_patches();

  // Bind the log crate to the ESP Logging facilities
  esp_idf_svc::log::EspLogger::initialize_default();

  initialize_nvs();

  let config = bt_controller_config_default();
  unsafe {
    esp_nofail!(esp_bt_controller_mem_release(
      esp_bt_mode_t_ESP_BT_MODE_CLASSIC_BT
    ));
    esp_nofail!(esp_bt_controller_init(leak!(config)));
    esp_nofail!(esp_bt_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE));

    esp_nofail!(esp_bluedroid_init());
    esp_nofail!(esp_bluedroid_enable());

    esp_nofail!(esp_ble_gatts_register_callback(Some(ble_gatts_callback)));
    esp_nofail!(esp_ble_gap_register_callback(Some(ble_gap_callback)));

    esp_nofail!(esp_ble_gatts_app_register(PROFILE_A_APP_ID));

    esp_nofail!(esp_ble_gatt_set_local_mtu(500));
  }

  spawn_heap_logger();

  loop {
    info!("main thread is alive");
    sleep(Duration::from_secs(10));
  }
}

extern "C" fn ble_gatts_callback(
  event: esp_gatts_cb_event_t,
  gatts_if: esp_gatt_if_t,
  param: *mut esp_ble_gatts_cb_param_t,
) {
  info!("gatts event: {:?}", event);
}

extern "C" fn ble_gap_callback(event: esp_gap_ble_cb_event_t, param: *mut esp_ble_gap_cb_param_t) {
  info!("gap event: {:?}", event);
}
