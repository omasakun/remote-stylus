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
  ffi::{CStr, CString},
  mem, slice,
  sync::{
    mpsc::{self, Sender, TryRecvError},
    Arc, Mutex,
  },
  thread::{sleep, spawn},
  time::Duration,
};

use derive_new::new;
use esp_idf_svc::{log::EspLogger, sys::*};
use log::{error, info};
use once_cell::sync::OnceCell;
use utils::{char_to_code, hex_from_raw_data, is_keyboard_cod, KEYBOARD_REPORT_MAP};

use crate::{
  lorem::LOREM_IPSUM,
  utils::{
    ble_gap_event_name, ble_gap_set_device_name, ble_gap_set_security_param, ble_key_type_name,
    bt_controller_config_default, bt_gap_event_name, initialize_nvs,
  },
};

static DISCOVERY_MANAGER: OnceCell<Mutex<DiscoveryManager>> = OnceCell::new();
static TYPING_TASK: OnceCell<TypingTask> = OnceCell::new();
static SCAN_TASK: OnceCell<ScanTask> = OnceCell::new();

fn main() {
  // It is necessary to call this function once.
  // Otherwise some patches to the runtime implemented by esp-idf-sys might not link properly.
  // See https://github.com/esp-rs/esp-idf-template/issues/71
  link_patches();

  // Bind the log crate to the ESP Logging facilities
  EspLogger::initialize_default();

  initialize_nvs();

  unsafe {
    let mut config = bt_controller_config_default(esp_bt_mode_t_ESP_BT_MODE_BTDM);
    esp_nofail!(esp_bt_controller_init(&mut config));
    esp_nofail!(esp_bt_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BTDM));

    esp_nofail!(esp_bluedroid_init());
    esp_nofail!(esp_bluedroid_enable());

    // If pin_type is ESP_BT_PIN_TYPE_VARIABLE, pin_code and pin_code_len will be ignored,
    // and ESP_BT_GAP_PIN_REQ_EVT will come when control requests for pin code
    esp_nofail!(esp_bt_gap_set_pin(
      esp_bt_pin_type_t_ESP_BT_PIN_TYPE_VARIABLE,
      0,
      [0; 16].as_mut_ptr()
    ));

    esp_nofail!(esp_bt_gap_register_callback(Some(bt_gap_callback)));
    esp_nofail!(esp_ble_gap_register_callback(Some(ble_gap_callback)));

    // Allow BT devices to connect back to us
    esp_nofail!(esp_bt_gap_set_scan_mode(
      esp_bt_connection_mode_t_ESP_BT_CONNECTABLE,
      esp_bt_discovery_mode_t_ESP_BT_NON_DISCOVERABLE
    ));
  }

  let device_name = "Keyboard Bridge";
  let manufacturer = "Remote Desktop";
  let serial_number = "0137";

  esp_hid_ble_gap_adv_init(device_name);

  unsafe { esp_nofail!(esp_ble_gatts_register_callback(Some(esp_hidd_gatts_event_handler))) }
  unsafe { esp_nofail!(esp_ble_gattc_register_callback(Some(esp_hidh_gattc_event_handler))) }

  let hid_dev = ble_hidd_init(
    device_name,
    manufacturer,
    serial_number,
    &KEYBOARD_REPORT_MAP,
    Some(hidd_event_callback),
  );

  unsafe {
    esp_nofail!(esp_hidh_init(&esp_hidh_config_t {
      callback: Some(hidh_event_callback),
      event_stack_size: 4096,
      callback_arg: std::ptr::null_mut(),
    }))
  }

  // spawn_heap_logger();

  DISCOVERY_MANAGER.get_or_init(|| Mutex::new(DiscoveryManager::new()));
  TYPING_TASK.get_or_init(|| TypingTask::new(hid_dev));
  SCAN_TASK.get_or_init(ScanTask::new);

  // loop {
  //   info!("main thread is alive");
  //   sleep(Duration::from_secs(100));
  // }
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

      sleep(Duration::from_secs(1));

      // info!("typing task: typing");

      let c = LOREM_IPSUM.as_bytes()[*i];
      *i = (*i + 1) % LOREM_IPSUM.len();

      hidd_send_keyboard_press(hid_dev, c);
      sleep(Duration::from_millis(10));
      hidd_send_keyboard_release(hid_dev);
      sleep(Duration::from_millis(10));
    }

    spawn(move || loop {
      match resume_rx.recv() {
        Ok(_) => {
          info!("typing task: resume");
        }
        Err(_) => {
          info!("typing task: exit");
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
            info!("typing task: exit");
            return;
          }
        }
        task(&hid_dev, &mut i);
      }
    });
    this
  }
  fn on_hidd_resume(&self) {
    self.resume.send(()).unwrap();
  }
  fn on_hidd_pause(&self) {
    self.pause.send(()).unwrap();
  }
}

