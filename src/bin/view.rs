use ash::{
    extensions::{ext::DebugUtils, khr::Swapchain},
    version::{DeviceV1_0, EntryV1_0, InstanceV1_0},
    vk, Device,
};
use std::borrow::Cow;
use std::ffi::CStr;
use std::path::Path;
use winit::{
    event::{
        DeviceEvent, ElementState, Event, KeyboardInput, StartCause, VirtualKeyCode, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use gumdrop::Options;

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number: i32 = callback_data.message_id_number as i32;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    println!(
        "{:?}:\n{:?} [{} ({})] : {}\n",
        message_severity,
        message_type,
        message_id_name,
        &message_id_number.to_string(),
        message,
    );

    vk::FALSE
}

struct Renderer {
    entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    surface: vk::SurfaceKHR,
    surface_ext: ash::extensions::khr::Surface,
    present_queue: vk::Queue,
    swapchain: vk::SwapchainKHR,
    swapchain_ext: ash::extensions::khr::Swapchain,
    swapchain_images: Vec<vk::Image>,
    allocator: vk_mem::Allocator,
    cmd_pool: vk::CommandPool,
    cmd_buffers: Vec<vk::CommandBuffer>,
    fences: Vec<vk::Fence>,
    image_available_semaphores: Vec<vk::Semaphore>,
    rendering_finished_semaphores: Vec<vk::Semaphore>,
    cur_frame_idx: usize,
    cur_swapchain_idx: usize,
}

impl Renderer {
    fn new(window: &winit::window::Window) -> Self {
        unsafe {
            let entry = ash::Entry::new().expect("Failed to load vulkan library");
            let surface_extensions = ash_window::enumerate_required_extensions(window)
                .expect("Failed to enumerate vulkan window extensions");
            let mut instance_extensions = surface_extensions
                .iter()
                .map(|ext| ext.as_ptr())
                .collect::<Vec<_>>();
            instance_extensions.push(DebugUtils::name().as_ptr());
            let app_desc = vk::ApplicationInfo::builder().api_version(vk::make_version(1, 2, 0));
            let instance_desc = vk::InstanceCreateInfo::builder()
                .application_info(&app_desc)
                .enabled_extension_names(&instance_extensions);

            let instance = entry
                .create_instance(&instance_desc, None)
                .expect("Failed to create vulkan instance");

            // Create a surface from winit window.
            let surface = ash_window::create_surface(&entry, &instance, window, None)
                .expect("Failed to create vulkan window surface");
            let surface_ext = ash::extensions::khr::Surface::new(&entry, &instance);

            let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::all())
                .pfn_user_callback(Some(vulkan_debug_callback));

            let debug_utils_loader = DebugUtils::new(&entry, &instance);
            let _debug_call_back = debug_utils_loader
                .create_debug_utils_messenger(&debug_info, None)
                .unwrap();
            let pdevices = instance
                .enumerate_physical_devices()
                .expect("Physical device error");
            let (pdevice, queue_family_index) = pdevices
                .iter()
                .map(|pdevice| {
                    instance
                        .get_physical_device_queue_family_properties(*pdevice)
                        .iter()
                        .enumerate()
                        .filter_map(|(index, ref info)| {
                            let supports_graphic_and_surface =
                                info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                                    && surface_ext
                                        .get_physical_device_surface_support(
                                            *pdevice,
                                            index as u32,
                                            surface,
                                        )
                                        .unwrap();
                            if supports_graphic_and_surface {
                                Some((*pdevice, index))
                            } else {
                                None
                            }
                        })
                        .next()
                })
                .filter_map(|v| v)
                .next()
                .expect("Couldn't find suitable device.");
            let queue_family_index = queue_family_index as u32;
            let device_extension_names_raw = [Swapchain::name().as_ptr()];
            let priorities = [1.0];

            let queue_info = [vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(queue_family_index)
                .queue_priorities(&priorities)
                .build()];

            let device_create_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(&queue_info)
                .enabled_extension_names(&device_extension_names_raw);

            let device: Device = instance
                .create_device(pdevice, &device_create_info, None)
                .unwrap();

            let present_queue = device.get_device_queue(queue_family_index as u32, 0);

            let surface_formats = surface_ext
                .get_physical_device_surface_formats(pdevice, surface)
                .unwrap();
            let surface_format = surface_formats
                .iter()
                .find(|sfmt| match sfmt.format {
                    vk::Format::R8G8B8A8_UNORM => true,
                    _ => false,
                })
                .expect("Unable to find suitable surface format.");
            let surface_capabilities = surface_ext
                .get_physical_device_surface_capabilities(pdevice, surface)
                .unwrap();
            let mut desired_image_count = surface_capabilities.min_image_count + 1;
            if surface_capabilities.max_image_count > 0
                && desired_image_count > surface_capabilities.max_image_count
            {
                desired_image_count = surface_capabilities.max_image_count;
            }
            let surface_resolution = match surface_capabilities.current_extent.width {
                std::u32::MAX => vk::Extent2D {
                    width: window.inner_size().width,
                    height: window.inner_size().height,
                },
                _ => surface_capabilities.current_extent,
            };
            let pre_transform = if surface_capabilities
                .supported_transforms
                .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
            {
                vk::SurfaceTransformFlagsKHR::IDENTITY
            } else {
                surface_capabilities.current_transform
            };
            let present_modes = surface_ext
                .get_physical_device_surface_present_modes(pdevice, surface)
                .unwrap();
            let present_mode = present_modes
                .iter()
                .cloned()
                .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
                .unwrap_or(vk::PresentModeKHR::FIFO);
            let swapchain_ext = Swapchain::new(&instance, &device);

            let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
                .surface(surface)
                .min_image_count(desired_image_count)
                .image_color_space(surface_format.color_space)
                .image_format(surface_format.format)
                .image_extent(surface_resolution)
                .image_usage(vk::ImageUsageFlags::TRANSFER_DST)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(pre_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(present_mode)
                .clipped(true)
                .image_array_layers(1);

            let swapchain = swapchain_ext
                .create_swapchain(&swapchain_create_info, None)
                .unwrap();

            let swapchain_images = swapchain_ext.get_swapchain_images(swapchain).unwrap();

            let allocator = vk_mem::Allocator::new(&vk_mem::AllocatorCreateInfo {
                physical_device: pdevice,
                device: device.clone(),
                instance: instance.clone(),
                flags: vk_mem::AllocatorCreateFlags::NONE,
                preferred_large_heap_block_size: 0,
                frame_in_use_count: 0,
                heap_size_limits: None,
            })
            .unwrap();

            let cmd_pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::builder()
                        .queue_family_index(queue_family_index)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .unwrap();

            let cmd_buffers = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::builder()
                        .command_pool(cmd_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(desired_image_count),
                )
                .unwrap();

            let create_info = vk::SemaphoreCreateInfo::builder();
            let image_available_semaphores: Vec<_> = (0..desired_image_count)
                .map(|_| device.create_semaphore(&create_info, None).unwrap())
                .collect();
            let rendering_finished_semaphores: Vec<_> = (0..desired_image_count)
                .map(|_| device.create_semaphore(&create_info, None).unwrap())
                .collect();

            let create_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
            let fences: Vec<_> = (0..desired_image_count)
                .map(|_| device.create_fence(&create_info, None).unwrap())
                .collect();

            Renderer {
                entry,
                instance,
                device,
                surface,
                surface_ext,
                present_queue,
                swapchain,
                swapchain_ext,
                swapchain_images,
                allocator,
                cmd_pool,
                cmd_buffers,
                fences,
                image_available_semaphores,
                rendering_finished_semaphores,
                cur_frame_idx: 0,
                cur_swapchain_idx: 0,
            }
        }
    }

    fn begin_frame(&mut self) -> vk::CommandBuffer {
        unsafe {
            // Acquire the current swapchain image index
            // TODO: Handle suboptimal swapchains
            let (image_index, _is_suboptimal) = self
                .swapchain_ext
                .acquire_next_image(
                    self.swapchain,
                    u64::MAX,
                    self.image_available_semaphores[self.cur_frame_idx],
                    vk::Fence::null(),
                )
                .unwrap();
            self.cur_swapchain_idx = image_index as usize;

            // Wait for the resources for this frame to become available
            self.device
                .wait_for_fences(&[self.fences[self.cur_frame_idx]], true, u64::MAX)
                .unwrap();

            let cmd_buffer = self.cmd_buffers[self.cur_swapchain_idx];

            self.device
                .begin_command_buffer(cmd_buffer, &vk::CommandBufferBeginInfo::default())
                .unwrap();

            cmd_buffer
        }
    }

    fn end_frame(&mut self, cmd_buffer: vk::CommandBuffer) {
        unsafe {
            self.device.end_command_buffer(cmd_buffer).unwrap();

            // The user should always pass the same cmdbuffer back to us after a frame
            assert_eq!(self.cmd_buffers[self.cur_swapchain_idx], cmd_buffer);

            let wait_semaphores = vec![self.image_available_semaphores[self.cur_frame_idx]];
            let command_buffers = vec![cmd_buffer];
            let signal_semaphores = vec![self.rendering_finished_semaphores[self.cur_frame_idx]];
            let submit_info = vk::SubmitInfo::builder()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::TOP_OF_PIPE])
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores)
                .build();

            let fence = self.fences[self.cur_frame_idx];
            self.device.reset_fences(&[fence]).unwrap();
            self.device
                .queue_submit(self.present_queue, &[submit_info], fence)
                .unwrap();

            let swapchains = vec![self.swapchain];
            let image_indices = vec![self.cur_swapchain_idx as u32];
            let present_info = vk::PresentInfoKHR::builder()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            self.swapchain_ext
                .queue_present(self.present_queue, &present_info)
                .unwrap();

            self.cur_frame_idx = (self.cur_frame_idx + 1) % self.swapchain_images.len();
        }
    }

    fn get_device(&self) -> &ash::Device {
        &self.device
    }
    fn get_allocator(&mut self) -> &mut vk_mem::Allocator {
        &mut self.allocator
    }

    fn get_cur_swapchain_idx(&self) -> usize {
        self.cur_swapchain_idx
    }
    fn get_cur_swapchain_image(&self) -> vk::Image {
        self.swapchain_images[self.cur_swapchain_idx]
    }
    fn get_num_swapchain_images(&self) -> usize {
        self.swapchain_images.len()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            // TODO: Proper vk destruction
            self.instance.destroy_instance(None);
        }
    }
}

