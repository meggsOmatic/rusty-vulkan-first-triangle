#![allow(
    dead_code,
    unused_variables,
//    clippy::too_many_arguments,
//    clippy::unnecessary_wraps
)]

use anyhow::Result;
use ash::extensions::ext::DebugUtils;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowBuilder};

use ash::util::*;
use ash::vk;
use std::default::Default;
use std::ffi::CStr;
use std::ffi::CString;
use std::io::Cursor;
use std::mem;
use std::mem::align_of;
use std::sync::Arc;

fn safer_cstr(chars : &[std::os::raw::c_char]) -> Option<&CStr> {
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
        .with_title("Vulkan Tutorial (Rust)")
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
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    graphics_queue: vk::Queue
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
                    if let Ok(ext_props) = entry.enumerate_instance_extension_properties(Some(prop)) {
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
    
        let mut extensions = Vec::<CString>::new();
        let mut layers = Vec::<CString>::new();
    
        if let Ok(win_ex) = ash_window::enumerate_required_extensions(&window) {
            extensions.extend(win_ex.iter().map(|s| CString::from(unsafe { CStr::from_ptr(*s) })));
        }
    
        if true {
            layers.push(CString::new("VK_LAYER_KHRONOS_validation").unwrap());
            extensions.push(CString::from(DebugUtils::name()));
        }
    
        println!("Extensions: {:?}", extensions);

        let extensions_raw: Vec<*const i8> = extensions.iter().map(|c| c.as_ptr()).collect();
        let layers_raw: Vec<*const i8> = layers.iter().map(|c| c.as_ptr()).collect();
    
        let instance = entry.create_instance(
            &vk::InstanceCreateInfo::default()
                .application_info(&app_info)
                .enabled_layer_names(&layers_raw)
                .enabled_extension_names(&extensions_raw), 
            None)
            .unwrap();
        
        let (physical_device, graphics_queue_family) = instance.enumerate_physical_devices().unwrap().iter().find_map(|&dev| {
           let props = instance.get_physical_device_properties(dev);
           let features = instance.get_physical_device_features(dev);
           let queues = instance.get_physical_device_queue_family_properties(dev);

           dbg!(&props);
           dbg!(&features);
           dbg!(&queues);

           if props.device_type != vk::PhysicalDeviceType::DISCRETE_GPU {
               return None;
           }

           let queue_family_index = queues.iter().position(|&q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))?;

           Some((dev, queue_family_index as u32))
        }).unwrap();

        let queue_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_queue_family as u32)
            .queue_priorities(&[1.0])];

        let needed_features = vk::PhysicalDeviceFeatures::default();

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_infos)
            .enabled_features(&needed_features)
            .enabled_layer_names(&layers_raw)
            ;// .enabled_extension_names(&extensions_raw); -- all of these are layer-level not device-level

        let device = instance.create_device(physical_device, &device_info, None)?;

        let queue = device.get_device_queue(graphics_queue_family, 0);

        Ok(Self { entry, instance, physical_device, device, graphics_queue: queue })
    }

    /// Renders a frame for our Vulkan app.
    unsafe fn render(&mut self, window: &Window) -> Result<()> {
        Ok(())
    }

    /// Destroys our Vulkan app.
    unsafe fn destroy(&mut self) {
        self.device.destroy_device(None);
        self.instance.destroy_instance(None);
    }
}

/// The Vulkan handles and associated properties used by our Vulkan app.
#[derive(Clone, Debug, Default)]
struct AppData {}