#[derive(Clone)]
struct ScanTask {
  scanning: Arc<Mutex<bool>>,
}
impl ScanTask {
  fn new() -> Self {
    let this = Self {
      scanning: Arc::new(Mutex::new(true)),
    };

    spawn({
      let this = this.clone();
      move || loop {
        if !this.is_scanning() {
          sleep(Duration::from_secs(1));
          continue;
        }

        info!("scan task: scanning");

        let devices = bt_hidh_scan(Duration::from_secs(5));
        if !this.is_scanning() {
          continue;
        }

        let mut keyboard = None;
        for device in devices.iter().filter(|d| d.is_keyboard()) {
          if device.is_keyboard() {
            info!("scan task: found keyboard: {:?}", device);
            keyboard = Some(device);
          } else {
            info!("scan task: found device: {:?}", device);
          }
        }
        if let Some(keyboard) = keyboard {
          info!("scan task: connecting to keyboard: {:?}", keyboard);
          let _ = unsafe {
            esp!(esp_hidh_dev_open(
              keyboard.bda.as_ptr() as _,
              esp_hid_transport_t_ESP_HID_TRANSPORT_BT,
              0
            ))
          }
          .inspect_err(|e| {
            error!("failed to open hid device: {:?}", e);
          });
          this.set_scanning(false);
        }
      }
    });
    this
  }
  fn is_scanning(&self) -> bool {
    *self.scanning.lock().unwrap()
  }
  fn set_scanning(&self, scanning: bool) {
    *self.scanning.lock().unwrap() = scanning;
  }
  fn on_hidh_open(&self, status: esp_err_t) {
    if status == ESP_OK {
      self.set_scanning(false);
    } else {
      self.set_scanning(true);
    }
  }
  fn on_hidh_close(&self) {
    self.set_scanning(true);
  }
  fn on_hidh_input(&self, usage_type: esp_hid_usage_t, data: &[u8]) {
    if usage_type & esp_hid_usage_t_ESP_HID_USAGE_KEYBOARD == esp_hid_usage_t_ESP_HID_USAGE_KEYBOARD {
      // https://usb.org/sites/default/files/hut1_22.pdf
      // TODO: send key press/release
    }
  }
}

fn bt_hidh_scan(duration: Duration) -> Vec<BtDevice> {
  let duration = duration.as_secs() as f64 / 1.28;
  let duration = duration as u8;

  let (tx, rx) = mpsc::channel();

  if let Some(manager) = DISCOVERY_MANAGER.get() {
    let mut manager = manager.lock().unwrap();
    manager.start_discovery(tx);
  } else {
    panic!("discovery manager not initialized");
  }

  unsafe {
    esp_nofail!(esp_bt_gap_start_discovery(
      esp_bt_inq_mode_t_ESP_BT_INQ_MODE_GENERAL_INQUIRY,
      duration,
      0
    ))
  };

  rx.recv().unwrap()
}

extern "C" fn hidh_event_callback(
  _handler_args: *mut std::ffi::c_void,
  _base: esp_event_base_t,
  event: i32,
  param: *mut std::ffi::c_void,
) {
  let event = event as esp_hidh_event_t;
  let param = param as *mut esp_hidh_event_data_t;
  let param = unsafe { &*param };

  match event {
    esp_hidh_event_t_ESP_HIDH_START_EVENT => {
      info!("hidh: start");
    }
    esp_hidh_event_t_ESP_HIDH_OPEN_EVENT => {
      let open = unsafe { param.open };
      if open.status == ESP_OK {
        let bda = get_hidh_dev_bda(open.dev);
        info!("hidh[{}]: open", bda);
      } else {
        info!("hidh: open failed: {}", open.status);
      }
      if let Some(task) = SCAN_TASK.get() {
        task.on_hidh_open(open.status);
      }
    }
    esp_hidh_event_t_ESP_HIDH_BATTERY_EVENT => {
      let battery = unsafe { param.battery };
      let bda = get_hidh_dev_bda(battery.dev);
      info!("hidh[{}]: battery: {}%", bda, battery.level);
    }
    esp_hidh_event_t_ESP_HIDH_INPUT_EVENT => {
      let input = unsafe { param.input };
      let bda = get_hidh_dev_bda(input.dev);
      let usage_type = unsafe { esp_hid_usage_str(input.usage) };
      let usage_type = unsafe { CStr::from_ptr(usage_type).to_str().unwrap() };
      let data = hex_from_raw_data(input.data, input.length as usize);
      info!(
        "hidh[{}]: input: {}, map: {}, id: {}, data: {}",
        bda, usage_type, input.map_index, input.report_id, data
      );
      if let Some(task) = SCAN_TASK.get() {
        task.on_hidh_input(input.usage, unsafe {
          slice::from_raw_parts(input.data, input.length as usize)
        });
      }
    }
    esp_hidh_event_t_ESP_HIDH_FEATURE_EVENT => {
      let feature = unsafe { param.feature };
      let bda = get_hidh_dev_bda(feature.dev);
      let usage_type = unsafe { esp_hid_usage_str(feature.usage) };
      let usage_type = unsafe { CStr::from_ptr(usage_type).to_str().unwrap() };
      let data = hex_from_raw_data(feature.data, feature.length as usize);
      info!(
        "hidh[{}]: feature: {}, map: {}, id: {}, data: {}",
        bda, usage_type, feature.map_index, feature.report_id, data
      );
    }
    esp_hidh_event_t_ESP_HIDH_CLOSE_EVENT => {
      let close = unsafe { param.close };
      let bda = get_hidh_dev_bda(close.dev);
      info!("hidh[{}]: close", bda);
      if let Some(task) = SCAN_TASK.get() {
        task.on_hidh_close();
      }
    }
    esp_hidh_event_t_ESP_HIDH_STOP_EVENT => {
      info!("hidh: stop");
    }
    _ => {
      info!("hidh: unhandled event: {:?}", event);
    }
  }
}

