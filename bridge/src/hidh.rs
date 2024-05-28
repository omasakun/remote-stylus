// Bluetooth Classic HID Host

use std::{ffi::CStr, fmt::Display, slice};

use esp_idf_svc::sys::*;
use log::info;
use once_cell::sync::OnceCell;

use crate::utils::{hex_from_raw_data, BdAddr};

static HANDLER: OnceCell<Box<dyn HidHostHandler>> = OnceCell::new();

pub trait HidHostHandler: Send + Sync {
  fn on_open(&self, addr: BdAddr);
  fn on_open_failed(&self, error: EspError);
  fn on_close(&self, addr: BdAddr);
  fn on_input(&self, addr: BdAddr, usage_type: HidUsage, map_index: u8, report_id: u16, data: &[u8]);
}

pub fn init_hid_host<T: HidHostHandler + 'static>(handler: T) -> Result<(), EspError> {
  HANDLER.set(Box::new(handler)).ok().expect("handler already set");
  unsafe {
    // If pin_type is ESP_BT_PIN_TYPE_VARIABLE, pin_code and pin_code_len will be ignored,
    // and ESP_BT_GAP_PIN_REQ_EVT will come when control requests for pin code
    esp_nofail!(esp_bt_gap_set_pin(
      esp_bt_pin_type_t_ESP_BT_PIN_TYPE_VARIABLE,
      0,
      [0; 16].as_mut_ptr()
    ));

    // Allow BT devices to connect back to us
    esp_nofail!(esp_bt_gap_set_scan_mode(
      esp_bt_connection_mode_t_ESP_BT_CONNECTABLE,
      esp_bt_discovery_mode_t_ESP_BT_NON_DISCOVERABLE
    ));

    esp!(esp_ble_gattc_register_callback(Some(esp_hidh_gattc_event_handler)))?;
    esp!(esp_hidh_init(&esp_hidh_config_t {
      callback: Some(event_callback),
      event_stack_size: 4096,
      callback_arg: std::ptr::null_mut(),
    }))
  }
}

pub fn open_hid_device(addr: BdAddr) -> Result<(), EspError> {
  let addr = addr.raw();
  unsafe {
    esp!(esp_hidh_dev_open(
      addr.as_ptr() as _,
      esp_hid_transport_t_ESP_HID_TRANSPORT_BT,
      0
    ))
  }
}

extern "C" fn event_callback(
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
      info!("start");
    }
    esp_hidh_event_t_ESP_HIDH_OPEN_EVENT => {
      let open = unsafe { param.open };
      let error = EspError::from(open.status);
      match error {
        None => {
          let bda = get_hidh_dev_bda(open.dev);
          info!("{} open", bda);
          if let Some(handler) = HANDLER.get() {
            handler.on_open(bda);
          }
        }
        Some(e) => {
          info!("open failed: {}", e);
          if let Some(handler) = HANDLER.get() {
            handler.on_open_failed(e);
          }
        }
      }
    }
    esp_hidh_event_t_ESP_HIDH_BATTERY_EVENT => {
      let battery = unsafe { param.battery };
      let bda = get_hidh_dev_bda(battery.dev);
      info!("{} battery: {}%", bda, battery.level);
    }
    esp_hidh_event_t_ESP_HIDH_INPUT_EVENT => {
      let input = unsafe { param.input };
      let bda = get_hidh_dev_bda(input.dev);
      let usage = HidUsage::from(input.usage);
      let data = unsafe { hex_from_raw_data(input.data, input.length) };
      info!(
        "{} input: {}, map: {}, id: {}, data: {}",
        bda, usage, input.map_index, input.report_id, data
      );
      if let Some(handler) = HANDLER.get() {
        handler.on_input(bda, usage, input.map_index, input.report_id, unsafe {
          slice::from_raw_parts(input.data, input.length as usize)
        });
      }
    }
    esp_hidh_event_t_ESP_HIDH_FEATURE_EVENT => {
      let feature = unsafe { param.feature };
      let bda = get_hidh_dev_bda(feature.dev);
      let usage = HidUsage::from(feature.usage);
      let data = unsafe { hex_from_raw_data(feature.data, feature.length) };
      info!(
        "{} feature: {}, map: {}, id: {}, data: {}",
        bda, usage, feature.map_index, feature.report_id, data
      );
    }
    esp_hidh_event_t_ESP_HIDH_CLOSE_EVENT => {
      let close = unsafe { param.close };
      let bda = get_hidh_dev_bda(close.dev);
      info!("{} close", bda);
      if let Some(handler) = HANDLER.get() {
        handler.on_close(bda);
      }
    }
    esp_hidh_event_t_ESP_HIDH_STOP_EVENT => {
      info!("stop");
    }
    _ => {
      info!("unhandled event: {:?}", event);
    }
  }
}

pub struct HidUsage(esp_hid_usage_t);
impl HidUsage {
  pub fn raw(&self) -> esp_hid_usage_t {
    self.0
  }
  pub fn is_keyboard(&self) -> bool {
    self.raw() & esp_hid_usage_t_ESP_HID_USAGE_KEYBOARD != 0
  }
}
impl Display for HidUsage {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let usage = unsafe { esp_hid_usage_str(self.raw()) };
    let usage = unsafe { CStr::from_ptr(usage).to_str().unwrap() };
    write!(f, "{}", usage)
  }
}
impl From<esp_hid_usage_t> for HidUsage {
  fn from(value: esp_hid_usage_t) -> Self {
    Self(value)
  }
}

fn get_hidh_dev_bda(dev: *mut esp_hidh_dev_t) -> BdAddr {
  let bda = unsafe { esp_hidh_dev_bda_get(dev) };
  let bda = unsafe { slice::from_raw_parts(bda, 6) };
  let bda: [u8; 6] = bda.try_into().unwrap();
  bda.into()
}
