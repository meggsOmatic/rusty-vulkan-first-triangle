#![allow(
    dead_code,
    unused_variables,
    //unused_imports
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

use crate::loaders::*;
use crate::device::*;
use crate::perframe::*;
use crate::swapsurface::*;
use crate::renderer::*;
use crate::window::*;

use anyhow::{Context, Result};
use winit::dpi::{ LogicalSize, PhysicalSize, PhysicalPosition };
use winit::event::{Event, KeyboardInput, VirtualKeyCode, WindowEvent};
use winit::event_loop::{EventLoopWindowTarget, ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use ash::prelude::*;
use ash::vk;
use std::collections::HashMap;
use std::rc::Rc;
use rand::prelude::*;


fn main() -> Result<()> {
    pretty_env_logger::init();

    let event_loop = EventLoop::new();

    let mut app = unsafe { App::create(&event_loop)? };
    let mut destroying = false;
    event_loop.run(move |event, el_window_target, control_flow| {
        *control_flow = ControlFlow::Poll;
        let mut close_window = |window_id, destroying: &mut bool| {        
            if app.windows.remove(&window_id).is_none() {
                println!("Could not find window {:?} to remove.", window_id);
            }

            if app.windows.is_empty() {
                *destroying = true;
                *control_flow = ControlFlow::Exit;
            }
        };

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
            } => { close_window(window_id, &mut destroying); },
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
                VirtualKeyCode::N => { app.add_window(el_window_target); },
                VirtualKeyCode::Escape => { close_window(window_id, &mut destroying); },
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
            .with_inner_size(LogicalSize::new(1280f32 * thread_rng().gen_range(0.75f32..1.25f32), 720f32 * thread_rng().gen_range(0.75f32..1.25f32)))
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

            anim_start_time: std::time::Instant::now(),
            shape_rotate_speed: thread_rng().gen_range(-1.5..1.5) as f32,
            color_rotate_speed: thread_rng().gen_range(-1.5..1.5) as f32,
            background_color: [ 1.0, 1.0, 1.0, 0.0 ]
        };

        let mut windows = HashMap::new();
        windows.insert(v_win.window.id(), v_win);

        Ok(Self { renderer, windows })
    }

    fn add_window(&mut self, event_loop: &EventLoopWindowTarget<()>) {
        unsafe {
            let monitor = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next()).unwrap();
            let monitor_size = monitor.size();
            let size = PhysicalSize::<u32> {
                width: (monitor_size.width as f64 * thread_rng().gen_range(0.33..0.66)) as u32,
                height: (monitor_size.height as f64 * thread_rng().gen_range(0.33..0.66)) as u32,
            };
            let pos = PhysicalPosition::<i32> {
                x: monitor.position().x + (thread_rng().gen_range(0..(monitor_size.width - size.width)) as i32),
                y: monitor.position().y + (thread_rng().gen_range(0..(monitor_size.height - size.height)) as i32)
            };
            let window = WindowBuilder::new()
                .with_title("VK_RUSTY_TRIANGLE")
                .with_inner_size(size)
                .with_position(pos)
                .build(event_loop)
                .context("Could not create window.")
                .unwrap();

            let loaders = &self.renderer.device.loaders;
            let surface = Rc::new(Surface {
                loaders: self.renderer.device.loaders.clone(),
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
                self.renderer.device.clone(),
                &window,
                surface.clone(),
                Some(&self.renderer),
                None,
            )
            .context("Could not create additional swapchain")
            .unwrap();

            let per_frame: Vec<PerFrame> = (0..4)
                .map(|_| PerFrame::new(self.renderer.device.clone()))
                .collect::<VkResult<Vec<PerFrame>>>()
                .context("Could not create per-frame queues")
                .unwrap();

            let rate = thread_rng().gen_range(0.0f32..3.0f32);
            let c = thread_rng().gen_range(0.1f32..0.9f32);
            let r = c + thread_rng().gen_range(-0.09f32..0.09f32);
            let g = c + thread_rng().gen_range(-0.09f32..0.09f32);
            let b = c + thread_rng().gen_range(-0.09f32..0.09f32);
            let v_win = VulkanWindow {
                window,
                surface,
                device: self.renderer.device.clone(),
                swap,
                per_frame,

                frame_count: 0,
                count_start_time: std::time::Instant::now(),
                count_start_frame: 0,

                anim_start_time: std::time::Instant::now(),
                shape_rotate_speed: rate - 1.5f32,
                color_rotate_speed: (rate + 1.5f32 % 3.0) - 1.5f32,
                background_color: [ r, g, b, 0.0 ]
            };

            self.windows.insert(v_win.window.id(), v_win);
        }
    }
}

