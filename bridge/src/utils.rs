use std::{
  thread::{sleep, spawn},
  time::Duration,
};

use esp_idf_svc::sys::*;
use log::{info, warn};

/// Macro to move a value to the heap and don't free it
macro_rules! leak {
  ($val:expr) => {
    // TODO: check all the places where this macro is used and ensure that the memory is freed
    Box::into_raw(Box::new($val))
  };
}

pub fn initialize_nvs() {
  unsafe {
    let result = nvs_flash_init();
    if result == ESP_ERR_NVS_NO_FREE_PAGES || result == ESP_ERR_NVS_NEW_VERSION_FOUND {
      warn!("failed to initialize nvs flash, erasing...");
      esp_nofail!(nvs_flash_erase());
      esp_nofail!(nvs_flash_init());
    } else {
      esp_nofail!(result);
    }
  }
}

pub fn spawn_heap_logger() {
  spawn(move || loop {
    sleep(Duration::from_millis(1000));
    unsafe {
      info!(
        "free heap: {} (min: {})",
        esp_get_free_heap_size(),
        esp_get_minimum_free_heap_size()
      );
    }
  });
}

pub fn bt_controller_config_default() -> esp_bt_controller_config_t {
  esp_bt_controller_config_t {
    controller_task_stack_size: ESP_TASK_BT_CONTROLLER_STACK as _,
    controller_task_prio: ESP_TASK_BT_CONTROLLER_PRIO as _,
    hci_uart_no: BT_HCI_UART_NO_DEFAULT as _,
    hci_uart_baudrate: BT_HCI_UART_BAUDRATE_DEFAULT,
    scan_duplicate_mode: SCAN_DUPLICATE_MODE as _,
    scan_duplicate_type: SCAN_DUPLICATE_TYPE_VALUE as _,
    normal_adv_size: NORMAL_SCAN_DUPLICATE_CACHE_SIZE as _,
    mesh_adv_size: MESH_DUPLICATE_SCAN_CACHE_SIZE as _,
    send_adv_reserved_size: SCAN_SEND_ADV_RESERVED_SIZE as _,
    controller_debug_flag: CONTROLLER_ADV_LOST_DEBUG_BIT,
    mode: esp_bt_mode_t_ESP_BT_MODE_BLE as _,
    ble_max_conn: CONFIG_BTDM_CTRL_BLE_MAX_CONN_EFF as _,
    bt_max_acl_conn: CONFIG_BTDM_CTRL_BR_EDR_MAX_ACL_CONN_EFF as _,
    bt_sco_datapath: CONFIG_BTDM_CTRL_BR_EDR_SCO_DATA_PATH_EFF as _,
    auto_latency: BTDM_CTRL_AUTO_LATENCY_EFF != 0,
    bt_legacy_auth_vs_evt: BTDM_CTRL_LEGACY_AUTH_VENDOR_EVT_EFF != 0,
    bt_max_sync_conn: CONFIG_BTDM_CTRL_BR_EDR_MAX_SYNC_CONN_EFF as _,
    ble_sca: CONFIG_BTDM_BLE_SLEEP_CLOCK_ACCURACY_INDEX_EFF as _,
    pcm_role: CONFIG_BTDM_CTRL_PCM_ROLE_EFF as _,
    pcm_polar: CONFIG_BTDM_CTRL_PCM_POLAR_EFF as _,
    hli: BTDM_CTRL_HLI != 0,
    magic: ESP_BT_CONTROLLER_CONFIG_MAGIC_VAL,
    dup_list_refresh_period: SCAN_DUPL_CACHE_REFRESH_PERIOD as _,
  }
}