pub fn show(elf_path: &impl AsRef<Path>) -> ! {
    let mut hw_device = devsim::device::Device::new();

    hw_device
        .load_elf(&elf_path)
        .expect("Failed to load elf file");

    let (fb_width, fb_height) = hw_device
        .query_framebuffer_size()
        .expect("Failed to query framebuffer size");

    let window_width = 256;
    let window_height = 256;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DevSim View")
        .with_inner_size(winit::dpi::PhysicalSize::new(window_width, window_height))
        .build(&event_loop)
        .expect("Failed to create window");

    unsafe {
        let mut renderer = Renderer::new(&window);
        let num_swapchain_images = renderer.get_num_swapchain_images() as u32;

        let image_size_bytes = fb_width * fb_height * 4;

        let create_info = vk_mem::AllocationCreateInfo {
            usage: vk_mem::MemoryUsage::CpuToGpu,
            flags: vk_mem::AllocationCreateFlags::MAPPED,
            ..Default::default()
        };

        let (buffer, _allocation, allocation_info) = renderer
            .get_allocator()
            .create_buffer(
                &ash::vk::BufferCreateInfo::builder()
                    .size((((image_size_bytes + 255) & !255) * num_swapchain_images) as u64)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC),
                &create_info,
            )
            .unwrap();

        let p_buf = allocation_info.get_mapped_data();

        event_loop.run(move |event, _, control_flow| match event {
            Event::NewEvents(StartCause::Init) => {
                *control_flow = ControlFlow::Poll;
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::DeviceEvent { event, .. } => match event {
                DeviceEvent::Key(KeyboardInput {
                    virtual_keycode: Some(keycode),
                    state,
                    ..
                }) => match (keycode, state) {
                    (VirtualKeyCode::Escape, ElementState::Released) => {
                        *control_flow = ControlFlow::Exit
                    }
                    _ => (),
                },
                _ => (),
            },
            Event::MainEventsCleared => {
                let cmd_buffer = renderer.begin_frame();

                // Enable the device
                hw_device.enable();

                loop {
                    match hw_device.query_is_halted() {
                        Ok(is_halted) => {
                            if !is_halted {
                                // Still executing...
                            } else {
                                break;
                            }
                        }
                        Err(err) => {
                            println!("Device error: {}", err);
                            break;
                        }
                    }
                }

                let p_current_buf = p_buf
                    .offset((image_size_bytes * (renderer.get_cur_swapchain_idx() as u32)) as isize)
                    as *mut u8;
                let mut current_buf =
                    core::slice::from_raw_parts_mut(p_current_buf, image_size_bytes as usize);

                hw_device
                    .dump_framebuffer(&mut current_buf)
                    .expect("Failed to dump device framebuffer!");

                let device = renderer.get_device();
                let swapchain_image = renderer.get_cur_swapchain_image();

                device.cmd_pipeline_barrier(
                    cmd_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier::builder()
                        .src_access_mask(vk::AccessFlags::empty())
                        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::UNDEFINED)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(swapchain_image)
                        .subresource_range(
                            vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        )
                        .build()],
                );

                device.cmd_clear_color_image(
                    cmd_buffer,
                    swapchain_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 1.0],
                    },
                    &[vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1)
                        .build()],
                );

                device.cmd_pipeline_barrier(
                    cmd_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier::builder()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(swapchain_image)
                        .subresource_range(
                            vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        )
                        .build()],
                );

                // Copy the latest device image data to the swapchain image
                let buffer_offset = (renderer.get_cur_swapchain_idx() as u32) * image_size_bytes;
                device.cmd_copy_buffer_to_image(
                    cmd_buffer,
                    buffer,
                    swapchain_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::builder()
                        .buffer_offset(buffer_offset as u64)
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image_offset(vk::Offset3D {
                            x: ((window_width / 2) - (fb_width / 2)) as i32,
                            y: ((window_height / 2) - (fb_height / 2)) as i32,
                            z: 0,
                        })
                        .image_extent(vk::Extent3D {
                            width: fb_width,
                            height: fb_height,
                            depth: 1,
                        })
                        .build()],
                );

                device.cmd_pipeline_barrier(
                    cmd_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier::builder()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::MEMORY_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(swapchain_image)
                        .subresource_range(
                            vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        )
                        .build()],
                );

                renderer.end_frame(cmd_buffer);
            }
            Event::LoopDestroyed => {}
            _ => (),
        });
    }
}

#[derive(Debug, Options)]
struct SimOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(free, required, help = "path to an elf file to execute")]
    elf_path: String,
}

fn main() {
    let opts = SimOptions::parse_args_default_or_exit();
    show(&opts.elf_path);
}
