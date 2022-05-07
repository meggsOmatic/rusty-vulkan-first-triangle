#![allow(
    dead_code,
    unused_variables,
//    clippy::too_many_arguments,
//    clippy::unnecessary_wraps
)]

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

static VERTEX_BYTECODE: &'static [u8] = include_bytes!("./vert.spv");
static FRAGMENT_BYTECODE: &'static [u8] = include_bytes!("./frag.spv");

const VK_DEBUG_LAYER: bool = true;
const VK_DYNAMIC_VIEW_SIZE: bool = true;

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
    // App

    let mut app = unsafe { App::create(&event_loop)? };
    let mut destroying = false;
    event_loop.run(move |event, el_window_target, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            // Render a frame if our Vulkan app is not being destroyed.
            Event::MainEventsCleared if !destroying => unsafe {
                for w in app.windows.values_mut() {
                    match app.renderer.render(w) {
                        Ok(_) => {}
                        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                            println!("Out of date");
                            let dev = &app.renderer.device.device;
                            let _ = dev.device_wait_idle();
                            w.swap = PerSwapchain::new(
                                app.renderer.device.clone(),
                                &w.window,
                                w.surface.clone(),
                                Some(&app.renderer),
                                Some(&w.swap),
                            )
                            .context("Recreating swapchain")
                            .unwrap();
                        }
                        Err(e) => {
                            panic!("Unexpected Vulkan error {} while rendering", e);
                        }
                    }
                }
            },
            // Destroy our Vulkan app.
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } => {
                if app.windows.remove(&window_id).is_none() {
                    println!("Could not find window {:?} to remove.", window_id);
                }

                if app.windows.is_empty() {
                    destroying = true;
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state: winit::event::ElementState::Pressed,
                                virtual_keycode: Some(key),
                                ..
                            },
                        ..
                    },
                window_id,
            } => match key {
                VirtualKeyCode::N => unsafe {
                    let window = WindowBuilder::new()
                        .with_title("VK_RUSTY_TRIANGLE")
                        .with_inner_size(LogicalSize::new(1024, 768))
                        .build(&el_window_target)
                        .context("Could not create window.")
                        .unwrap();

                    let loaders = &app.renderer.device.loaders;
                    let surface = Rc::new(Surface {
                        loaders: app.renderer.device.loaders.clone(),
                        surface: ash_window::create_surface(
                            &loaders.entry,
                            &loaders.instance,
                            &window,
                            None,
                        )
                        .context("Could not create surface from window handle")
                        .unwrap(),
                    });

                    let swap = PerSwapchain::new(
                        app.renderer.device.clone(),
                        &window,
                        surface.clone(),
                        Some(&app.renderer),
                        None,
                    )
                    .context("Could not create additional swapchain")
                    .unwrap();

                    let per_frame: Vec<PerFrame> = (0..4)
                        .map(|_| PerFrame::new(app.renderer.device.clone()))
                        .collect::<VkResult<Vec<PerFrame>>>()
                        .context("Could not create per-frame queues")
                        .unwrap();

                    let v_win = VulkanWindow {
                        window,
                        surface,
                        device: app.renderer.device.clone(),
                        swap,
                        per_frame,

                        frame_count: 0,
                        count_start_time: std::time::Instant::now(),
                        count_start_frame: 0,
                    };

                    app.windows.insert(v_win.window.id(), v_win);
                },
                _ => {}
            },
            _ => {}
        }
    });
}

struct Loaders {
    entry: ash::Entry,
    instance: ash::Instance,
    surface: ash::extensions::khr::Surface,
}