fn get_hidh_dev_bda(dev: *mut esp_hidh_dev_t) -> String {
  let bda = unsafe { esp_hidh_dev_bda_get(dev) };
  let bda = unsafe {
    slice::from_raw_parts(bda, 6)
      .iter()
      .map(|&x| format!("{:02x}", x))
      .collect::<Vec<_>>()
      .join(":")
  };
  bda
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

extern "C" fn hidd_event_callback(
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
      let mode = unsafe { CStr::from_ptr(mode).to_str().unwrap() };
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
      if let Some(task) = TYPING_TASK.get() {
        if control.control == 1 {
          task.on_hidd_resume();
        } else {
          task.on_hidd_pause();
        }
      }
    }
    esp_hidd_event_t_ESP_HIDD_OUTPUT_EVENT => {
      let output = unsafe { &param.output };
      let data = hex_from_raw_data(output.data, output.length as usize);
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
      let reason = unsafe { CStr::from_ptr(reason).to_str().unwrap() };
      info!("hidd: disconnect: {}", reason);
      if let Some(task) = TYPING_TASK.get() {
        task.on_hidd_pause();
      }
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

fn hidd_send_keyboard_press(hid_dev: *mut esp_hidd_dev_t, key: u8) {
  let mut data = char_to_code(key);
  let _ = unsafe { esp!(esp_hidd_dev_input_set(hid_dev, 0, 1, data.as_mut_ptr(), data.len())) }.inspect_err(|e| {
    error!("failed to send keyboard input: {:?}", e);
  });
}

fn hidd_send_keyboard_release(hid_dev: *mut esp_hidd_dev_t) {
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
      info!("ble-gap: key type: {}", ble_key_type_name(ble_key.key_type));
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_SEC_REQ_EVT => {
      let mut ble_req = unsafe { param.ble_security.ble_req };
      info!("ble-gap: security request");
      unsafe { esp_nofail!(esp_ble_gap_security_rsp(ble_req.bd_addr.as_mut_ptr(), true)) }
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_AUTH_CMPL_EVT => {
      let auth_cmpl = unsafe { param.ble_security.auth_cmpl };
      if auth_cmpl.success {
        info!("ble-gap: auth success");
      } else {
        info!("ble-gap: auth failed: {}", auth_cmpl.fail_reason);
      }
      if let Some(task) = TYPING_TASK.get() {
        task.on_hidd_resume();
      }
    }
    esp_gap_ble_cb_event_t_ESP_GAP_BLE_UPDATE_CONN_PARAMS_EVT => {
      let params = unsafe { param.update_conn_params };
      info!("ble-gap: update conn params: {:?}", params);
    }
    _ => {
      info!("ble-gap: {}", ble_gap_event_name(event));
    }
  }
}

extern "C" fn bt_gap_callback(event: esp_bt_gap_cb_event_t, param: *mut esp_bt_gap_cb_param_t) {
  let param = unsafe { &*param };
  match event {
    esp_bt_gap_cb_event_t_ESP_BT_GAP_DISC_STATE_CHANGED_EVT => {
      let state_change = unsafe { param.disc_st_chg };
      let state = if state_change.state == 0 { "stopped" } else { "started" };
      info!("bt-gap: discovery {}", state);
      if state_change.state == 0 {
        if let Some(manager) = DISCOVERY_MANAGER.get() {
          let mut manager = manager.lock().unwrap();
          manager.finish_discovery();
        }
      }
    }
    esp_bt_gap_cb_event_t_ESP_BT_GAP_DISC_RES_EVT => {
      let disc_res = unsafe { param.disc_res };
      let device = parse_bt_device_result(disc_res);
      if let Some(manager) = DISCOVERY_MANAGER.get() {
        let mut manager = manager.lock().unwrap();
        manager.add_result(device);
      }
    }
    esp_bt_gap_cb_event_t_ESP_BT_GAP_MODE_CHG_EVT => {
      let mode_chg = unsafe { param.mode_chg };
      info!("bt-gap: mode change: {}", mode_chg.mode);
    }
    esp_bt_gap_cb_event_t_ESP_BT_GAP_PIN_REQ_EVT => {
      let mut pin_req = unsafe { param.pin_req };
      if pin_req.min_16_digit {
        info!("bt-gap: input pin code 0000 0000 0000 0000");
        let mut pin_code: esp_bt_pin_code_t = [0; 16];
        unsafe {
          esp_nofail!(esp_bt_gap_pin_reply(
            pin_req.bda.as_mut_ptr(),
            true,
            16,
            pin_code.as_mut_ptr()
          ))
        }
      } else {
        info!("bt-gap: input pin code 0000");
        let mut pin_code: esp_bt_pin_code_t = [0; 16];
        unsafe {
          esp_nofail!(esp_bt_gap_pin_reply(
            pin_req.bda.as_mut_ptr(),
            true,
            4,
            pin_code.as_mut_ptr()
          ))
        }
      }
    }
    _ => {
      info!("bt-gap: {}", bt_gap_event_name(event));
    }
  }
}

struct DiscoveryManager {
  devices: Vec<BtDevice>,
  callback: Option<Sender<Vec<BtDevice>>>,
}
impl DiscoveryManager {
  fn new() -> Self {
    Self {
      devices: vec![],
      callback: None,
    }
  }
  fn start_discovery(&mut self, callback: Sender<Vec<BtDevice>>) {
    assert!(self.callback.is_none(), "discovery already in progress");
    self.devices.clear();
    self.callback = Some(callback);
  }
  fn add_result(&mut self, device: BtDevice) {
    match self.devices.iter().position(|d| d.bda == device.bda) {
      Some(i) => {
        self.devices[i].merge(device);
      }
      None => {
        self.devices.push(device);
      }
    }
  }
  fn finish_discovery(&mut self) {
    let devices = mem::take(&mut self.devices);
    if let Some(callback) = self.callback.take() {
      callback.send(devices).unwrap();
    }
  }
}

#[derive(Debug, new)]
struct BtDevice {
  bda: [u8; 6],
  name: Option<String>,
  rssi: Option<i8>,
  cod: Option<u32>,
}
impl BtDevice {
  fn merge(&mut self, other: BtDevice) {
    if let Some(name) = other.name {
      self.name = Some(name);
    }
    if let Some(rssi) = other.rssi {
      if let Some(self_rssi) = self.rssi {
        self.rssi = Some(self_rssi.max(rssi));
      } else {
        self.rssi = Some(rssi);
      }
    }
    if let Some(cod) = other.cod {
      self.cod = Some(cod);
    }
  }
  fn is_keyboard(&self) -> bool {
    if let Some(cod) = self.cod {
      is_keyboard_cod(cod)
    } else {
      false
    }
  }
}

fn parse_bt_device_result(disc_res: esp_bt_gap_cb_param_t_disc_res_param) -> BtDevice {
  let bda = disc_res.bda;
  let mut device = BtDevice::new(bda, None, None, None);

  // let bda = bda.iter().map(|&x| format!("{:02x}", x)).collect::<Vec<_>>().join(":");
  // info!("bt-gap: device found: {}", bda);

  let props = unsafe { slice::from_raw_parts(disc_res.prop, disc_res.num_prop as usize) };
  for prop in props {
    match prop.type_ {
      esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_BDNAME => {
        let name = unsafe { CStr::from_ptr(prop.val as *const i8).to_str().unwrap() };
        device.name = Some(name.to_string());
        // info!("bt-gap:   name: {}", name);
      }
      esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_COD => {
        let cod = unsafe { *(prop.val as *const u32) };
        device.cod = Some(cod);
        // info!("bt-gap:   class: {:08x}", cod);
      }
      esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_RSSI => {
        let rssi = unsafe { *(prop.val as *const i8) };
        device.rssi = Some(rssi);
        // info!("bt-gap:   rssi: {}", rssi);
      }
      esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_EIR => {
        // let eir = unsafe { slice::from_raw_parts(prop.val as *const u8, prop.len as usize) };
        // let eir = eir.iter().map(|&x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ");
        // info!("bt-gap:   eir: {}", eir);
        // TODO: call esp_bt_gap_resolve_eir_data to retrieve device name (ESP_BT_EIR_TYPE_CMPL_LOCAL_NAME, ESP_BT_EIR_TYPE_SHORT_LOCAL_NAME)
      }
      _ => {}
    }
  }

  device
}
