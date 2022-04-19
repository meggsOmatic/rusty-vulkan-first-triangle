#![allow(
    dead_code,
    unused_variables,
//    clippy::too_many_arguments,
//    clippy::unnecessary_wraps
)]

use anyhow::Result;
use ash::extensions::ext::DebugUtils;
use ash::extensions::khr::{Swapchain, TimelineSemaphore};
use safe_transmute::guard::AllOrNothingGuard;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::windows::WindowExtWindows;
use winit::window::{Window, WindowBuilder};

use ash::prelude::*;
use ash::util::*;
use ash::vk::{self, SwapchainKHR};
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
        .with_inner_size(LogicalSize::new(1536, 1152))
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

struct PerFrame {
    device: Rc<ash::Device>,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available_semaphore: vk::Semaphore,
    render_finished_semaphore: vk::Semaphore,
    in_flight_fence: vk::Fence,
}

impl PerFrame {
    fn create(device: Rc<ash::Device>, command_pool: vk::CommandPool) -> VkResult<PerFrame> {
        unsafe {
            let command_buffer = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            );

            let image_available_semaphore = match command_buffer {
                Ok(_) => device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None),
                Err(e) => Err(e)
            };

            let render_finished_semaphore = image_available_semaphore
                .and_then(|_| device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None));

            let in_flight_fence = render_finished_semaphore.and_then(|_| {
                device.create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
            });

            if in_flight_fence.is_ok() {
                return Ok(PerFrame {
                    device,
                    command_pool,
                    command_buffer: command_buffer.unwrap()[0],
                    image_available_semaphore: image_available_semaphore.unwrap(),
                    render_finished_semaphore: render_finished_semaphore.unwrap(),
                    in_flight_fence: in_flight_fence.unwrap(),
                });
            }

            if let Ok(f) = in_flight_fence {
                device.destroy_fence(f, None);
            }

            if let Ok(s) = render_finished_semaphore {
                device.destroy_semaphore(s, None);
            }

            if let Ok(s) = image_available_semaphore {
                device.destroy_semaphore(s, None);
            }

            if let Ok(c) = command_buffer {
                device.free_command_buffers(command_pool, &c);
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
                .wait_for_fences(&[self.in_flight_fence], true, 100_000_000);
            self.device
                .destroy_semaphore(self.image_available_semaphore, None);
            self.device
                .destroy_semaphore(self.render_finished_semaphore, None);
            self.device.free_command_buffers(self.command_pool, &[self.command_buffer]);
            self.device.destroy_fence(self.in_flight_fence, None);
        }
    }
}


/// Our Vulkan app.
//#[derive(Clone)]
struct App {
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::extensions::khr::Surface,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: Rc<ash::Device>,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
    swapchain_loader: ash::extensions::khr::Swapchain,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_views: Vec<vk::ImageView>,
    swapchain_framebuffers: Vec<vk::Framebuffer>,
    swap_size: vk::Extent2D,
    vertex_shader_module: vk::ShaderModule,
    fragment_shader_module: vk::ShaderModule,
    pipeline_layout: vk::PipelineLayout,
    renderpass: vk::RenderPass,
    pipeline: vk::Pipeline,
    command_pool: vk::CommandPool,

    per_frame: Vec<PerFrame>,

    frame_count: usize,
    count_start_time: std::time::Instant,
    count_start_frame: usize
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

        let (
            physical_device,
            graphics_queue_family,
            present_queue_family,
            swap_capabilities,
            swap_formats,
            present_modes,
        ) = instance
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