impl Loaders {
    unsafe fn new(window: &Window) -> anyhow::Result<Loaders> {
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

struct Device {
    loaders: Rc<Loaders>,
    device: ash::Device,
    physical_device: vk::PhysicalDevice,
    graphics_queue_family: u32,
    graphics_queue: vk::Queue,
    present_queue_family: u32,
    present_queue: vk::Queue,
    command_pool: vk::CommandPool,
    swapchain_loader: ash::extensions::khr::Swapchain,
}

impl Device {
    unsafe fn create(loaders: Rc<Loaders>, surface: SurfaceKHR) -> anyhow::Result<Device> {
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

struct PerFrame {
    device: Rc<Device>,
    command_buffer: vk::CommandBuffer,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    in_flight_fence: vk::Fence,
}

impl PerFrame {
    fn new(device: Rc<Device>) -> VkResult<PerFrame> {
        unsafe {
            let command_buffer = device.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(device.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            );

            let image_available_semaphore = match command_buffer {
                Ok(_) => device
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None),
                Err(e) => Err(e),
            };

            let render_finished_semaphore = image_available_semaphore.and_then(|_| {
                device
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            });

            let in_flight_fence = render_finished_semaphore.and_then(|_| {
                device.device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
            });

            if in_flight_fence.is_ok() {
                return Ok(PerFrame {
                    device,
                    command_buffer: command_buffer.unwrap()[0],
                    image_available_semaphore: image_available_semaphore.unwrap(),
                    render_finished_semaphore: render_finished_semaphore.unwrap(),
                    in_flight_fence: in_flight_fence.unwrap(),
                });
            }

            if let Ok(f) = in_flight_fence {
                device.device.destroy_fence(f, None);
            }

            if let Ok(s) = render_finished_semaphore {
                device.device.destroy_semaphore(s, None);
            }

            if let Ok(s) = image_available_semaphore {
                device.device.destroy_semaphore(s, None);
            }

            if let Ok(c) = command_buffer {
                device.device.free_command_buffers(device.command_pool, &c);
            }

            Err(in_flight_fence.unwrap_err())
        }
    }
}

impl Drop for PerFrame {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .device
                .device
                .wait_for_fences(&[self.in_flight_fence], true, 100_000_000);
            self.device
                .device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device
                .device
                .destroy_semaphore(self.render_finished_semaphore, None);
            self.device
                .device
                .free_command_buffers(self.device.command_pool, &[self.command_buffer]);
            self.device.device.destroy_fence(self.in_flight_fence, None);
        }
    }
}

struct Surface {
    loaders: Rc<Loaders>,
    surface: vk::SurfaceKHR,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            self.loaders.surface.destroy_surface(self.surface, None);
        }
    }
}

struct PerSwapchain {
    device: Rc<Device>,
    surface: Rc<Surface>,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    framebuffers: Vec<vk::Framebuffer>,
    size: vk::Extent2D,
    format: vk::SurfaceFormatKHR,
}

