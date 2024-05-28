// Importing esp_idf_svc::sys::* together slows down vscode autocomplete,
// so it is better to import them individually during development

// HID Usage Tables
// https://usb.org/sites/default/files/hut1_22.pdf

#![allow(non_upper_case_globals)]

mod hidd;
mod hidh;
mod scan;
mod utils;

use std::{
  sync::{
    mpsc::{self, TryRecvError},
    Arc, Mutex,
  },
  thread::{sleep, spawn},
  time::Duration,
};

use esp_idf_svc::{log::EspLogger, sys::*};
use log::{error, info};

use crate::{
  hidd::{init_hid_device, notify_gap_auth_success, HidDevice, HidDeviceHandler},
  hidh::{init_hid_host, open_hid_device, HidHostHandler},
  scan::{notify_discovery_finished, notify_discovery_result, scan_bluetooth},
  utils::{
    ble_gap_event_name, ble_key_type_name, bt_controller_config_default, bt_gap_event_name, initialize_nvs, BdAddr,
    KEYBOARD_REPORT_MAP,
  },
};

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

    esp_nofail!(esp_bt_gap_register_callback(Some(bt_gap_callback)));
    esp_nofail!(esp_ble_gap_register_callback(Some(ble_gap_callback)));
  }

  let (input_tx, input_rx) = mpsc::channel();

  init_hid_device("Keyboard Bridge", "o137", "0137", &KEYBOARD_REPORT_MAP, |device| {
    TypingTask::new(device, input_rx)
  })
  .unwrap();
  init_hid_host(ReceiveTask::new(input_tx)).unwrap();
}

struct TypingTask {
  resume: mpsc::Sender<()>,
  pause: mpsc::Sender<()>,
}
impl TypingTask {
  fn new(device: HidDevice, input_rx: mpsc::Receiver<Vec<u8>>) -> Self {
    let (resume_tx, resume_rx) = mpsc::channel();
    let (pause_tx, pause_rx) = mpsc::channel();

    let task = move || {
      let mut input = input_rx.recv().unwrap();

      let _ = device
        .send_input(0, 1, &mut input)
        .inspect_err(|e| error!("failed to send key press: {:?}", e));
    };

    spawn(move || loop {
      match resume_rx.recv() {
        Ok(_) => {
          info!("typing: resume");
        }
        Err(_) => {
          info!("typing: exit");
          return;
        }
      }
      loop {
        match pause_rx.try_recv() {
          Err(TryRecvError::Empty) => {}
          Ok(_) => {
            info!("typing: pause");
            break;
          }
          Err(TryRecvError::Disconnected) => {
            info!("typing: exit");
            return;
          }
        }
        task();
      }
    });

    Self {
      resume: resume_tx,
      pause: pause_tx,
    }
  }
}
impl HidDeviceHandler for TypingTask {
  fn on_resume(&self) {
    self.resume.send(()).unwrap();
  }
  fn on_pause(&self) {
    self.pause.send(()).unwrap();
  }
}

#[derive(Clone)]
struct ReceiveTask {
  scanning: Arc<Mutex<bool>>,
  input_tx: mpsc::Sender<Vec<u8>>,
}
impl ReceiveTask {
  fn new(input_tx: mpsc::Sender<Vec<u8>>) -> Self {
    let this = Self {
      scanning: Arc::new(Mutex::new(true)),
      input_tx,
    };

    spawn({
      let this = this.clone();
      move || loop {
        if !this.is_scanning() {
          sleep(Duration::from_secs(1));
          continue;
        }

        let devices = scan_bluetooth(Duration::from_secs(5));
        if !this.is_scanning() {
          continue;
        }

        let mut keyboard = None;
        for device in devices.iter().filter(|d| d.is_keyboard()) {
          if device.is_keyboard() {
            info!("found keyboard: {:?}", device);
            keyboard = Some(device);
          } else {
            info!("found device: {:?}", device);
          }
        }

        if let Some(keyboard) = keyboard {
          info!("connecting to keyboard: {}", keyboard.bda);
          if let Err(e) = open_hid_device(keyboard.bda) {
            error!("failed to open hid device: {:?}", e);
          }
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
}
impl HidHostHandler for ReceiveTask {
  fn on_open(&self, _addr: BdAddr) {
    self.set_scanning(false);
  }
  fn on_open_failed(&self, _error: EspError) {
    self.set_scanning(true);
  }
  fn on_close(&self, _addr: BdAddr) {
    self.set_scanning(true);
  }
  fn on_input(&self, _addr: BdAddr, usage: hidh::HidUsage, _map_index: u8, _report_id: u16, data: &[u8]) {
    if usage.is_keyboard() {
      self.input_tx.send(data.to_vec()).unwrap();
    }
  }
}

extern "C" fn ble_gap_callback(event: esp_gap_ble_cb_event_t, param: *mut esp_ble_gap_cb_param_t) {
  let param = unsafe { &*param };
  match event {
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
        notify_gap_auth_success();
      } else {
        info!("ble-gap: auth failed: {}", auth_cmpl.fail_reason);
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
        notify_discovery_finished();
      }
    }
    esp_bt_gap_cb_event_t_ESP_BT_GAP_DISC_RES_EVT => {
      let disc_res = unsafe { param.disc_res };
      notify_discovery_result(disc_res);
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
