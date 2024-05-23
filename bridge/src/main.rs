// unsafe をちゃんと扱う・伝搬させることは、ひとまず考えないでコードを書いてる！

// 引数をアレコレしないと呼べない関数は、ラッパー関数を作って呼ぶ、多分そうする

// esp_idf_svc::sys::* を glob import すると vscode autocomplete が遅くなるので、
// 開発時には個別に import している

// https://doc.rust-jp.rs/book-ja/ch19-01-unsafe-rust.html
// https://github.com/espressif/esp-idf/tree/v5.2.1/examples/bluetooth/esp_hid_device
// https://www.espressif.com/sites/default/files/documentation/esp32_bluetooth_architecture_en.pdf

#![allow(non_upper_case_globals)]

mod lorem;
mod utils;

use std::{
  ffi::CString,
  sync::mpsc::{self, TryRecvError},
  thread::{sleep, spawn},
  time::Duration,
};

use esp_idf_svc::{log::EspLogger, sys::*};
use log::{error, info};
use once_cell::sync::OnceCell;
use utils::{char_to_code, KEYBOARD_REPORT_MAP};

use crate::{
  lorem::LOREM_IPSUM,
  utils::{
    ble_gap_set_device_name, ble_gap_set_security_param, ble_key_type_name, bt_controller_config_default,
    gap_ble_event_name, initialize_nvs,
  },
};

static TYPING_TASK: OnceCell<TypingTask> = OnceCell::new();

fn main() {
  // It is necessary to call this function once.
  // Otherwise some patches to the runtime implemented by esp-idf-sys might not link properly.
  // See https://github.com/esp-rs/esp-idf-template/issues/71
  link_patches();

  // Bind the log crate to the ESP Logging facilities
  EspLogger::initialize_default();

  initialize_nvs();

  unsafe {
    let mut config = bt_controller_config_default(esp_bt_mode_t_ESP_BT_MODE_BLE);
    esp_nofail!(esp_bt_controller_mem_release(esp_bt_mode_t_ESP_BT_MODE_CLASSIC_BT));
    esp_nofail!(esp_bt_controller_init(&mut config));
    esp_nofail!(esp_bt_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE));

    esp_nofail!(esp_bluedroid_init());
    esp_nofail!(esp_bluedroid_enable());

    esp_nofail!(esp_ble_gap_register_callback(Some(ble_gap_callback)));
  }

  let device_name = "ESP32 Keyboard";
  let manufacturer = "Remote Desktop";
  let serial_number = "0137";

  esp_hid_ble_gap_adv_init(device_name);

  unsafe { esp_nofail!(esp_ble_gatts_register_callback(Some(esp_hidd_gatts_event_handler))) }

  let hid_dev = ble_hidd_init(
    device_name,
    manufacturer,
    serial_number,
    &KEYBOARD_REPORT_MAP,
    Some(ble_hidd_event_callback),
  );

  // spawn_heap_logger();

  TYPING_TASK.get_or_init(|| TypingTask::new(hid_dev));

  // loop {
  //   info!("main thread is alive");
  //   sleep(Duration::from_secs(100));
  // }
}

fn ble_hid_task_start_up() {
  if let Some(task) = TYPING_TASK.get() {
    task.resume();
  }
}

fn ble_hid_task_shut_down() {
  if let Some(task) = TYPING_TASK.get() {
    task.pause();
  }
}

// TODO: I don't know if this is actually safe to do
struct HiddDevBox(*mut esp_hidd_dev_t);
unsafe impl Send for HiddDevBox {}

struct TypingTask {
  resume: mpsc::Sender<()>,
  pause: mpsc::Sender<()>,
}
impl TypingTask {
  fn new(hid_dev: *mut esp_hidd_dev_t) -> Self {
    let hid_dev = HiddDevBox(hid_dev);
    let (resume_tx, resume_rx) = mpsc::channel();
    let (pause_tx, pause_rx) = mpsc::channel();
    let this = Self {
      resume: resume_tx,
      pause: pause_tx,
    };

    fn task(hid_dev: &HiddDevBox, i: &mut usize) {
      let hid_dev = hid_dev.0;

      // info!("typing task: typing");

      let c = LOREM_IPSUM.as_bytes()[*i];
      *i = (*i + 1) % LOREM_IPSUM.len();

      ble_hidd_send_keyboard_press(hid_dev, c);
      sleep(Duration::from_millis(10));
      ble_hidd_send_keyboard_release(hid_dev);
      sleep(Duration::from_millis(10));
    }

    spawn(move || loop {
      match resume_rx.recv() {
        Ok(_) => {
          info!("typing task: resume");
        }
        Err(_) => {
          info!("typing task: resume channel closed");
          return;
        }
      }

      let mut i = 0;
      sleep(Duration::from_secs(5));

      loop {
        match pause_rx.try_recv() {
          Err(TryRecvError::Empty) => {}
          Ok(_) => {
            info!("typing task: pause");
            break;
          }
          Err(TryRecvError::Disconnected) => {
            info!("typing task: pause channel closed");
            return;
          }
        }
        task(&hid_dev, &mut i);
      }
    });
    this
  }
  fn resume(&self) {
    self.resume.send(()).unwrap();
  }
  fn pause(&self) {
    self.pause.send(()).unwrap();
  }
}