impl PerSwapchain {
    fn new(
        device: Rc<Device>,
        window: &Window,
        surface: Rc<Surface>,
        renderer: Option<&Renderer>,
        old: Option<&PerSwapchain>,
    ) -> Result<PerSwapchain> {
        unsafe {
            let capabilities = device
                .loaders
                .surface
                .get_physical_device_surface_capabilities(device.physical_device, surface.surface)
                .context("Could not get surface capabilities")?;

            let formats = device
                .loaders
                .surface
                .get_physical_device_surface_formats(device.physical_device, surface.surface)
                .context("Could not get surface formats")?;

            let present_modes = device
                .loaders
                .surface
                .get_physical_device_surface_present_modes(device.physical_device, surface.surface)
                .context("Could not get present modes")?;

            let image_count = capabilities.min_image_count;

            let window_size = window.inner_size();
            let swap_size = vk::Extent2D {
                width: cmp::min(
                    cmp::max(window_size.width, capabilities.min_image_extent.width),
                    capabilities.max_image_extent.width,
                ),
                height: cmp::min(
                    cmp::max(window_size.height, capabilities.min_image_extent.height),
                    capabilities.max_image_extent.height,
                ),
            };

            let format = (|| {
                let preference = [vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8A8_SRGB,
                    color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
                }];

                for pref in preference {
                    if formats.contains(&pref) {
                        return pref;
                    }
                }
                formats[0]
            })();

            let present = (|| {
                let preference = [
                    vk::PresentModeKHR::MAILBOX,
                    vk::PresentModeKHR::IMMEDIATE,
                    vk::PresentModeKHR::FIFO_RELAXED,
                    vk::PresentModeKHR::FIFO,
                ];

                for pref in preference {
                    if present_modes.contains(&pref) {
                        return pref;
                    }
                }
                present_modes[0]
            })();

            let swapchain_info = vk::SwapchainCreateInfoKHR::default()
                .surface(surface.surface)
                .min_image_count(image_count)
                .image_color_space(format.color_space)
                .image_format(format.format)
                .image_extent(swap_size)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .pre_transform(capabilities.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present)
                .clipped(true)
                .old_swapchain(match old {
                    Some(swap) => swap.swapchain,
                    None => SwapchainKHR::default(),
                });

            let shared_queues = [device.present_queue_family, device.graphics_queue_family];
            let swapchain_info = if shared_queues[0] != shared_queues[1] {
                swapchain_info
                    .image_sharing_mode(vk::SharingMode::CONCURRENT)
                    .queue_family_indices(&shared_queues)
            } else {
                swapchain_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            };

            let swapchain = device
                .swapchain_loader
                .create_swapchain(&swapchain_info, None)
                .context("Could not create swapchain")?;

            let images = device
                .swapchain_loader
                .get_swapchain_images(swapchain)
                .context("Could not get images for swapchain")?;

            let views = images
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

                    device.device.create_image_view(&view_info, None)
                })
                .collect::<VkResult<Vec<vk::ImageView>>>()?;

            let mut result = PerSwapchain {
                device,
                surface,
                swapchain,
                images,
                views,
                framebuffers: Vec::new(),
                size: swap_size,
                format,
            };

            if let Some(r) = renderer {
                result
                    .create_framebuffers(r)
                    .context("Creating initial framebuffers")?;
            }

            Ok(result)
        }
    }

    fn create_framebuffers(&mut self, renderer: &Renderer) -> VkResult<()> {
        assert!(self.framebuffers.is_empty());
        assert!(!self.images.is_empty());
        assert_eq!(self.device.device.handle(), renderer.device.device.handle());

        unsafe {
            for &image_view in self.views.iter() {
                match self.device.device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(renderer.renderpass)
                        .attachments(&[image_view])
                        .width(self.size.width)
                        .height(self.size.height)
                        .layers(1),
                    None,
                ) {
                    Ok(fb) => {
                        self.framebuffers.push(fb);
                    }
                    Err(e) => {
                        for fb in self.framebuffers.iter() {
                            self.device.device.destroy_framebuffer(*fb, None);
                        }
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for PerSwapchain {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device.device_wait_idle();
            for fb in self.framebuffers.iter() {
                self.device.device.destroy_framebuffer(*fb, None);
            }
            for &view in self.views.iter() {
                self.device.device.destroy_image_view(view, None);
            }
            self.device
                .swapchain_loader
                .destroy_swapchain(self.swapchain, None);
        }
    }
}

struct App {
    renderer: Renderer,
    windows: HashMap<winit::window::WindowId, VulkanWindow>,
}

struct VulkanWindow {
    window: Window,
    surface: Rc<Surface>,
    device: Rc<Device>,
    swap: PerSwapchain,
    per_frame: Vec<PerFrame>,

    frame_count: usize,
    count_start_time: std::time::Instant,
    count_start_frame: usize,
}

impl Drop for VulkanWindow {
    fn drop(&mut self) {
        unsafe {}
    }
}

struct Renderer {
    device: Rc<Device>,
    renderpass: vk::RenderPass,
    vertex_shader_module: vk::ShaderModule,
    fragment_shader_module: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

impl Renderer {
    unsafe fn new(device: Rc<Device>, swap: &PerSwapchain) -> Result<Self> {
        let color_attachment_desc = [vk::AttachmentDescription::default()
            .format(swap.format.format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)];

        let color_attchment_ref = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];

        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color_attchment_ref)];

        let subpass_dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)];

        let renderpass_info = vk::RenderPassCreateInfo::default()
            .attachments(&color_attachment_desc)
            .subpasses(&subpass)
            .dependencies(&subpass_dependencies);

        let renderpass = device.device.create_render_pass(&renderpass_info, None)?;

        let pipeline_layout = device
            .device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
            .unwrap();

        let create_shader_module = |bytecode| {
            let code = transmute_many::<u32, PedanticGuard>(bytecode).unwrap();
            let shadermodule_info = vk::ShaderModuleCreateInfo::default().code(code);
            device.device.create_shader_module(&shadermodule_info, None)
        };
        let vertex_shader_module = create_shader_module(&VERTEX_BYTECODE)?;
        let fragment_shader_module = create_shader_module(&FRAGMENT_BYTECODE)?;

        let create_shader_stage = |module, stage| {
            vk::PipelineShaderStageCreateInfo::default()
                .module(module)
                .stage(stage)
                .name(CStr::from_bytes_with_nul(b"main\0").unwrap())
        };

        let shader_stages = [
            create_shader_stage(vertex_shader_module, vk::ShaderStageFlags::VERTEX),
            create_shader_stage(fragment_shader_module, vk::ShaderStageFlags::FRAGMENT),
        ];

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: swap.size.width as f32,
            height: swap.size.height as f32,
            min_depth: 0.0,
            max_depth: 1.,
        }];

        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swap.size,
        }];

        let viewport_info = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewport)
            .scissors(&scissor);

        let rasterizer_info = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE);

        let multisample_info = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .min_sample_shading(1.0)
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        let blendattachment_info = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(
                vk::ColorComponentFlags::R
                    | vk::ColorComponentFlags::G
                    | vk::ColorComponentFlags::B
                    | vk::ColorComponentFlags::A,
            )
            .blend_enable(false)];

        let colorblend_info =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&blendattachment_info);

        let mut dyn_states = Vec::<vk::DynamicState>::new();
        if VK_DYNAMIC_VIEW_SIZE {
            dyn_states.push(vk::DynamicState::VIEWPORT);
            dyn_states.push(vk::DynamicState::SCISSOR);
        }
        let dyn_state = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_info)
            .rasterization_state(&rasterizer_info)
            .multisample_state(&multisample_info)
            .color_blend_state(&colorblend_info)
            .layout(pipeline_layout)
            .render_pass(renderpass)
            .dynamic_state(&dyn_state)
            .subpass(0);

        let pipeline = device
            .device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .unwrap()[0];

        Ok(Renderer {
            device,
            renderpass,
            vertex_shader_module,
            fragment_shader_module,
            pipeline_layout,
            pipeline,
        })
    }

    unsafe fn render(&mut self, win: &mut VulkanWindow) -> VkResult<()> {
        let dev: &ash::Device = &self.device.device;

        let pf = &win.per_frame[win.frame_count % win.per_frame.len()];

        dev.wait_for_fences(&[pf.in_flight_fence], true, u64::max_value())?;

        win.frame_count += 1;
        let now = std::time::Instant::now();
        let elapsed = (now - win.count_start_time).as_secs_f64();
        if elapsed > 1.0 {
            let num_frames = win.frame_count - win.count_start_frame;
            println!(
                "{} frames in {:.3} secs, average time {:.2} msecs or {:.1} FPS",
                num_frames,
                elapsed,
                elapsed * 1000. / num_frames as f64,
                num_frames as f64 / elapsed
            );
            win.count_start_frame = win.frame_count;
            win.count_start_time = now;
        }

        let (swap_index, _) = self.device.swapchain_loader.acquire_next_image(
            win.swap.swapchain,
            u64::MAX,
            pf.image_available_semaphore,
            vk::Fence::null(),
        )?;

        dev.reset_command_buffer(pf.command_buffer, vk::CommandBufferResetFlags::empty())?;

        dev.begin_command_buffer(pf.command_buffer, &vk::CommandBufferBeginInfo::default())?;
        dev.cmd_begin_render_pass(
            pf.command_buffer,
            &vk::RenderPassBeginInfo::default()
                .render_pass(self.renderpass)
                .framebuffer(win.swap.framebuffers[swap_index as usize])
                .render_area(win.swap.size.into())
                .clear_values(&[vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [1.0, 1.0, 1.0, 0.0],
                    },
                }]),
            vk::SubpassContents::INLINE,
        );

        dev.cmd_bind_pipeline(
            pf.command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline,
        );

        if VK_DYNAMIC_VIEW_SIZE {
            dev.cmd_set_viewport(
                pf.command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: win.swap.size.width as f32,
                    height: win.swap.size.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.,
                }],
            );

            dev.cmd_set_scissor(
                pf.command_buffer,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: win.swap.size,
                }],
            );
        }

        dev.cmd_draw(pf.command_buffer, 3, 1, 0, 0);
        dev.cmd_end_render_pass(pf.command_buffer);
        dev.end_command_buffer(pf.command_buffer)?;

        dev.reset_fences(&[pf.in_flight_fence])?;
        dev.queue_submit(
            self.device.graphics_queue,
            &[vk::SubmitInfo::default()
                .wait_semaphores(&[pf.image_available_semaphore])
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                .command_buffers(&[pf.command_buffer])
                .signal_semaphores(&[pf.render_finished_semaphore])],
            pf.in_flight_fence,
        )?;

        self.device.swapchain_loader.queue_present(
            self.device.present_queue,
            &vk::PresentInfoKHR::default()
                .wait_semaphores(&[pf.render_finished_semaphore])
                .swapchains(&[win.swap.swapchain])
                .image_indices(&[swap_index]),
        )?;

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device.device_wait_idle();
            self.device.device.destroy_pipeline(self.pipeline, None);
            self.device
                .device
                .destroy_render_pass(self.renderpass, None);
            self.device
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .device
                .destroy_shader_module(self.fragment_shader_module, None);
            self.device
                .device
                .destroy_shader_module(self.vertex_shader_module, None);
        }
    }
}

