use ash::{
    version::{DeviceV1_0, InstanceV1_0},
    vk,
};
use std::path::Path;
use std::sync::{Arc, Weak};
use winit::{
    event::{
        DeviceEvent, ElementState, Event, KeyboardInput, StartCause, VirtualKeyCode, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use devsim::vkutil::*;
use gumdrop::Options;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Selects a physical device from the provided list
fn select_physical_device(physical_devices: &[vk::PhysicalDevice]) -> vk::PhysicalDevice {
    // TODO: Support proper physical device selection
    //       For now, we just use the first device
    physical_devices[0]
}

struct FrameState {
    fb_image_view: VkImageView,
    fb_image: VkImage,
    cmd_buffer: vk::CommandBuffer,
    fence: VkFence,
    descriptor_set: vk::DescriptorSet,
    rendering_finished_semaphore: VkSemaphore,
}

impl FrameState {
    fn new(
        device: &VkDevice,
        allocator: Weak<vk_mem::Allocator>,
        command_pool: &VkCommandPool,
        descriptor_pool: &VkDescriptorPool,
        descriptor_set_layout: &VkDescriptorSetLayout,
        fb_width: u32,
        fb_height: u32,
    ) -> Result<Self> {
        let cmd_buffer = command_pool.allocate_command_buffer(vk::CommandBufferLevel::PRIMARY)?;

        let rendering_finished_semaphore =
            VkSemaphore::new(device.raw(), &vk::SemaphoreCreateInfo::default())?;
        let fence = VkFence::new(
            device.raw(),
            &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED),
        )?;

        let descriptor_set =
            descriptor_pool.allocate_descriptor_set(descriptor_set_layout.raw())?;

        let fb_image = VkImage::new(
            allocator,
            &ash::vk::ImageCreateInfo::builder()
                .image_type(vk::ImageType::TYPE_2D)
                .extent(vk::Extent3D {
                    width: fb_width,
                    height: fb_height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .format(vk::Format::R8G8B8A8_UNORM)
                .tiling(vk::ImageTiling::OPTIMAL)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .samples(vk::SampleCountFlags::TYPE_1),
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::GpuOnly,
                ..Default::default()
            },
        )?;

        let fb_image_view = VkImageView::new(
            device.raw(),
            &vk::ImageViewCreateInfo::builder()
                .image(fb_image.raw())
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .components(
                    vk::ComponentMapping::builder()
                        .r(vk::ComponentSwizzle::IDENTITY)
                        .g(vk::ComponentSwizzle::IDENTITY)
                        .b(vk::ComponentSwizzle::IDENTITY)
                        .a(vk::ComponentSwizzle::IDENTITY)
                        .build(),
                )
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1)
                        .build(),
                ),
        )?;
        unsafe {
            device.raw().upgrade().unwrap().update_descriptor_sets(
                &[vk::WriteDescriptorSet::builder()
                    .dst_set(descriptor_set)
                    .dst_binding(2)
                    .dst_array_element(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&[vk::DescriptorImageInfo::builder()
                        .image_view(fb_image_view.raw())
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .build()])
                    .build()],
                &[],
            );
        }

        Ok(FrameState {
            fb_image_view,
            fb_image,
            cmd_buffer,
            fence,
            descriptor_set,
            rendering_finished_semaphore,
        })
    }
}

struct Renderer {
    frame_states: Vec<FrameState>,
    fb_upload_buffer: VkBuffer,
    image_available_semaphores: Vec<VkSemaphore>,
    framebuffers: Vec<VkFramebuffer>,
    renderpass: VkRenderPass,
    cmd_pool: VkCommandPool,
    sampler: VkSampler,
    pipeline_layout: VkPipelineLayout,
    descriptor_set_layout: VkDescriptorSetLayout,
    descriptor_pool: VkDescriptorPool,
    gfx_pipeline: VkPipeline,
    pipeline_cache: VkPipelineCache,
    cur_frame_idx: usize,
    cur_swapchain_idx: usize,
    swapchain_image_views: Vec<VkImageView>,
    swapchain: VkSwapchain,
    allocator: Arc<vk_mem::Allocator>,
    surface: VkSurface,
    device: VkDevice,
    _debug_messenger: Option<VkDebugMessenger>,
    instance: VkInstance,
}

