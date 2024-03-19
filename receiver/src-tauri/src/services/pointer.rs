use std::{collections::HashMap, sync::Arc};

use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize};
use tauri::{
  async_runtime::Mutex,
  plugin::{Builder, TauriPlugin},
  Manager, Runtime, State,
};
use windows::Win32::{
  Foundation::{HANDLE, HWND, POINT, RECT},
  UI::{
    Controls::{
      CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
      POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
    },
    Input::Pointer::{
      InjectSyntheticPointerInput, POINTER_CHANGE_NONE, POINTER_FLAG_CANCELED, POINTER_FLAG_DOWN,
      POINTER_FLAG_INCONTACT, POINTER_FLAG_INRANGE, POINTER_FLAG_PRIMARY, POINTER_FLAG_UP,
      POINTER_FLAG_UPDATE, POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
    },
    WindowsAndMessaging::{
      PEN_FLAG_NONE, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y,
      PT_MOUSE, PT_PEN, PT_TOUCH, TOUCH_FLAG_NONE, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE,
    },
  },
};

const MAX_CONTACTS: usize = 10;

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
enum PointerEventType {
  #[serde(rename = "down")]
  Down,
  #[serde(rename = "move")]
  Move,
  #[serde(rename = "up")]
  Up,
  #[serde(rename = "cancel")]
  Cancel,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
enum PointerType {
  #[serde(rename = "mouse")]
  Mouse,
  #[serde(rename = "pen")]
  Pen,
  #[serde(rename = "touch")]
  Touch,
}

bitflags! {
  #[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
  struct Button: u8 {
      const NONE = 0b0000_0000;
      const PRIMARY = 0b0000_0001;
      const SECONDARY = 0b0000_0010;
      const AUXILARY = 0b0000_0100;
      const FOURTH = 0b0000_1000;
      const FIFTH = 0b0001_0000;
      const ERASER = 0b0010_0000;
  }
}

fn button_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Button, D::Error> {
  let bits: u8 = Deserialize::deserialize(deserializer)?;
  Button::from_bits(bits).ok_or(serde::de::Error::custom("invalid button bits"))
}

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
struct PointerEvent {
  #[serde(rename = "eventType")]
  event_type: PointerEventType,
  #[serde(rename = "pointerId")]
  id: u32,
  #[serde(rename = "pointerType")]
  pointer_type: PointerType,
  #[serde(rename = "isPrimary")]
  is_primary: bool,
  #[serde(rename = "normalizedX")]
  x: f64,
  #[serde(rename = "normalizedY")]
  y: f64,
  #[serde(deserialize_with = "button_from")]
  button: Button,
  #[serde(deserialize_with = "button_from")]
  buttons: Button,
  width: f64,
  height: f64,
  pressure: f64,
  #[serde(rename = "tiltX")]
  tilt_x: i32,
  #[serde(rename = "tiltY")]
  tilt_y: i32,
  twist: u32,
}

impl From<PointerEvent> for POINTER_TYPE_INFO {
  fn from(event: PointerEvent) -> Self {
    let mut pointer_flags = match event.event_type {
      PointerEventType::Down => POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN,
      PointerEventType::Move => POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_UPDATE,
      PointerEventType::Up => POINTER_FLAG_UP,
      PointerEventType::Cancel => {
        POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE | POINTER_FLAG_CANCELED
      }
    };

    let device_type = match event.pointer_type {
      PointerType::Mouse => PT_MOUSE,
      PointerType::Pen => PT_PEN,
      PointerType::Touch => PT_TOUCH,
    };

    // TODO: ボタンを押したりできるペンを使うときに使いそう
    // BUTTON_DOWN だけじゃなくて BUTTON_UP もある
    let button_change_type = POINTER_CHANGE_NONE;
    // let button_change_type = match val.button {
    //   Button::PRIMARY => POINTER_CHANGE_FIRSTBUTTON_DOWN,
    //   Button::SECONDARY => POINTER_CHANGE_SECONDBUTTON_DOWN,
    //   Button::AUXILARY => POINTER_CHANGE_THIRDBUTTON_DOWN,
    //   Button::FOURTH => POINTER_CHANGE_FOURTHBUTTON_DOWN,
    //   Button::FIFTH => POINTER_CHANGE_FIFTHBUTTON_DOWN,
    //   Button::NONE => POINTER_CHANGE_NONE,
    //   _ => POINTER_CHANGE_NONE,
    // };

    if event.is_primary {
      pointer_flags |= POINTER_FLAG_PRIMARY;
    }

    let x = event.x * 1920.0;
    let y = event.y * 1080.0;

    let pressure = (event.pressure * 1024.0) as u32;

    let pointer_info = POINTER_INFO {
      pointerType: device_type,
      pointerId: event.id,
      frameId: 0,
      pointerFlags: pointer_flags,
      sourceDevice: HANDLE::default(),
      hwndTarget: HWND::default(),
      ptPixelLocation: POINT {
        x: x as i32,
        y: y as i32,
      },
      ptHimetricLocation: POINT::default(),
      ptPixelLocationRaw: POINT {
        x: x as i32,
        y: y as i32,
      },
      ptHimetricLocationRaw: POINT::default(),
      dwTime: 0,
      historyCount: 1,
      InputData: 0,
      dwKeyStates: 0,
      PerformanceCount: 0,
      ButtonChangeType: button_change_type,
    };

    let union_arg = if device_type == PT_TOUCH {
      let width_half = event.width / 2.0;
      let height_half = event.height / 2.0;
      let contact_area = RECT {
        left: (x - width_half) as i32,
        top: (y - height_half) as i32,
        right: (x + width_half) as i32,
        bottom: (y + height_half) as i32,
      };

      POINTER_TYPE_INFO_0 {
        touchInfo: POINTER_TOUCH_INFO {
          pointerInfo: pointer_info,
          touchFlags: TOUCH_FLAG_NONE,
          touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
          rcContact: contact_area,
          rcContactRaw: RECT::default(),
          orientation: 0,
          pressure,
        },
      }
    } else {
      POINTER_TYPE_INFO_0 {
        penInfo: POINTER_PEN_INFO {
          pointerInfo: pointer_info,
          penFlags: PEN_FLAG_NONE,
          penMask: PEN_MASK_PRESSURE | PEN_MASK_ROTATION | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
          pressure,
          rotation: event.twist,
          tiltX: event.tilt_x,
          tiltY: event.tilt_y,
        },
      }
    };

    POINTER_TYPE_INFO {
      r#type: device_type,
      Anonymous: union_arg,
    }
  }
}

struct PointerDevices {
  touch: HSYNTHETICPOINTERDEVICE,
  pen: HSYNTHETICPOINTERDEVICE,
  touches: HashMap<u32, POINTER_TYPE_INFO>,
}

impl Drop for PointerDevices {
  fn drop(&mut self) {
    unsafe {
      DestroySyntheticPointerDevice(self.touch);
      DestroySyntheticPointerDevice(self.pen);
    }
  }
}

impl PointerDevices {
  fn new() -> windows::core::Result<Self> {
    let touch = unsafe {
      CreateSyntheticPointerDevice(PT_TOUCH, MAX_CONTACTS as u32, POINTER_FEEDBACK_NONE)?
    };

    let pen = unsafe { CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_NONE)? };

