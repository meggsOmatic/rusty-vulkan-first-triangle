use crate::util::*;
use crate::config::*;
use crate::loaders::*;
use crate::device::*;
use crate::swapsurface::*;
use crate::perframe::*;

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



pub struct VulkanWindow {
    pub window: Window,
    pub surface: Rc<Surface>,
    pub device: Rc<Device>,
    pub swap: PerSwapchain,
    pub per_frame: Vec<PerFrame>,

    pub frame_count: usize,
    pub count_start_time: std::time::Instant,
    pub count_start_frame: usize,
}

impl Drop for VulkanWindow {
    fn drop(&mut self) {
    }
}