impl Renderer {
    fn new(
        window: &winit::window::Window,
        fb_width: u32,
        fb_height: u32,
        enable_validation: bool,
    ) -> Result<Self> {
        let instance = VkInstance::new(window, enable_validation)?;

        let _debug_messenger = if enable_validation {
            Some(VkDebugMessenger::new(&instance)?)
        } else {
            None
        };

        let physical_devices = unsafe { instance.raw().enumerate_physical_devices()? };
        let physical_device = select_physical_device(&physical_devices);

        let surface = VkSurface::new(&instance, window)?;

        let device = VkDevice::new(&instance, physical_device, &surface)?;

        let allocator = Arc::new(vk_mem::Allocator::new(&vk_mem::AllocatorCreateInfo {
            physical_device,
            device: (*device.raw().upgrade().unwrap()).clone(),
            instance: instance.raw().clone(),
            flags: vk_mem::AllocatorCreateFlags::NONE,
            preferred_large_heap_block_size: 0,
            frame_in_use_count: 0,
            heap_size_limits: None,
        })?);

        let pipeline_cache =
            VkPipelineCache::new(device.raw(), &vk::PipelineCacheCreateInfo::default())?;

        let swapchain = VkSwapchain::new(
            &instance,
            &surface,
            &device,
            window.inner_size().width,
            window.inner_size().height,
        )?;

        let surface_format = swapchain.surface_format;
        let surface_resolution = swapchain.surface_resolution;
        let desired_image_count = swapchain.images.len() as u32;
        let queue_family_index = 0;

        let swapchain_image_views = swapchain
            .images
            .iter()
            .map(|image| {
                VkImageView::new(
                    device.raw(),
                    &vk::ImageViewCreateInfo::builder()
                        .image(*image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(surface_format.format)
                        .components(
                            vk::ComponentMapping::builder()
                                .r(vk::ComponentSwizzle::IDENTITY)
                                .g(vk::ComponentSwizzle::IDENTITY)
                                .b(vk::ComponentSwizzle::IDENTITY)
                                .a(vk::ComponentSwizzle::IDENTITY)
                                .build(),
                        )
                        .subresource_range(
                            vk::ImageSubresourceRange::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        ),
                )
            })
            .collect::<Result<Vec<VkImageView>>>()?;

        let renderpass = VkRenderPass::new(
            device.raw(),
            &vk::RenderPassCreateInfo::builder()
                .attachments(&[vk::AttachmentDescription::builder()
                    .format(surface_format.format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .build()])
                .subpasses(&[vk::SubpassDescription::builder()
                    .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                    .color_attachments(&[vk::AttachmentReference::builder()
                        .attachment(0)
                        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                        .build()])
                    .build()]),
        )?;

        let framebuffers = swapchain_image_views
            .iter()
            .map(|image_view| {
                VkFramebuffer::new(
                    device.raw(),
                    &vk::FramebufferCreateInfo::builder()
                        .render_pass(renderpass.raw())
                        .attachments(&[image_view.raw()])
                        .width(surface_resolution.width)
                        .height(surface_resolution.height)
                        .layers(1),
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let cmd_pool = VkCommandPool::new(
            device.raw(),
            &vk::CommandPoolCreateInfo::builder()
                .queue_family_index(queue_family_index)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
        )?;

        let sampler = VkSampler::new(
            device.raw(),
            &vk::SamplerCreateInfo::builder()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .min_lod(0.0)
                .max_lod(10000.0)
                .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK),
        )?;

        let descriptor_set_layout = VkDescriptorSetLayout::new(
            device.raw(),
            &vk::DescriptorSetLayoutCreateInfo::builder().bindings(&[
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::COMPUTE)
                    .immutable_samplers(&[sampler.raw()])
                    .build(),
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::COMPUTE)
                    .build(),
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::COMPUTE)
                    .build(),
            ]),
        )?;

        let pipeline_layout = VkPipelineLayout::new(
            device.raw(),
            &vk::PipelineLayoutCreateInfo::builder().set_layouts(&[descriptor_set_layout.raw()]),
        )?;

        let descriptor_pool = VkDescriptorPool::new(
            device.raw(),
            &vk::DescriptorPoolCreateInfo::builder()
                .max_sets(desired_image_count)
                .pool_sizes(&[
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::SAMPLER)
                        .descriptor_count(desired_image_count)
                        .build(),
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
                        .descriptor_count(desired_image_count)
                        .build(),
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::SAMPLED_IMAGE)
                        .descriptor_count(desired_image_count)
                        .build(),
                ]),
        )?;

