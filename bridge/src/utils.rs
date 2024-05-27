use std::{
  ffi::CString,
  fmt::{Debug, Display},
  mem, slice,
  thread::{sleep, spawn},
  time::Duration,
};

use esp_idf_svc::sys::*;
use log::{error, info, warn};

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
    sleep(Duration::from_millis(5000));
    unsafe {
      info!(
        "free heap: {} (min: {})",
        esp_get_free_heap_size(),
        esp_get_minimum_free_heap_size()
      );
    }
  });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BdAddr([u8; 6]);
impl BdAddr {
  pub fn raw(&self) -> [u8; 6] {
    self.0
  }
}
impl From<[u8; 6]> for BdAddr {
  fn from(value: [u8; 6]) -> Self {
    Self(value)
  }
}
impl Display for BdAddr {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let bda = self.raw();
    write!(
      f,
      "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
      bda[0], bda[1], bda[2], bda[3], bda[4], bda[5]
    )
  }
}
impl Debug for BdAddr {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    Display::fmt(self, f)
  }
}

// https://www.usb.org/sites/default/files/documents/hid1_11.pdf
pub const KEYBOARD_REPORT_MAP: [u8; 65] = [
  // 7 bytes input (modifiers, resrvd, keys*5), 1 byte output
  0x05, 0x01, // Usage Page (Generic Desktop Ctrls)
  0x09, 0x06, // Usage (Keyboard)
  0xA1, 0x01, // Collection (Application)
  0x85, 0x01, //   Report ID (1)
  0x05, 0x07, //   Usage Page (Kbrd/Keypad)
  0x19, 0xE0, //   Usage Minimum (0xE0)
  0x29, 0xE7, //   Usage Maximum (0xE7)
  0x15, 0x00, //   Logical Minimum (0)
  0x25, 0x01, //   Logical Maximum (1)
  0x75, 0x01, //   Report Size (1)
  0x95, 0x08, //   Report Count (8)
  0x81, 0x02, //   Input (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
  0x95, 0x01, //   Report Count (1)
  0x75, 0x08, //   Report Size (8)
  0x81, 0x03, //   Input (Const,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
  0x95, 0x05, //   Report Count (5)
  0x75, 0x01, //   Report Size (1)
  0x05, 0x08, //   Usage Page (LEDs)
  0x19, 0x01, //   Usage Minimum (Num Lock)
  0x29, 0x05, //   Usage Maximum (Kana)
  0x91, 0x02, //   Output (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
  0x95, 0x01, //   Report Count (1)
  0x75, 0x03, //   Report Size (3)
  0x91, 0x03, //   Output (Const,Var,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
  0x95, 0x05, //   Report Count (5)
  0x75, 0x08, //   Report Size (8)
  0x15, 0x00, //   Logical Minimum (0)
  0x25, 0x65, //   Logical Maximum (101)
  0x05, 0x07, //   Usage Page (Kbrd/Keypad)
  0x19, 0x00, //   Usage Minimum (0x00)
  0x29, 0x65, //   Usage Maximum (0x65)
  0x81, 0x00, //   Input (Data,Array,Abs,No Wrap,Linear,Preferred State,No Null Position)
  0xC0, //       End Collection
];