/// Our Vulkan app.
//#[derive(Clone)]

impl App {
    /// Creates our Vulkan app.
    unsafe fn create(event_loop: &EventLoop<()>) -> Result<Self> {
        // let required_device_extensions = [

        // ];

        let window = WindowBuilder::new()
            .with_title("VK_RUSTY_TRIANGLE")
            .with_inner_size(LogicalSize::new(1536, 1152))
            .build(&event_loop)
            .context("Could not create window.")?;

        let loaders = Rc::new(Loaders::new(&window).context("Could not create Vulkan Loaders")?);

        let surface = Rc::new(Surface {
            loaders: loaders.clone(),
            surface: ash_window::create_surface(&loaders.entry, &loaders.instance, &window, None)
                .context("Could not create surface from window handle")?,
        });

        let device = Rc::new(
            Device::create(loaders.clone(), surface.surface)
                .context("Could not create Vulkan Device")?,
        );

        let mut swap = PerSwapchain::new(device.clone(), &window, surface.clone(), None, None)
            .context("Could not create initial swapchain")?;

        let renderer = Renderer::new(device.clone(), &swap).context("Could not create Renderer")?;

        swap.create_framebuffers(&renderer)
            .context("Could not create framebuffers")?;

        let per_frame: Vec<PerFrame> = (0..4)
            .map(|_| PerFrame::new(device.clone()))
            .collect::<VkResult<Vec<PerFrame>>>()?;

        let v_win = VulkanWindow {
            window,
            surface,
            device: device.clone(),
            swap,
            per_frame,

            frame_count: 0,
            count_start_time: std::time::Instant::now(),
            count_start_frame: 0,
        };

        let mut windows = HashMap::new();
        windows.insert(v_win.window.id(), v_win);

        Ok(Self { renderer, windows })
    }
}

/// The Vulkan handles and associated properties used by our Vulkan app.
#[derive(Clone, Debug, Default)]
struct AppData {}
