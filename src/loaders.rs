
use crate::util::*;
use crate::config::*;
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

pub struct Loaders {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub surface: ash::extensions::khr::Surface,
}

impl Loaders {
    pub unsafe fn new(window: &Window) -> anyhow::Result<Loaders> {
        let entry = ash::Entry::load()?;
        let version = match entry.try_enumerate_instance_version() {
            Ok(Some(version)) => version,
            Ok(None) => vk::make_api_version(0, 1, 0, 0),
            Err(e) => {
                return Err(e).context("Could not get Vulkan version.");
            }
        };
        println!(
            "{} - Vulkan Instance {}.{}.{}",
            "Vulkan Tutorial (Rust)",
            vk::api_version_major(version),
            vk::api_version_minor(version),
            vk::api_version_patch(version)
        );

        if let Ok(ext_props) = entry.enumerate_instance_extension_properties(None) {
            for prop in ext_props.iter().enumerate() {
                println!("base extension prop {} : {:?}", prop.0, prop.1);
            }
        } else {
            println!("No extension props");
        }

        if let Ok(layer_props) = entry.enumerate_instance_layer_properties() {
            for prop in layer_props.iter().enumerate() {
                println!("layer prop {} : {:?}", prop.0, prop.1);
                if let Some(prop) = safer_cstr(&prop.1.layer_name) {
                    if let Ok(ext_props) = entry.enumerate_instance_extension_properties(Some(prop))
                    {
                        for prop in ext_props.iter().enumerate() {
                            println!("base extension prop {} : {:?}", prop.0, prop.1);
                        }
                    } else {
                        println!("No extension props");
                    }
                }
            }
        } else {
            println!("No layer props");
        }

        let app_name = &CString::new("Triangle 1")?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .application_version(0)
            .engine_name(app_name)
            .engine_version(0)
            .api_version(vk::make_api_version(0, 1, 3, 0));

        // Extensions

        let mut instance_extensions = Vec::<CString>::new();
        let mut layers = Vec::<CString>::new();

        if let Ok(win_ex) = ash_window::enumerate_required_extensions(window) {
            instance_extensions.extend(
                win_ex
                    .iter()
                    .map(|s| CString::from(unsafe { CStr::from_ptr(*s) })),
            );
        }

        if VK_DEBUG_LAYER {
            layers.push(CString::new("VK_LAYER_KHRONOS_validation").unwrap());
            instance_extensions.push(CString::from(DebugUtils::name()));
        }

        println!("Instance Extensions: {:?}", instance_extensions);

        let instance_extensions_raw: Vec<*const i8> =
            instance_extensions.iter().map(|c| c.as_ptr()).collect();
        let layers_raw: Vec<*const i8> = layers.iter().map(|c| c.as_ptr()).collect();

        let instance = entry
            .create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(&app_info)
                    .enabled_layer_names(&layers_raw)
                    .enabled_extension_names(&instance_extensions_raw),
                None,
            )
            .with_context(|| {
                format!(
                    "Could not create Vulkan instance. Version {:#x} extensions {:?}, layers {:?}",
                    version, instance_extensions, layers
                )
            })?;

        let surface = ash::extensions::khr::Surface::new(&entry, &instance);

        Ok(Loaders {
            entry,
            instance,
            surface,
        })
    }
}

impl Drop for Loaders {
    fn drop(&mut self) {
        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}