pub fn bt_controller_config_default(mode: esp_bt_mode_t) -> esp_bt_controller_config_t {
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
    mode: mode as _,
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

pub fn ble_gap_event_name(event: esp_gap_ble_cb_event_t) -> String {
  let names = [
    "ADV_DATA_SET_COMPLETE",
    "SCAN_RSP_DATA_SET_COMPLETE",
    "SCAN_PARAM_SET_COMPLETE",
    "SCAN_RESULT",
    "ADV_DATA_RAW_SET_COMPLETE",
    "SCAN_RSP_DATA_RAW_SET_COMPLETE",
    "ADV_START_COMPLETE",
    "SCAN_START_COMPLETE",
    "AUTH_CMPL",
    "KEY",
    "SEC_REQ",
    "PASSKEY_NOTIF",
    "PASSKEY_REQ",
    "OOB_REQ",
    "LOCAL_IR",
    "LOCAL_ER",
    "NC_REQ",
    "ADV_STOP_COMPLETE",
    "SCAN_STOP_COMPLETE",
    "SET_STATIC_RAND_ADDR",
    "UPDATE_CONN_PARAMS",
    "SET_PKT_LENGTH_COMPLETE",
    "SET_LOCAL_PRIVACY_COMPLETE",
    "REMOVE_BOND_DEV_COMPLETE",
    "CLEAR_BOND_DEV_COMPLETE",
    "GET_BOND_DEV_COMPLETE",
    "READ_RSSI_COMPLETE",
    "UPDATE_WHITELIST_COMPLETE",
  ];
  names
    .get(event as usize)
    .map(|s| s.to_string())
    .unwrap_or_else(|| format!("Unknown({})", event))
}

pub fn bt_gap_event_name(event: esp_bt_gap_cb_event_t) -> String {
  let names = [
    "DISC_RES",
    "DISC_STATE_CHANGED",
    "RMT_SRVCS",
    "RMT_SRVC_REC",
    "AUTH_CMPL",
    "PIN_REQ",
    "CFM_REQ",
    "KEY_NOTIF",
    "KEY_REQ",
    "READ_RSSI_DELTA",
    "CONFIG_EIR_DATA",
    "SET_AFH_CHANNELS",
    "READ_REMOTE_NAME",
    "MODE_CHG",
    "REMOVE_BOND_DEV_COMPLETE",
    "QOS_CMPL",
    "ACL_CONN_CMPL_STAT",
    "ACL_DISCONN_CMPL_STAT",
    "ACL_PKT_TYPE_CHANGED",
  ];
  names
    .get(event as usize)
    .map(|s| s.to_string())
    .unwrap_or_else(|| format!("Unknown({})", event))
}

pub fn ble_key_type_name(key_type: esp_ble_key_type_t) -> String {
  match key_type as _ {
    ESP_LE_KEY_NONE => "ESP_LE_KEY_NONE",
    ESP_LE_KEY_PENC => "ESP_LE_KEY_PENC",
    ESP_LE_KEY_PID => "ESP_LE_KEY_PID",
    ESP_LE_KEY_PCSRK => "ESP_LE_KEY_PCSRK",
    ESP_LE_KEY_PLK => "ESP_LE_KEY_PLK",
    ESP_LE_KEY_LLK => "ESP_LE_KEY_LLK",
    ESP_LE_KEY_LENC => "ESP_LE_KEY_LENC",
    ESP_LE_KEY_LID => "ESP_LE_KEY_LID",
    ESP_LE_KEY_LCSRK => "ESP_LE_KEY_LCSRK",
    _ => "INVALID",
  }
  .to_string()
}

pub fn char_to_code(key: u8) -> [u8; 8] {
  let mut data = [0; 8];
  match key {
    b'a'..=b'z' => {
      let code = key - b'a';
      data[2] = code + 4;
    }
    b'A'..=b'Z' => {
      let code = key - b'A';
      data[0] = 0x02; // shift
      data[2] = code + 4;
    }
    b'0' => {
      data[2] = 0x27;
    }
    b'1'..=b'9' => {
      let code = key - b'1';
      data[2] = code + 0x1e;
    }
    b' ' => {
      data[2] = 0x2c;
    }
    b'.' => {
      data[2] = 0x37;
    }
    b',' => {
      data[2] = 0x36;
    }
    b'\n' => {
      data[0] = 0x28;
    }
    _ => {
      error!("unsupported key: {}", key);
    }
  };
  data
}

pub fn hex_from_raw_data(data: *const u8, len: usize) -> String {
  unsafe {
    slice::from_raw_parts(data, len)
      .iter()
      .map(|&x| format!("{:02x}", x))
      .collect::<Vec<_>>()
      .join(" ")
  }
}