    Ok(PointerDevices {
      touch,
      pen,
      touches: HashMap::new(),
    })
  }

  fn inject(&mut self, event: PointerEvent) -> windows::core::Result<()> {
    let info: POINTER_TYPE_INFO = event.into();

    if info.r#type == PT_TOUCH {
      self.touches.insert(event.id, info);
      let result = unsafe {
        InjectSyntheticPointerInput(
          self.touch,
          self
            .touches
            .values()
            .copied()
            .collect::<Vec<POINTER_TYPE_INFO>>()
            .as_slice(),
        )
      };
      match event.event_type {
        PointerEventType::Up | PointerEventType::Cancel => {
          self.touches.remove(&event.id);
        }
        _ => {}
      }
      result?;
    } else if info.r#type == PT_PEN {
      unsafe { InjectSyntheticPointerInput(self.pen, &[info])? };
    } else {
      panic!("invalid pointer type");
    }
    Ok(())
  }
}

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command

#[tauri::command]
async fn reset(state: State<'_, Arc<Mutex<PointerDevices>>>) -> Result<(), String> {
  let mut state = state.lock().await;
  state.touches.clear();
  Ok(())
}

#[tauri::command]
async fn inject(
  event: PointerEvent,
  state: State<'_, Arc<Mutex<PointerDevices>>>,
) -> Result<(), String> {
  let mut state = state.lock().await;
  state.inject(event).map_err(|e| format!("{:?}", e))
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
  Builder::new("pointer")
    .invoke_handler(tauri::generate_handler![reset, inject])
    .setup(|app| {
      let devices = PointerDevices::new().expect("failed to create pointer devices");
      app.manage(Arc::new(Mutex::new(devices)));
      Ok(())
    })
    .build()
}