fn ble_hidd_init(
  device_name: &str,
  manufacturer: &str,
  serial_number: &str,
  report_map: &[u8],
  callback: esp_event_handler_t,
) -> *mut esp_hidd_dev_t {
  let mut hid_dev: *mut esp_hidd_dev_t = std::ptr::null_mut();

  let device_name = CString::new(device_name).unwrap();
  let manufacturer = CString::new(manufacturer).unwrap();
  let serial_number = CString::new(serial_number).unwrap();
  let hid_config = esp_hid_device_config_t {
    vendor_id: 0x16c0,
    product_id: 0x05df,
    version: 0x0100,
    device_name: device_name.as_ptr() as _,
    manufacturer_name: manufacturer.as_ptr() as _,
    serial_number: serial_number.as_ptr() as _,
    report_maps: &mut esp_hid_raw_report_map_t {
      data: report_map.as_ptr(),
      len: report_map.len() as _,
    } as _,
    report_maps_len: 1,
  };

  unsafe {
    esp_nofail!(esp_hidd_dev_init(
      &hid_config,
      esp_hid_transport_t_ESP_HID_TRANSPORT_BLE,
      callback,
      &mut hid_dev as _
    ))
  };
  hid_dev
}

extern "C" fn ble_hidd_event_callback(
  _handler_args: *mut std::ffi::c_void,
  _base: esp_event_base_t,
  event: i32,
  param: *mut std::ffi::c_void,
) {
  let event = event as esp_hidd_event_t;
  let param = param as *mut esp_hidd_event_data_t;
  let param = unsafe { &*param };

  match event {
    esp_hidd_event_t_ESP_HIDD_START_EVENT => {
      info!("hidd: start");
      esp_hid_ble_gap_adv_start();
    }
    esp_hidd_event_t_ESP_HIDD_CONNECT_EVENT => {
      info!("hidd: connect");
    }
    esp_hidd_event_t_ESP_HIDD_PROTOCOL_MODE_EVENT => {
      let protocol_mode = unsafe { &param.protocol_mode };
      let mode = unsafe { esp_hid_protocol_mode_str(protocol_mode.protocol_mode) };
      let mode = unsafe { std::ffi::CStr::from_ptr(mode).to_str().unwrap() };
      info!("hidd: protocol mode[{}] -> {}", protocol_mode.map_index, mode);
    }
    esp_hidd_event_t_ESP_HIDD_CONTROL_EVENT => {
      let control = unsafe { &param.control };
      let operation = if control.control == 1 {
        "exit_suspend"
      } else {
        "enter_suspend"
      };
      info!("hidd: control[{}] -> {}", control.map_index, operation);
      if control.control == 1 {
        ble_hid_task_start_up();
      } else {
        ble_hid_task_shut_down();
      }
    }
    esp_hidd_event_t_ESP_HIDD_OUTPUT_EVENT => {
      let output = unsafe { &param.output };
      let data = unsafe {
        std::slice::from_raw_parts(output.data, output.length as usize)
          .iter()
          .map(|&x| format!("{:02x}", x))
          .collect::<Vec<_>>()
          .join(" ")
      };
      info!("hidd: output[{}]: {:}", output.map_index, data);
    }
    esp_hidd_event_t_ESP_HIDD_FEATURE_EVENT => {
      let feature = unsafe { &param.feature };
      info!("hidd: feature[{}]: {:?}", feature.map_index, feature);
    }
    esp_hidd_event_t_ESP_HIDD_DISCONNECT_EVENT => {
      let disconnect = unsafe { &param.disconnect };
      let reason =
        unsafe { esp_hid_disconnect_reason_str(esp_hidd_dev_transport_get(disconnect.dev), disconnect.reason) };
      let reason = unsafe { std::ffi::CStr::from_ptr(reason).to_str().unwrap() };
      info!("hidd: disconnect: {}", reason);
      ble_hid_task_shut_down();
      esp_hid_ble_gap_adv_start();
    }
    esp_hidd_event_t_ESP_HIDD_STOP_EVENT => {
      info!("hidd: stop");
    }
    _ => {
      info!("hidd: unhandled event: {:?}", event);
    }
  }
}

