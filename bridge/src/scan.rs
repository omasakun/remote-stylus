use std::{
  ffi::CStr,
  fmt::Debug,
  mem, slice,
  sync::{
    mpsc::{self, Sender},
    Mutex,
  },
  time::Duration,
};

use derive_new::new;
use esp_idf_svc::sys::*;
use once_cell::sync::OnceCell;

use crate::utils::BdAddr;

static DISCOVERY_MANAGER: OnceCell<Mutex<DiscoveryManager>> = OnceCell::new();

pub fn scan_bluetooth(duration: Duration) -> Vec<DiscoveredDevice> {
  let duration = duration.as_secs() as f64 / 1.28;
  let duration = duration as u8;

  let (tx, rx) = mpsc::channel();

  DISCOVERY_MANAGER.get_or_init(|| Mutex::new(DiscoveryManager::new()));

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

pub fn notify_discovery_finished() {
  if let Some(manager) = DISCOVERY_MANAGER.get() {
    let mut manager = manager.lock().unwrap();
    manager.finish_discovery();
  }
}

pub fn notify_discovery_result(device: impl Into<DiscoveredDevice>) {
  if let Some(manager) = DISCOVERY_MANAGER.get() {
    let mut manager = manager.lock().unwrap();
    manager.add_result(device.into());
  }
}

struct DiscoveryManager {
  devices: Vec<DiscoveredDevice>,
  callback: Option<Sender<Vec<DiscoveredDevice>>>,
}
impl DiscoveryManager {
  fn new() -> Self {
    Self {
      devices: vec![],
      callback: None,
    }
  }
  fn start_discovery(&mut self, callback: Sender<Vec<DiscoveredDevice>>) {
    assert!(self.callback.is_none(), "discovery already in progress");
    self.devices.clear();
    self.callback = Some(callback);
  }
  fn add_result(&mut self, device: DiscoveredDevice) {
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
pub struct DiscoveredDevice {
  pub bda: BdAddr,
  pub name: Option<String>,
  pub rssi: Option<i8>,
  pub cod: Option<u32>,
}
impl DiscoveredDevice {
  fn merge(&mut self, other: DiscoveredDevice) {
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
  pub fn is_keyboard(&self) -> bool {
    if let Some(cod) = self.cod {
      is_keyboard_cod(cod)
    } else {
      false
    }
  }
}
impl From<esp_bt_gap_cb_param_t_disc_res_param> for DiscoveredDevice {
  fn from(value: esp_bt_gap_cb_param_t_disc_res_param) -> Self {
    let mut device = Self::new(value.bda.into(), None, None, None);
    let props = unsafe { slice::from_raw_parts(value.prop, value.num_prop as usize) };
    for prop in props {
      match prop.type_ {
        esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_BDNAME => {
          let name = unsafe { CStr::from_ptr(prop.val as *const i8).to_str().unwrap() };
          device.name = Some(name.to_string());
        }
        esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_COD => {
          let cod = unsafe { *(prop.val as *const u32) };
          device.cod = Some(cod);
        }
        esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_RSSI => {
          let rssi = unsafe { *(prop.val as *const i8) };
          device.rssi = Some(rssi);
        }
        esp_bt_gap_dev_prop_type_t_ESP_BT_GAP_DEV_PROP_EIR => {
          // TODO: call esp_bt_gap_resolve_eir_data to retrieve device name (ESP_BT_EIR_TYPE_CMPL_LOCAL_NAME, ESP_BT_EIR_TYPE_SHORT_LOCAL_NAME)
        }
        _ => {}
      }
    }
    device
  }
}

#[allow(clippy::unusual_byte_groupings)]
fn is_keyboard_cod(cod: u32) -> bool {
  // https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Assigned_Numbers/out/en/Assigned_Numbers.pdf
  // class of device
  // - reserved_2:  2 bits
  // - minor:       6 bits
  // - major:       5 bits
  // - service:    11 bits
  // - reserved_8:  8 bits

  // example of cod for keyboard:
  // unused   service     major minor  unused
  // 00000000 00000000001 00101 010000 00
  //                      ^^^^^  ^
  //                 peripheral  keyboard

  (cod & 0b00000000_00000000000_11111_010000_00) == 0b00000000_00000000000_00101_010000_00
}
