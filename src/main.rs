#![allow(
    dead_code,
    unused_variables,
    unused_imports
//    clippy::too_many_arguments,
//    clippy::unnecessary_wraps
)]

mod util;
mod config;
mod loaders;
mod device;
mod perframe;
mod swapsurface;
mod renderer;
mod window;

use crate::util::*;
use crate::loaders::*;
use crate::config::*;
use crate::device::*;
use crate::perframe::*;
use crate::swapsurface::*;
use crate::renderer::*;
use crate::window::*;

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



struct App {
    renderer: Renderer,
    windows: HashMap<winit::window::WindowId, VulkanWindow>,
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

