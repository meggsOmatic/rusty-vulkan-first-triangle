


use anyhow::{Context, Result};
use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::{Swapchain, TimelineSemaphore};
use safe_transmute::guard::AllOrNothingGuard;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::windows::WindowExtWindows;
use winit::window::{Window, WindowBuilder};

use ash::prelude::*;
use ash::util::*;
use ash::vk::{self, SurfaceKHR, SwapchainKHR};
use safe_transmute::*;
use std::cmp;
use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet};
use std::default::Default;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::mem;
use std::mem::align_of;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;


pub fn safer_cstr(chars: &[std::os::raw::c_char]) -> Option<&CStr> {
    if chars.contains(&0) && chars[0] != 0 {
        Some(unsafe { CStr::from_ptr(&chars[0]) })
    } else {
        None
    }
}