                let present_queue_family_index = (graphics_queue_family_index
                    ..=graphics_queue_family_index)
                    .chain(0..queues.len() as u32)
                    .find(|&i| {
                        surface_loader.get_physical_device_surface_support(dev, i, surface)
                            == Ok(true)
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

                Some((
                    dev,
                    graphics_queue_family_index,
                    present_queue_family_index,
                    capabilities,
                    formats,
                    present_modes,
                ))
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

        let device = Rc::new(instance.create_device(physical_device, &device_info, None)?);

        let graphics_queue = device.get_device_queue(graphics_queue_family, 0);

        let present_queue = device.get_device_queue(present_queue_family, 0);

        let window_size = window.inner_size();
        let swap_size = vk::Extent2D {
            width: cmp::min(
                cmp::max(window_size.width, swap_capabilities.min_image_extent.width),
                swap_capabilities.max_image_extent.width,
            ),
            height: cmp::min(
                cmp::max(
                    window_size.height,
                    swap_capabilities.min_image_extent.height,
                ),
                swap_capabilities.max_image_extent.height,
            ),
        };

        let format = (|| {
            let preference = [vk::SurfaceFormatKHR {
                format: vk::Format::B8G8R8A8_SRGB,
                color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
            }];

            for pref in preference {
                if swap_formats.contains(&pref) {
                    return pref;
                }
            }
            swap_formats[0]
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

        let swapchain_count = swap_capabilities.min_image_count + 4;
        let swapchain_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(swapchain_count)
            .image_color_space(format.color_space)
            .image_format(format.format)
            .image_extent(swap_size)
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
            swapchain_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
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
            })
            .collect::<VkResult<Vec<vk::ImageView>>>()?;

        let create_shader_module = |bytecode| {
            let code = transmute_many::<u32, PedanticGuard>(bytecode).unwrap();
            let shadermodule_info = vk::ShaderModuleCreateInfo::default().code(code);
            device.create_shader_module(&shadermodule_info, None)
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
            width: swap_size.width as f32,
            height: swap_size.height as f32,
            min_depth: 0.0,
            max_depth: 1.,
        }];

        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: swap_size,
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

        let pipeline_layout = device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
            .unwrap();

        let color_attachment_desc = [vk::AttachmentDescription::default()
            .format(format.format)
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

        let renderpass = device.create_render_pass(&renderpass_info, None)?;

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
            .subpass(0);

        let pipeline = device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .unwrap()[0];

        let command_pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(graphics_queue_family),
                None,
            )
            .unwrap();

        let swapchain_framebuffers = swapchain_views
            .iter()
            .map(|&image_view| {
                device.create_framebuffer(
                    &vk::FramebufferCreateInfo::default()
                        .render_pass(renderpass)
                        .attachments(&[image_view])
                        .width(swap_size.width)
                        .height(swap_size.height)
                        .layers(1),
                    None,
                )
            })
            .collect::<VkResult<Vec<vk::Framebuffer>>>()?;

        let per_frame: Vec<PerFrame> = (0..4)
            .map(|_| PerFrame::create(device.clone(), command_pool))
            .collect::<VkResult<Vec<PerFrame>>>()?;

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
            swapchain_framebuffers,
            swapchain_views,
            swap_size,
            vertex_shader_module,
            fragment_shader_module,
            pipeline_layout,
            renderpass,
            pipeline,
            command_pool,
            per_frame,
            frame_count: 0,
            count_start_time: std::time::Instant::now(),
            count_start_frame: 0
        })
    }

    /// Renders a frame for our Vulkan app.
    unsafe fn render(&mut self, window: &Window) -> Result<()> {
        let pf = &self.per_frame[self.frame_count % self.per_frame.len()];

        self.device
            .wait_for_fences(&[pf.in_flight_fence], true, u64::max_value())?;

        self.frame_count += 1;
        let now = std::time::Instant::now();
        let elapsed = (now - self.count_start_time).as_secs_f64();
        if elapsed > 1.0 {            
            let num_frames = self.frame_count - self.count_start_frame;
            println!("{} frames in {:.3} secs, average time {:.2} msecs or {:.1} FPS", num_frames, elapsed, elapsed * 1000. / num_frames as f64, num_frames as f64 / elapsed);
            self.count_start_frame = self.frame_count;
            self.count_start_time = now;
        }
        
        self.device.reset_fences(&[pf.in_flight_fence])?;
        let (swap_index, _) = self
            .swapchain_loader
            .acquire_next_image(
                self.swapchain,
                u64::MAX,
                pf.image_available_semaphore,
                vk::Fence::null(),
            )
            .unwrap();
        self.device
            .reset_command_buffer(pf.command_buffer, vk::CommandBufferResetFlags::empty())?;

        self.device
            .begin_command_buffer(pf.command_buffer, &vk::CommandBufferBeginInfo::default())?;
        self.device.cmd_begin_render_pass(
            pf.command_buffer,
            &vk::RenderPassBeginInfo::default()
                .render_pass(self.renderpass)
                .framebuffer(self.swapchain_framebuffers[swap_index as usize])
                .render_area(self.swap_size.into())
                .clear_values(&[vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [1.0, 1.0, 1.0, 0.0],
                    },
                }]),
            vk::SubpassContents::INLINE,
        );
        self.device.cmd_bind_pipeline(
            pf.command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            self.pipeline,
        );
        self.device.cmd_draw(pf.command_buffer, 3, 1, 0, 0);
        self.device.cmd_end_render_pass(pf.command_buffer);
        self.device.end_command_buffer(pf.command_buffer)?;

        self.device.queue_submit(
            self.graphics_queue,
            &[vk::SubmitInfo::default()
                .wait_semaphores(&[pf.image_available_semaphore])
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                .command_buffers(&[pf.command_buffer])
                .signal_semaphores(&[pf.render_finished_semaphore])],
            pf.in_flight_fence,
        )?;

        self.swapchain_loader
            .queue_present(
                self.present_queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&[pf.render_finished_semaphore])
                    .swapchains(&[self.swapchain])
                    .image_indices(&[swap_index]),
            )
            .unwrap();

        //self.device.queue_submit(queue, submits, fence)

        Ok(())
    }

    /// Destroys our Vulkan app.
    unsafe fn destroy(&mut self) {
        let _ = self.device.device_wait_idle();
        self.per_frame.clear();
        for &fb in self.swapchain_framebuffers.iter() {
            self.device.destroy_framebuffer(fb, None);
        }
        self.device.destroy_command_pool(self.command_pool, None);
        self.device.destroy_pipeline(self.pipeline, None);
        self.device.destroy_render_pass(self.renderpass, None);
        self.device
            .destroy_pipeline_layout(self.pipeline_layout, None);
        self.device
            .destroy_shader_module(self.fragment_shader_module, None);
        self.device
            .destroy_shader_module(self.vertex_shader_module, None);
        for &view in self.swapchain_views.iter() {
            self.device.destroy_image_view(view, None);
        }
        self.swapchain_loader
            .destroy_swapchain(self.swapchain, None);
        self.device.destroy_device(None);
        self.surface_loader.destroy_surface(self.surface, None);
        self.instance.destroy_instance(None);
    }
}

/// The Vulkan handles and associated properties used by our Vulkan app.
#[derive(Clone, Debug, Default)]
struct AppData {}
