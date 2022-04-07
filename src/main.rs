#![allow(
    dead_code,
    unused_variables,
//    clippy::too_many_arguments,
//    clippy::unnecessary_wraps
)]

use anyhow::Result;
use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::Swapchain;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::windows::WindowExtWindows;
use winit::window::{Window, WindowBuilder};

use ash::util::*;
use ash::prelude::*;
use ash::vk::{self, SwapchainKHR};
use std::cmp;
use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet};
use std::default::Default;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::mem;
use std::mem::align_of;
use std::sync::Arc;

fn safer_cstr(chars: &[std::os::raw::c_char]) -> Option<&CStr> {
    if chars.contains(&0) && chars[0] != 0 {
        Some(unsafe { CStr::from_ptr(&chars[0]) })
    } else {
        None
    }
}

fn main() -> Result<()> {
    pretty_env_logger::init();

    // Window

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("VK_RUSTY_TRIANGLE")
        .with_inner_size(LogicalSize::new(1024, 768))
        .build(&event_loop)?;

    // App

    let mut app = unsafe { App::create(&window)? };
    let mut destroying = false;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            // Render a frame if our Vulkan app is not being destroyed.
            Event::MainEventsCleared if !destroying => unsafe { app.render(&window) }.unwrap(),
            // Destroy our Vulkan app.
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                destroying = true;
                *control_flow = ControlFlow::Exit;
                unsafe {
                    app.destroy();
                }
            }
            _ => {}
        }
    });
}

/// Our Vulkan app.
//#[derive(Clone)]
struct App {
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::extensions::khr::Surface,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
    swapchain_loader: ash::extensions::khr::Swapchain,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_views: Vec<vk::ImageView>
}

