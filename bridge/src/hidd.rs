// Bluetooth LE HID Device

use std::{
  ffi::{CStr, CString},
  mem,
};

use esp_idf_svc::sys::*;
use log::info;
use once_cell::sync::OnceCell;

use crate::utils::{char_to_code, hex_from_raw_data};

static HANDLER: OnceCell<Box<dyn HidDeviceHandler>> = OnceCell::new();

pub struct HidDevice(*mut esp_hidd_dev_t);
unsafe impl Send for HidDevice {} // TODO: I don't know if this is actually okay
impl HidDevice {
  pub fn raw(&self) -> *mut esp_hidd_dev_t {
    self.0
  }
  pub fn send_input(&self, map_index: usize, report_id: usize, data: &mut [u8]) -> Result<(), EspError> {
    unsafe {
      esp!(esp_hidd_dev_input_set(
        self.raw(),
        map_index,
        report_id,
        data.as_mut_ptr(),
        data.len()
      ))
    }
  }
  pub fn send_keyboard_press(&self, key: u8) -> Result<(), EspError> {
    let mut data = char_to_code(key);
    self.send_input(0, 1, &mut data)
  }
  pub fn send_keyboard_release(&self) -> Result<(), EspError> {
    let mut data = [0; 8];
    self.send_input(0, 1, &mut data)
  }
}
impl From<*mut esp_hidd_dev_t> for HidDevice {
  fn from(ptr: *mut esp_hidd_dev_t) -> HidDevice {
    HidDevice(ptr)
  }
}

pub trait HidDeviceHandler: Send + Sync {
  fn on_resume(&self);
  fn on_pause(&self);
}

pub fn init_hid_device<T: HidDeviceHandler + 'static>(
  device_name: &str,
  manufacturer: &str,
  serial_number: &str,
  report_map: &[u8],
  handler: impl FnOnce(HidDevice) -> T,
) -> Result<HidDevice, EspError> {
  let mut device: *mut esp_hidd_dev_t = std::ptr::null_mut();

  unsafe {
    esp_hid_ble_gap_adv_init(device_name)?;
    esp!(esp_ble_gatts_register_callback(Some(esp_hidd_gatts_event_handler)))?;
  };

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
    esp!(esp_hidd_dev_init(
      &hid_config,
      esp_hid_transport_t_ESP_HID_TRANSPORT_BLE,
      Some(event_callback),
      &mut device as _
    ))?;
  };

  HANDLER
    .set(Box::new(handler(device.into())))
    .ok()
    .expect("handler already set");

  Ok(device.into())
}

// TODO: this api design is not good. create a gap callback manager?
pub fn notify_gap_auth_success() {
  if let Some(handler) = HANDLER.get() {
    handler.on_resume();
  }
}

fn esp_hid_ble_gap_adv_init(device_name: &str) -> Result<(), EspError> {
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

  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_AUTHEN_REQ_MODE, &auth_req)?;
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_IOCAP_MODE, &iocap)?;
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_INIT_KEY, &init_key)?;
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_RSP_KEY, &rsp_key)?;
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_MAX_KEY_SIZE, &key_size)?;
  ble_gap_set_security_param(esp_ble_sm_param_t_ESP_BLE_SM_SET_STATIC_PASSKEY, &passkey)?;

  ble_gap_set_device_name(device_name)?;
  unsafe { esp!(esp_ble_gap_config_adv_data(&mut adv_data)) }
}

fn ble_gap_set_security_param<T>(param: esp_ble_sm_param_t, value: &T) -> Result<(), EspError> {
  let value = value as *const T as _;
  let len = mem::size_of::<T>() as _;
  unsafe { esp!(esp_ble_gap_set_security_param(param, value, len)) }
}

fn ble_gap_set_device_name(name: &str) -> Result<(), EspError> {
  let name = CString::new(name).unwrap();
  let name = name.as_ptr();
  unsafe { esp!(esp_ble_gap_set_device_name(name)) }
}

extern "C" fn event_callback(
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
      start_advertising();
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
      if let Some(handler) = HANDLER.get() {
        if control.control == 1 {
          handler.on_resume();
        } else {
          handler.on_pause();
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
      if let Some(handler) = HANDLER.get() {
        handler.on_pause();
      }
      start_advertising();
    }
    esp_hidd_event_t_ESP_HIDD_STOP_EVENT => {
      info!("hidd: stop");
    }
    _ => {
      info!("hidd: unhandled event: {:?}", event);
    }
  }
}

fn start_advertising() {
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
