use crate::util::*;
use crate::config::*;
use crate::loaders::*;

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


pub struct Device {
    pub loaders: Rc<Loaders>,
    pub device: ash::Device,
    pub physical_device: vk::PhysicalDevice,
    pub graphics_queue_family: u32,
    pub graphics_queue: vk::Queue,
    pub present_queue_family: u32,
    pub present_queue: vk::Queue,
    pub command_pool: vk::CommandPool,
    pub swapchain_loader: ash::extensions::khr::Swapchain,
}

impl Device {
    pub unsafe fn create(loaders: Rc<Loaders>, surface: SurfaceKHR) -> anyhow::Result<Device> {
        let required_device_extensions = [(vk::KhrSwapchainFn::name(), 0u32)];

        let required_device_extensions_raw = required_device_extensions
            .iter()
            .map(|(name, version)| name.as_ptr())
            .collect::<Vec<*const i8>>();

        let (physical_device, graphics_queue_family, present_queue_family) = loaders
            .instance
            .enumerate_physical_devices()
            .unwrap()
            .iter()
            .find_map(|&dev| {
                let props = loaders.instance.get_physical_device_properties(dev);
                let features = loaders.instance.get_physical_device_features(dev);
                let queues = loaders
                    .instance
                    .get_physical_device_queue_family_properties(dev);

                dbg!(&props);
                dbg!(&features);
                dbg!(&queues);

                if props.device_type != vk::PhysicalDeviceType::DISCRETE_GPU {
                    return None;
                }

                let graphics_queue_family_index = queues
                    .iter()
                    .position(|&q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))?
                    as u32;

                let present_queue_family_index = (graphics_queue_family_index
                    ..=graphics_queue_family_index)
                    .chain(0..queues.len() as u32)
                    .find(|&i| {
                        loaders
                            .surface
                            .get_physical_device_surface_support(dev, i, surface)
                            == Ok(true)
                    })?;

                let raw_extensions =
                    match loaders.instance.enumerate_device_extension_properties(dev) {
                        Ok(exts) => exts,
                        Err(e) => {
                            return None;
                        }
                    };

                let avail_extensions: BTreeMap<&CStr, u32> =
                    BTreeMap::from_iter(raw_extensions.iter().filter_map(|props| {
                        Some((safer_cstr(&props.extension_name)?, props.spec_version))
                    }));

                dbg!(&avail_extensions);

                for req in required_device_extensions {
                    if let Some(&version) = avail_extensions.get(req.0) {
                        if version < req.1 {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }

                let capabilities = loaders
                    .surface
                    .get_physical_device_surface_capabilities(dev, surface)
                    .ok()?;

                let formats = loaders
                    .surface
                    .get_physical_device_surface_formats(dev, surface)
                    .ok()?;

                let present_modes = loaders
                    .surface
                    .get_physical_device_surface_present_modes(dev, surface)
                    .ok()?;

                dbg!(&capabilities);
                dbg!(&formats);
                dbg!(&present_modes);

                if formats.is_empty() || present_modes.is_empty() {
                    return None;
                }

                Some((dev, graphics_queue_family_index, present_queue_family_index))
            })
            .ok_or(vk::Result::ERROR_DEVICE_LOST)
            .context("Could not find any suitable physical device")?;

        let queue_infos: Vec<vk::DeviceQueueCreateInfo> =
            HashSet::from([graphics_queue_family, present_queue_family])
                .iter()
                .map(|&family_index| {
                    vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(family_index)
                        .queue_priorities(&[1.0])
                })
                .collect();

        let needed_features = vk::PhysicalDeviceFeatures::default();
        let mut layers = Vec::<CString>::new();
        if VK_DEBUG_LAYER {
            layers.push(CString::new("VK_LAYER_KHRONOS_validation").unwrap());
        }
        let layers_raw: Vec<*const i8> = layers.iter().map(|c| c.as_ptr()).collect();

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_infos)
            .enabled_features(&needed_features)
            .enabled_layer_names(&layers_raw)
            .enabled_extension_names(&required_device_extensions_raw); // .enabled_extension_names(&extensions_raw); -- all of these are layer-level not device-level

        let device = loaders
            .instance
            .create_device(physical_device, &device_info, None)
            .context("Could not create logical device")?;

        let graphics_queue = device.get_device_queue(graphics_queue_family, 0);

        let present_queue = device.get_device_queue(present_queue_family, 0);

        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(graphics_queue_family),
                None,
            )
            .context("Could not create command pool for device")?;

        let swapchain_loader = Swapchain::new(&loaders.instance, &device);

        Ok(Device {
            loaders,
            device,
            physical_device,
            graphics_queue_family,
            graphics_queue,
            present_queue_family,
            present_queue,
            command_pool,
            swapchain_loader,
        })
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
        }
    }
}