fn ble_hidd_send_keyboard_press(hid_dev: *mut esp_hidd_dev_t, key: u8) {
  let mut data = char_to_code(key);
  let _ = unsafe { esp!(esp_hidd_dev_input_set(hid_dev, 0, 1, data.as_mut_ptr(), data.len())) }.inspect_err(|e| {
    error!("failed to send keyboard input: {:?}", e);
  });
}
fn ble_hidd_send_keyboard_release(hid_dev: *mut esp_hidd_dev_t) {
  let mut data = [0; 8];
  let _ = unsafe { esp!(esp_hidd_dev_input_set(hid_dev, 0, 1, data.as_mut_ptr(), data.len())) }.inspect_err(|e| {
    error!("failed to send keyboard input: {:?}", e);
  });
}

fn esp_hid_ble_gap_adv_init(device_name: &str) {
  let appearance = ESP_BLE_APPEARANCE_HID_KEYBOARD;

  // https://www.bluetooth.com/specifications/assigned-numbers/
  // UUID for human interface device service
  let mut hidd_service_uuid: [u8; 16] = [
    0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x12, 0x18, 0x00, 0x00,
  ];

  let mut adv_data = esp_ble_adv_data_t {
    set_scan_rsp: false,
    include_name: true,
    include_txpower: true,
    min_interval: 0x0006, // time = min_interval * 1.25 ms
    max_interval: 0x0010, // time = max_interval * 1.25 ms
    appearance: appearance as _,
    manufacturer_len: 0,
    p_manufacturer_data: std::ptr::null_mut(),
    service_data_len: 0,
    p_service_data: std::ptr::null_mut(),
    service_uuid_len: hidd_service_uuid.len() as _,
    p_service_uuid: hidd_service_uuid.as_mut_ptr(),
    flag: 0x6,
  };

  // TODO: what are these?
  // https://github.com/espressif/esp-idf/blob/master/examples/bluetooth/bluedroid/ble/gatt_security_server/tutorial/Gatt_Security_Server_Example_Walkthrough.md
  let auth_req = ESP_LE_AUTH_REQ_SC_MITM_BOND;
  let iocap = ESP_IO_CAP_NONE;
  let init_key = ESP_BLE_ENC_KEY_MASK | ESP_BLE_ID_KEY_MASK;
  let rsp_key = ESP_BLE_ENC_KEY_MASK | ESP_BLE_ID_KEY_MASK;
  let key_size = 16;
  let passkey = 0;

  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_AUTHEN_REQ_MODE, &auth_req);
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_IOCAP_MODE, &iocap);
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_INIT_KEY, &init_key);
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_RSP_KEY, &rsp_key);
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_MAX_KEY_SIZE, &key_size);
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_STATIC_PASSKEY, &passkey);

  ble_gap_set_device_name(device_name);
  unsafe { esp_nofail!(esp_ble_gap_config_adv_data(&mut adv_data)) }
}

fn esp_hid_ble_gap_adv_start() {
  let mut hidd_adv_params = esp_ble_adv_params_t {
    adv_int_min: 0x20,
    adv_int_max: 0x30,
    adv_type: esp_ble_adv_type_t_ADV_TYPE_IND,
    own_addr_type: esp_ble_addr_type_t_BLE_ADDR_TYPE_PUBLIC,
    channel_map: esp_ble_adv_channel_t_ADV_CHNL_ALL,
    adv_filter_policy: esp_ble_adv_filter_t_ADV_FILTER_ALLOW_SCAN_ANY_CON_ANY,
    ..Default::default()
  };
  unsafe { esp_nofail!(esp_ble_gap_start_advertising(&mut hidd_adv_params)) }
}

extern "C" fn ble_gap_callback(event: esp_gap_ble_cb_event_t, param: *mut esp_ble_gap_cb_param_t) {
  let param = unsafe { &*param };
  match event {
    // scan
    // advertise
    // authentication
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_KEY_EVT => {
      let ble_key = unsafe { param.ble_security.ble_key };
      info!("gap: key type: {}", ble_key_type_name(ble_key.key_type));
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_SEC_REQ_EVT => {
      let mut ble_req = unsafe { param.ble_security.ble_req };
      info!("gap: security request");
      unsafe { esp_nofail!(esp_ble_gap_security_rsp(ble_req.bd_addr.as_mut_ptr(), true)) }
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_AUTH_CMPL_EVT => {
      let auth_cmpl = unsafe { param.ble_security.auth_cmpl };
      if auth_cmpl.success {
        info!("gap: auth success");
      } else {
        info!("gap: auth failed: {}", auth_cmpl.fail_reason);
      }
      ble_hid_task_start_up()
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_UPDATE_CONN_PARAMS_EVT => {
      let params = unsafe { param.update_conn_params };
      info!("gap: update conn params: {:?}", params);
    }
    _ => {
      info!("gap: {}", gap_ble_event_name(event));
    }
  }
}