impl App {
    /// Creates our Vulkan app.
    unsafe fn create(window: &Window) -> Result<Self> {
        let entry = unsafe { ash::Entry::load()? };
        let version = match entry.try_enumerate_instance_version() {
            Ok(Some(version)) => version,
            Ok(None) => vk::make_api_version(0, 1, 0, 0),
            Err(_) => panic!("No API version"),
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

        if let Ok(win_ex) = ash_window::enumerate_required_extensions(&window) {
            instance_extensions.extend(
                win_ex
                    .iter()
                    .map(|s| CString::from(unsafe { CStr::from_ptr(*s) })),
            );
        }

        if true {
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
            .unwrap();

        let surface = ash_window::create_surface(&entry, &instance, &window, None)?;

        let surface_loader = ash::extensions::khr::Surface::new(&entry, &instance);

        // let required_device_extensions = [

        // ];

        let required_device_extensions = [(vk::KhrSwapchainFn::name(), 0u32)];

        let required_device_extensions_raw = required_device_extensions
            .iter()
            .map(|(name, version)| name.as_ptr())
            .collect::<Vec<*const i8>>();

        let (physical_device, graphics_queue_family, present_queue_family, swap_capabilities, swap_formats, present_modes) = instance
            .enumerate_physical_devices()
            .unwrap()
            .iter()
            .find_map(|&dev| {
                let props = instance.get_physical_device_properties(dev);
                let features = instance.get_physical_device_features(dev);
                let queues = instance.get_physical_device_queue_family_properties(dev);

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

                let present_queue_family_index = (graphics_queue_family_index..=graphics_queue_family_index).chain(0..queues.len() as u32).find(|&i| {
                    surface_loader.get_physical_device_surface_support(dev, i, surface) == Ok(true)
                })?;

                let raw_extensions = match instance.enumerate_device_extension_properties(dev) {
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

                let (capabilities, formats, present_modes) = (
                    surface_loader
                        .get_physical_device_surface_capabilities(dev, surface)
                        .ok()?,
                    surface_loader
                        .get_physical_device_surface_formats(dev, surface)
                        .ok()?,
                    surface_loader
                        .get_physical_device_surface_present_modes(dev, surface)
                        .ok()?,
                );

                dbg!(&capabilities);
                dbg!(&formats);
                dbg!(&present_modes);

                if formats.is_empty() || present_modes.is_empty() {
                    return None;
                }

                Some((dev, graphics_queue_family_index, present_queue_family_index, capabilities, formats, present_modes))
            })
            .unwrap();

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

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_infos)
            .enabled_features(&needed_features)
            .enabled_layer_names(&layers_raw)
            .enabled_extension_names(&required_device_extensions_raw); // .enabled_extension_names(&extensions_raw); -- all of these are layer-level not device-level

        let device = instance.create_device(physical_device, &device_info, None)?;

        let graphics_queue = device.get_device_queue(graphics_queue_family, 0);

        let present_queue = device.get_device_queue(present_queue_family, 0);

        let window_size = window.inner_size();
        let swap_size = (
            cmp::min(
                cmp::max(window_size.width, swap_capabilities.min_image_extent.width),
                swap_capabilities.max_image_extent.width,
            ),
            cmp::min(
                cmp::max(window_size.height, swap_capabilities.min_image_extent.height),
                swap_capabilities.max_image_extent.height,
            ),
        );

        let format = (|| {
            let preference = [
                vk::SurfaceFormatKHR { format: vk::Format::B8G8R8A8_SRGB, color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR }
            ];
    
            for pref in preference {
                if swap_formats.contains(&pref) {
                    return pref;
                }
            }
            swap_formats[0]
        })();
        
        let present = (|| {
            let preference = [
                vk::PresentModeKHR::FIFO_RELAXED,
                vk::PresentModeKHR::MAILBOX,
                vk::PresentModeKHR::FIFO,
                vk::PresentModeKHR::IMMEDIATE
            ];
    
            for pref in preference {
                if present_modes.contains(&pref) {
                    return pref;
                }
            }
            present_modes[0]
        })();

        let swapchain_count = swap_capabilities.min_image_count;
        let swapchain_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(swapchain_count)
            .image_color_space(format.color_space)
            .image_format(format.format)
            .image_extent(vk::Extent2D { width : swap_size.0, height : swap_size.1 })
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .pre_transform(swap_capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present)
            .clipped(true)
            .old_swapchain(SwapchainKHR::default());
        
        let shared_queues = [present_queue_family, graphics_queue_family];
        let swapchain_info = if present_queue_family != graphics_queue_family {
            swapchain_info
                .image_sharing_mode(vk::SharingMode::CONCURRENT)
                .queue_family_indices(&shared_queues)
        } else {
            swapchain_info
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        };

        let swapchain_loader = Swapchain::new(&instance, &device);
        let swapchain = swapchain_loader.create_swapchain(&swapchain_info, None)?;
        let swapchain_images = swapchain_loader.get_swapchain_images(swapchain)?;
        let swapchain_views = swapchain_images
            .iter()
            .map(|&image| {
                let subresource_info = vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1);

                let view_info = vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(subresource_info);
                
                device.create_image_view(&view_info, None)
            }).collect::<VkResult<Vec<vk::ImageView>>>()?;

        Ok(Self {
            entry,
            instance,
            surface_loader,
            surface,
            physical_device,
            device,
            graphics_queue,
            present_queue,
            swapchain_loader,
            swapchain,
            swapchain_images,
            swapchain_views
        })
    }

    /// Renders a frame for our Vulkan app.
    unsafe fn render(&mut self, window: &Window) -> Result<()> {
        Ok(())
    }

    /// Destroys our Vulkan app.
    unsafe fn destroy(&mut self) {
        for &view in self.swapchain_views.iter() {
            self.device.destroy_image_view(view, None);
        }
        self.swapchain_loader.destroy_swapchain(self.swapchain, None);
        self.device.destroy_device(None);
        self.surface_loader.destroy_surface(self.surface, None);
        self.instance.destroy_instance(None);
    }
}

/// The Vulkan handles and associated properties used by our Vulkan app.
#[derive(Clone, Debug, Default)]
struct AppData {}