        let mut compiler = shaderc::Compiler::new().expect("Failed to create compiler");

        let vert_source = include_str!("../shaders/FullscreenPass.vert");
        let frag_source = include_str!("../shaders/CopyTexture.frag");

        let vert_result = compiler.compile_into_spirv(
            vert_source,
            shaderc::ShaderKind::Vertex,
            "FullscreenPass.vert",
            "main",
            None,
        )?;

        let vert_module = VkShaderModule::new(
            device.raw(),
            &vk::ShaderModuleCreateInfo::builder().code(vert_result.as_binary()),
        )?;

        let frag_result = compiler.compile_into_spirv(
            frag_source,
            shaderc::ShaderKind::Fragment,
            "CopyTexture.frag",
            "main",
            None,
        )?;

        let frag_module = VkShaderModule::new(
            device.raw(),
            &vk::ShaderModuleCreateInfo::builder().code(frag_result.as_binary()),
        )?;

        let entry_point_c_string = std::ffi::CString::new("main").unwrap();
        let gfx_pipeline = pipeline_cache.create_graphics_pipeline(
            &vk::GraphicsPipelineCreateInfo::builder()
                .stages(&[
                    vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(vert_module.raw())
                        .name(entry_point_c_string.as_c_str())
                        .build(),
                    vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::FRAGMENT)
                        .module(frag_module.raw())
                        .name(entry_point_c_string.as_c_str())
                        .build(),
                ])
                .input_assembly_state(
                    &vk::PipelineInputAssemblyStateCreateInfo::builder()
                        .topology(vk::PrimitiveTopology::TRIANGLE_LIST),
                )
                .vertex_input_state(&vk::PipelineVertexInputStateCreateInfo::builder().build())
                .viewport_state(
                    &vk::PipelineViewportStateCreateInfo::builder()
                        .viewports(&[vk::Viewport::default()])
                        .scissors(&[vk::Rect2D::default()]),
                )
                .rasterization_state(
                    &vk::PipelineRasterizationStateCreateInfo::builder()
                        .polygon_mode(vk::PolygonMode::FILL)
                        .cull_mode(vk::CullModeFlags::BACK)
                        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                        .line_width(1.0),
                )
                .multisample_state(
                    &vk::PipelineMultisampleStateCreateInfo::builder()
                        .rasterization_samples(vk::SampleCountFlags::TYPE_1),
                )
                // Don't need depth state
                .color_blend_state(
                    &vk::PipelineColorBlendStateCreateInfo::builder().attachments(&[
                        vk::PipelineColorBlendAttachmentState::builder()
                            .color_write_mask(
                                vk::ColorComponentFlags::R
                                    | vk::ColorComponentFlags::G
                                    | vk::ColorComponentFlags::B
                                    | vk::ColorComponentFlags::A,
                            )
                            .build(),
                    ]),
                )
                .dynamic_state(
                    &vk::PipelineDynamicStateCreateInfo::builder()
                        .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]),
                )
                .layout(pipeline_layout.raw())
                .render_pass(renderpass.raw())
                .subpass(0),
        )?;

        let frame_states = swapchain_image_views
            .iter()
            .map(|_image_view| {
                FrameState::new(
                    &device,
                    Arc::downgrade(&allocator),
                    &cmd_pool,
                    &descriptor_pool,
                    &descriptor_set_layout,
                    fb_width,
                    fb_height,
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let image_available_semaphores = swapchain
            .images
            .iter()
            .map(|_| VkSemaphore::new(device.raw(), &vk::SemaphoreCreateInfo::default()))
            .collect::<Result<Vec<_>>>()?;

        let image_size_bytes = fb_width * fb_height * 4;

        let fb_upload_buffer = VkBuffer::new(
            Arc::downgrade(&allocator),
            &ash::vk::BufferCreateInfo::builder()
                .size((((image_size_bytes + 255) & !255) * desired_image_count) as u64)
                .usage(vk::BufferUsageFlags::TRANSFER_SRC),
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::CpuOnly,
                flags: vk_mem::AllocationCreateFlags::MAPPED,
                ..Default::default()
            },
        )?;

        Ok(Renderer {
            frame_states,
            fb_upload_buffer,
            framebuffers,
            image_available_semaphores,
            renderpass,
            cmd_pool,
            sampler,
            pipeline_layout,
            descriptor_set_layout,
            descriptor_pool,
            gfx_pipeline,
            pipeline_cache,
            cur_frame_idx: 0,
            cur_swapchain_idx: 0,
            swapchain_image_views,
            swapchain,
            allocator,
            surface,
            device,
            _debug_messenger,
            instance,
        })
    }

    fn get_cur_frame_state(&self) -> &FrameState {
        &self.frame_states[self.cur_swapchain_idx]
    }

    fn get_frame_state(&self, frame_index: usize) -> &FrameState {
        &self.frame_states[frame_index]
    }

    fn begin_frame(&mut self) -> vk::CommandBuffer {
        unsafe {
            // Acquire the current swapchain image index
            // TODO: Handle suboptimal swapchains
            let (image_index, _is_suboptimal) = self
                .swapchain
                .acquire_next_image(
                    u64::MAX,
                    Some(self.image_available_semaphores[self.cur_frame_idx].raw()),
                    None,
                )
                .unwrap();
            self.cur_swapchain_idx = image_index as usize;

            let frame_state = self.get_cur_frame_state();

            // Wait for the resources for this frame to become available
            self.device
                .raw()
                .upgrade()
                .unwrap()
                .wait_for_fences(&[frame_state.fence.raw()], true, u64::MAX)
                .unwrap();

            let cmd_buffer = frame_state.cmd_buffer;

            self.device
                .raw()
                .upgrade()
                .unwrap()
                .begin_command_buffer(cmd_buffer, &vk::CommandBufferBeginInfo::default())
                .unwrap();

            cmd_buffer
        }
    }

    fn begin_render(&self) {
        let frame_state = self.get_cur_frame_state();
        let framebuffer = &self.framebuffers[self.cur_swapchain_idx];
        unsafe {
            self.device.raw().upgrade().unwrap().cmd_begin_render_pass(
                frame_state.cmd_buffer,
                &vk::RenderPassBeginInfo::builder()
                    .render_pass(self.renderpass.raw())
                    .framebuffer(framebuffer.raw())
                    .render_area(
                        vk::Rect2D::builder()
                            .extent(self.swapchain.surface_resolution)
                            .build(),
                    )
                    .clear_values(&[vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [0.0, 0.0, 0.0, 1.0],
                        },
                    }]),
                vk::SubpassContents::INLINE,
            );
        }
    }

    fn end_render(&self) {
        let frame_state = self.get_cur_frame_state();
        unsafe {
            self.device
                .raw()
                .upgrade()
                .unwrap()
                .cmd_end_render_pass(frame_state.cmd_buffer);
        }
    }

    fn end_frame(&mut self, cmd_buffer: vk::CommandBuffer) {
        let frame_state = self.get_cur_frame_state();
        unsafe {
            self.device
                .raw()
                .upgrade()
                .unwrap()
                .end_command_buffer(cmd_buffer)
                .unwrap();

            // The user should always pass the same cmdbuffer back to us after a frame
            assert_eq!(frame_state.cmd_buffer, cmd_buffer);

            let wait_semaphores = [self.image_available_semaphores[self.cur_frame_idx].raw()];
            let command_buffers = [cmd_buffer];
            let signal_semaphores = [frame_state.rendering_finished_semaphore.raw()];
            let submit_info = vk::SubmitInfo::builder()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::TOP_OF_PIPE])
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores)
                .build();

            let fence = &frame_state.fence;
            self.device
                .raw()
                .upgrade()
                .unwrap()
                .reset_fences(&[fence.raw()])
                .unwrap();
            self.device
                .raw()
                .upgrade()
                .unwrap()
                .queue_submit(self.device.present_queue(), &[submit_info], fence.raw())
                .unwrap();

            self.swapchain
                .present_image(
                    self.cur_swapchain_idx as u32,
                    &signal_semaphores,
                    self.device.present_queue(),
                )
                .unwrap();

            self.cur_frame_idx = (self.cur_frame_idx + 1) % self.swapchain.images.len();
        }
    }

    fn wait_for_idle(&self) {
        unsafe { self.get_device().device_wait_idle().unwrap() };
    }

    fn get_device(&self) -> Arc<ash::Device> {
        self.device.raw().upgrade().unwrap()
    }
    fn get_allocator(&self) -> Weak<vk_mem::Allocator> {
        Arc::downgrade(&self.allocator)
    }

    fn get_cur_swapchain_idx(&self) -> usize {
        self.cur_swapchain_idx
    }
    fn get_num_swapchain_images(&self) -> usize {
        self.swapchain.images.len()
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
    let image_size_bytes = fb_width * fb_height * 4;

    let window_width = 256;
    let window_height = 256;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DevSim View")
        .with_inner_size(winit::dpi::PhysicalSize::new(window_width, window_height))
        .build(&event_loop)
        .expect("Failed to create window");

    let mut renderer =
        Renderer::new(&window, fb_width, fb_height, true).expect("Failed to create renderer");

    unsafe {
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

                let fb_upload_buffer = &renderer.fb_upload_buffer;
                let p_fb_upload_buf_mem = fb_upload_buffer.info().get_mapped_data();
                let p_current_fb_upload_buf_mem = p_fb_upload_buf_mem
                    .offset((image_size_bytes * (renderer.get_cur_swapchain_idx() as u32)) as isize)
                    as *mut u8;
                let mut current_fb_upload_buf_slice = core::slice::from_raw_parts_mut(
                    p_current_fb_upload_buf_mem,
                    image_size_bytes as usize,
                );

                hw_device
                    .dump_framebuffer(&mut current_fb_upload_buf_slice)
                    .expect("Failed to dump device framebuffer!");

                let device = renderer.get_device();

                let cur_fb_image = &renderer.get_cur_frame_state().fb_image;

                // Initialize the current framebuffer image
                device.cmd_pipeline_barrier(
                    cmd_buffer,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
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
                        .image(cur_fb_image.raw())
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

                // Copy the latest device output to the framebuffer image
                let buffer_offset = (renderer.get_cur_swapchain_idx() as u32) * image_size_bytes;
                device.cmd_copy_buffer_to_image(
                    cmd_buffer,
                    fb_upload_buffer.raw(),
                    cur_fb_image.raw(),
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::builder()
                        .buffer_offset(buffer_offset as u64)
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image_extent(vk::Extent3D {
                            width: fb_width,
                            height: fb_height,
                            depth: 1,
                        })
                        .build()],
                );

                // Make sure the fb image is ready to be read by shaders
                device.cmd_pipeline_barrier(
                    cmd_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[vk::ImageMemoryBarrier::builder()
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::SHADER_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(cur_fb_image.raw())
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

                let frame_state = renderer.get_cur_frame_state();
                let descriptor_set = frame_state.descriptor_set;

                renderer.begin_render();

                device.cmd_bind_pipeline(
                    cmd_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    renderer.gfx_pipeline.raw(),
                );

                device.cmd_set_viewport(
                    cmd_buffer,
                    0,
                    &[vk::Viewport::builder()
                        .x(0.0)
                        .y(0.0)
                        .width(window.inner_size().width as f32)
                        .height(window.inner_size().height as f32)
                        .build()],
                );

                device.cmd_set_scissor(
                    cmd_buffer,
                    0,
                    &[vk::Rect2D::builder()
                        .offset(vk::Offset2D::builder().x(0).y(0).build())
                        .extent(
                            vk::Extent2D::builder()
                                .width(window.inner_size().width)
                                .height(window.inner_size().height)
                                .build(),
                        )
                        .build()],
                );

                device.cmd_bind_descriptor_sets(
                    cmd_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    renderer.pipeline_layout.raw(),
                    0,
                    &[descriptor_set],
                    &[0],
                );

                device.cmd_draw(cmd_buffer, 3, 1, 0, 0);

                renderer.end_render();

                renderer.end_frame(cmd_buffer);
            }
            Event::LoopDestroyed => {}
            _ => {
                renderer.wait_for_idle();
            }
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
