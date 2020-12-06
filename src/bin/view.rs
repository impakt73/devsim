use ash::{
    version::{DeviceV1_0, InstanceV1_0},
    vk,
};
use imgui::{DrawCmd, DrawCmdParams};
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use std::sync::{Arc, Weak};
use std::time::Instant;
use winit::{
    event::{
        DeviceEvent, ElementState, Event, KeyboardInput, StartCause, VirtualKeyCode, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

use clap::Clap;
use devsim::vkutil::*;
use imgui::internal::RawWrapper;
use std::io;
use std::io::Write;
use std::path::Path;
use std::slice;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Utility structure that simplifies the process of writing data that's constant over a single frame into GPU memory
struct ConstantDataWriter {
    buffer: *mut u8,
    buffer_size: usize,
    bytes_written: usize,
}

impl ConstantDataWriter {
    pub fn new(buffer: *mut u8, buffer_size: usize) -> Self {
        ConstantDataWriter {
            buffer,
            buffer_size,
            bytes_written: 0,
        }
    }

    pub fn dword_offset(&self) -> u32 {
        (self.bytes_written / 4) as u32
    }
}

impl io::Write for ConstantDataWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_remaining = self.buffer_size - self.bytes_written;
        let bytes_written = if buf.len() <= bytes_remaining {
            buf.len()
        } else {
            bytes_remaining
        };

        let buffer = unsafe {
            slice::from_raw_parts_mut(self.buffer.add(self.bytes_written), bytes_remaining)
        };
        buffer[..bytes_written].clone_from_slice(&buf[..bytes_written]);

        self.bytes_written += bytes_written;

        Ok(bytes_written as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Selects a physical device from the provided list
fn select_physical_device(physical_devices: &[vk::PhysicalDevice]) -> vk::PhysicalDevice {
    // TODO: Support proper physical device selection
    //       For now, we just use the first device
    physical_devices[0]
}

/// Size of the scratch memory buffer in bytes that is available to each frame
const FRAME_MEMORY_SIZE: u64 = 8 * 1024 * 1024;

/// Number of individual texture slots available to shaders during a frame
const NUM_TEXTURE_SLOTS: u64 = 64;

/// Texture slot index associated with the imgui font
const IMGUI_FONT_TEXTURE_SLOT_INDEX: u64 = NUM_TEXTURE_SLOTS - 1;

struct FrameState {
    #[allow(dead_code)]
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
        frame_memory_buffer: &VkBuffer,
        fb_width: u32,
        fb_height: u32,
        imgui_renderer: &ImguiRenderer,
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
                    .dst_binding(0)
                    .dst_array_element(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
                    .buffer_info(&[vk::DescriptorBufferInfo::builder()
                        .buffer(frame_memory_buffer.raw())
                        .offset(0)
                        .range(FRAME_MEMORY_SIZE)
                        .build()])
                    .build()],
                &[],
            );

            let mut image_infos = (0..(NUM_TEXTURE_SLOTS - 1))
                .map(|_| {
                    vk::DescriptorImageInfo::builder()
                        .image_view(fb_image_view.raw())
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .build()
                })
                .collect::<Vec<_>>();
            image_infos.push(
                vk::DescriptorImageInfo::builder()
                    .image_view(imgui_renderer.font_atlas_image_view.raw())
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build(),
            );
            device.raw().upgrade().unwrap().update_descriptor_sets(
                &[vk::WriteDescriptorSet::builder()
                    .dst_set(descriptor_set)
                    .dst_binding(2)
                    .dst_array_element(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&image_infos)
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
    #[allow(dead_code)]
    imgui_renderer: ImguiRenderer,
    frame_states: Vec<FrameState>,
    fb_upload_buffer: VkBuffer,
    frame_memory_buffer: VkBuffer,
    image_available_semaphores: Vec<VkSemaphore>,
    framebuffers: Vec<VkFramebuffer>,
    renderpass: VkRenderPass,
    #[allow(dead_code)]
    cmd_pool: VkCommandPool,
    #[allow(dead_code)]
    sampler: VkSampler,
    pipeline_layout: VkPipelineLayout,
    #[allow(dead_code)]
    descriptor_set_layout: VkDescriptorSetLayout,
    #[allow(dead_code)]
    descriptor_pool: VkDescriptorPool,
    gfx_pipeline: VkPipeline,
    imgui_pipeline: VkPipeline,
    #[allow(dead_code)]
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
        context: &mut imgui::Context,
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
            None,
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
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
                    .descriptor_count(1)
                    .stage_flags(
                        vk::ShaderStageFlags::VERTEX
                            | vk::ShaderStageFlags::FRAGMENT
                            | vk::ShaderStageFlags::COMPUTE,
                    )
                    .build(),
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::COMPUTE)
                    .immutable_samplers(&[sampler.raw()])
                    .build(),
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(NUM_TEXTURE_SLOTS as u32)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::COMPUTE)
                    .build(),
            ]),
        )?;

        let pipeline_layout = VkPipelineLayout::new(
            device.raw(),
            &vk::PipelineLayoutCreateInfo::builder()
                .set_layouts(&[descriptor_set_layout.raw()])
                .push_constant_ranges(&[vk::PushConstantRange::builder()
                    .offset(0)
                    .size((4 * std::mem::size_of::<u32>()) as u32)
                    .stage_flags(
                        vk::ShaderStageFlags::VERTEX
                            | vk::ShaderStageFlags::FRAGMENT
                            | vk::ShaderStageFlags::COMPUTE,
                    )
                    .build()]),
        )?;

        let descriptor_pool = VkDescriptorPool::new(
            device.raw(),
            &vk::DescriptorPoolCreateInfo::builder()
                .max_sets(desired_image_count)
                .pool_sizes(&[
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
                        .descriptor_count(desired_image_count)
                        .build(),
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::SAMPLER)
                        .descriptor_count(desired_image_count)
                        .build(),
                    vk::DescriptorPoolSize::builder()
                        .ty(vk::DescriptorType::SAMPLED_IMAGE)
                        .descriptor_count(desired_image_count * (NUM_TEXTURE_SLOTS as u32))
                        .build(),
                ]),
        )?;

        let mut compiler = shaderc::Compiler::new().expect("Failed to create compiler");

        let vert_source = include_str!("../shaders/FullscreenPass.vert");
        let frag_source = include_str!("../shaders/CopyTexture.frag");
        let mut compile_options = shaderc::CompileOptions::new().unwrap();
        let shader_dir = std::env::current_dir().unwrap().join("src/shaders");
        compile_options.set_include_callback(move |name, _inc_type, _parent_name, _depth| {
            let path = shader_dir.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                Ok(shaderc::ResolvedInclude {
                    resolved_name: String::from(name),
                    content,
                })
            } else {
                Err(format!(
                    "Failed to load included shader code from {}.",
                    name
                ))
            }
        });

        let vert_result = compiler.compile_into_spirv(
            vert_source,
            shaderc::ShaderKind::Vertex,
            "FullscreenPass.vert",
            "main",
            Some(&compile_options),
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
            Some(&compile_options),
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

        let imgui_vert_source = include_str!("../shaders/ImguiTriangle.vert");
        let imgui_frag_source = include_str!("../shaders/ImguiTriangle.frag");

        let imgui_vert_result = compiler.compile_into_spirv(
            imgui_vert_source,
            shaderc::ShaderKind::Vertex,
            "ImguiTriangle.vert",
            "main",
            Some(&compile_options),
        )?;

        let imgui_vert_module = VkShaderModule::new(
            device.raw(),
            &vk::ShaderModuleCreateInfo::builder().code(imgui_vert_result.as_binary()),
        )?;

        let imgui_frag_result = compiler.compile_into_spirv(
            imgui_frag_source,
            shaderc::ShaderKind::Fragment,
            "ImguiTriangle.frag",
            "main",
            Some(&compile_options),
        )?;

        let imgui_frag_module = VkShaderModule::new(
            device.raw(),
            &vk::ShaderModuleCreateInfo::builder().code(imgui_frag_result.as_binary()),
        )?;

        let imgui_entry_point_c_string = std::ffi::CString::new("main").unwrap();
        let imgui_pipeline = pipeline_cache.create_graphics_pipeline(
            &vk::GraphicsPipelineCreateInfo::builder()
                .stages(&[
                    vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(imgui_vert_module.raw())
                        .name(imgui_entry_point_c_string.as_c_str())
                        .build(),
                    vk::PipelineShaderStageCreateInfo::builder()
                        .stage(vk::ShaderStageFlags::FRAGMENT)
                        .module(imgui_frag_module.raw())
                        .name(imgui_entry_point_c_string.as_c_str())
                        .build(),
                ])
                .input_assembly_state(
                    &vk::PipelineInputAssemblyStateCreateInfo::builder()
                        .topology(vk::PrimitiveTopology::TRIANGLE_LIST),
                )
                .vertex_input_state(
                    &vk::PipelineVertexInputStateCreateInfo::builder()
                        .vertex_binding_descriptions(&[vk::VertexInputBindingDescription::builder(
                        )
                        .binding(0)
                        .stride(std::mem::size_of::<imgui::DrawVert>() as u32)
                        .input_rate(vk::VertexInputRate::VERTEX)
                        .build()])
                        .vertex_attribute_descriptions(&[
                            vk::VertexInputAttributeDescription::builder()
                                .location(0)
                                .binding(0)
                                .format(vk::Format::R32G32_SFLOAT)
                                .offset(0)
                                .build(),
                            vk::VertexInputAttributeDescription::builder()
                                .location(1)
                                .binding(0)
                                .format(vk::Format::R32G32_SFLOAT)
                                .offset(8)
                                .build(),
                            vk::VertexInputAttributeDescription::builder()
                                .location(2)
                                .binding(0)
                                .format(vk::Format::R32_UINT)
                                .offset(16)
                                .build(),
                        ]),
                )
                .viewport_state(
                    &vk::PipelineViewportStateCreateInfo::builder()
                        .viewports(&[vk::Viewport::default()])
                        .scissors(&[vk::Rect2D::default()]),
                )
                .rasterization_state(
                    &vk::PipelineRasterizationStateCreateInfo::builder()
                        .polygon_mode(vk::PolygonMode::FILL)
                        .cull_mode(vk::CullModeFlags::NONE)
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
                            .blend_enable(true)
                            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                            .color_blend_op(vk::BlendOp::ADD)
                            .src_alpha_blend_factor(vk::BlendFactor::ONE)
                            .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                            .alpha_blend_op(vk::BlendOp::ADD)
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

        let frame_memory_buffer = VkBuffer::new(
            Arc::downgrade(&allocator),
            &ash::vk::BufferCreateInfo::builder()
                .size(FRAME_MEMORY_SIZE * (desired_image_count as u64))
                .usage(vk::BufferUsageFlags::STORAGE_BUFFER),
            &vk_mem::AllocationCreateInfo {
                usage: vk_mem::MemoryUsage::CpuToGpu,
                flags: vk_mem::AllocationCreateFlags::MAPPED,
                ..Default::default()
            },
        )?;

        let imgui_renderer = ImguiRenderer::new(&device, Arc::downgrade(&allocator), context)?;

        let frame_states = swapchain_image_views
            .iter()
            .map(|_image_view| {
                FrameState::new(
                    &device,
                    Arc::downgrade(&allocator),
                    &cmd_pool,
                    &descriptor_pool,
                    &descriptor_set_layout,
                    &frame_memory_buffer,
                    fb_width,
                    fb_height,
                    &imgui_renderer,
                )
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Renderer {
            imgui_renderer,
            frame_states,
            frame_memory_buffer,
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
            imgui_pipeline,
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

    fn recreate_swapchain(&mut self, window: &winit::window::Window) -> Result<()> {
        println!(
            "Recreating {}x{} swapchain!",
            window.inner_size().width,
            window.inner_size().height
        );

        // Make sure all previous rendering work is completed before we destroy the old swapchain resources
        self.wait_for_idle();

        let swapchain = VkSwapchain::new(
            &self.instance,
            &self.surface,
            &self.device,
            window.inner_size().width,
            window.inner_size().height,
            Some(&self.swapchain),
        )?;

        let surface_format = swapchain.surface_format;
        let surface_resolution = swapchain.surface_resolution;

        let swapchain_image_views = swapchain
            .images
            .iter()
            .map(|image| {
                VkImageView::new(
                    self.device.raw(),
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

        let framebuffers = swapchain_image_views
            .iter()
            .map(|image_view| {
                VkFramebuffer::new(
                    self.device.raw(),
                    &vk::FramebufferCreateInfo::builder()
                        .render_pass(self.renderpass.raw())
                        .attachments(&[image_view.raw()])
                        .width(surface_resolution.width)
                        .height(surface_resolution.height)
                        .layers(1),
                )
            })
            .collect::<Result<Vec<_>>>()?;

        self.swapchain_image_views = swapchain_image_views;
        self.framebuffers = framebuffers;
        self.swapchain = swapchain;

        Ok(())
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
            // TODO: This should never happen since we're already handling window resize events, but this could be handled
            // more robustly in the future.
            assert!(!_is_suboptimal);
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

            let _is_suboptimal = self
                .swapchain
                .present_image(
                    self.cur_swapchain_idx as u32,
                    &signal_semaphores,
                    self.device.present_queue(),
                )
                .unwrap();
            // TODO: This should never happen since we're already handling window resize events, but this could be handled
            // more robustly in the future.
            assert!(!_is_suboptimal);

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

struct ImguiRenderer {
    #[allow(dead_code)]
    font_atlas_image: VkImage,
    font_atlas_image_view: VkImageView,
}

impl ImguiRenderer {
    fn new(
        device: &VkDevice,
        allocator: Weak<vk_mem::Allocator>,
        context: &mut imgui::Context,
    ) -> Result<Self> {
        let font_atlas_image;
        let font_atlas_image_view;
        {
            let mut context_fonts = context.fonts();
            let font_atlas = context_fonts.build_alpha8_texture();

            font_atlas_image = VkImage::new(
                allocator.clone(),
                &ash::vk::ImageCreateInfo::builder()
                    .image_type(vk::ImageType::TYPE_2D)
                    .extent(vk::Extent3D {
                        width: font_atlas.width,
                        height: font_atlas.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .format(vk::Format::R8_UNORM)
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
            font_atlas_image_view = VkImageView::new(
                device.raw(),
                &vk::ImageViewCreateInfo::builder()
                    .image(font_atlas_image.raw())
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8_UNORM)
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

            let cmd_pool = VkCommandPool::new(
                device.raw(),
                &vk::CommandPoolCreateInfo::builder()
                    .queue_family_index(device.graphics_queue_family_index() as u32),
            )?;
            let cmd_buffer = cmd_pool.allocate_command_buffer(vk::CommandBufferLevel::PRIMARY)?;
            unsafe {
                let raw_device = device.raw().upgrade().unwrap();
                raw_device.begin_command_buffer(
                    cmd_buffer,
                    &vk::CommandBufferBeginInfo::builder()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                        .build(),
                )?;

                // TODO: It would be faster to upload this with the transfer queue, but it would significantly increase
                //       the complexity of the upload process here. Replace this with a more standardized resource
                //       upload process when it becomes available.
                raw_device.cmd_pipeline_barrier(
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
                        .image(font_atlas_image.raw())
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

                let atlas_buffer_size =
                    ((font_atlas.width * font_atlas.height) as usize) * std::mem::size_of::<u8>();
                let atlas_buffer = VkBuffer::new(
                    allocator,
                    &ash::vk::BufferCreateInfo::builder()
                        .size(atlas_buffer_size as u64)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC),
                    &vk_mem::AllocationCreateInfo {
                        usage: vk_mem::MemoryUsage::CpuToGpu,
                        flags: vk_mem::AllocationCreateFlags::MAPPED,
                        ..Default::default()
                    },
                )?;

                let atlas_data_src = font_atlas.data.as_ptr();
                let atlas_data_dst = atlas_buffer.info().get_mapped_data();
                core::ptr::copy_nonoverlapping(atlas_data_src, atlas_data_dst, atlas_buffer_size);

                raw_device.cmd_copy_buffer_to_image(
                    cmd_buffer,
                    atlas_buffer.raw(),
                    font_atlas_image.raw(),
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::builder()
                        .buffer_offset(0)
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        })
                        .image_extent(vk::Extent3D {
                            width: font_atlas.width,
                            height: font_atlas.height,
                            depth: 1,
                        })
                        .build()],
                );

                raw_device.cmd_pipeline_barrier(
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
                        .image(font_atlas_image.raw())
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

                raw_device.end_command_buffer(cmd_buffer)?;
                raw_device.queue_submit(
                    device.graphics_queue(),
                    &[vk::SubmitInfo::builder()
                        .command_buffers(&[cmd_buffer])
                        .build()],
                    vk::Fence::null(),
                )?;
                raw_device.queue_wait_idle(device.graphics_queue())?;
            }
        }

        context.fonts().tex_id = imgui::TextureId::from(IMGUI_FONT_TEXTURE_SLOT_INDEX as usize);

        Ok(ImguiRenderer {
            font_atlas_image,
            font_atlas_image_view,
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
enum SimulationState {
    Running,
    Paused,
}

/// Simulation control object
/// This object is used to simplify interactions with the underlying device simulation code
/// TODO: The lower level interface should be cleaned up to avoid the need to recreate the device during resets.
struct Simulation {
    device: Option<devsim::device::Device>,
    elf_path: Option<String>,
    state: SimulationState,
    fb_width: u32,
    fb_height: u32,
}

impl Simulation {
    fn new() -> Result<Self> {
        // Create a device so we can query the framebuffer size
        let mut device = devsim::device::Device::new();
        let (fb_width, fb_height) = device.query_framebuffer_size()?;

        Ok(Self {
            device: None,
            elf_path: None,
            state: SimulationState::Running,
            fb_width,
            fb_height,
        })
    }

    /// Loads an ELF file from the provided path into the simulator
    fn load_elf(&mut self, path: &impl AsRef<Path>) -> Result<()> {
        self.elf_path = Some(path.as_ref().to_str().unwrap().to_string());
        self.reset()?;

        Ok(())
    }

    /// Resets the simulator and reloads the current ELF file if there is one
    fn reset(&mut self) -> Result<()> {
        if let Some(path) = &self.elf_path {
            let mut device = devsim::device::Device::new();
            device.load_elf(path)?;

            self.device = Some(device);
        }

        Ok(())
    }

    /// Returns the width of the framebuffer image inside the device
    fn framebuffer_width(&self) -> u32 {
        self.fb_width
    }

    /// Returns the height of the framebuffer image inside the device
    fn framebuffer_height(&self) -> u32 {
        self.fb_height
    }

    /// Returns the size in bytes of the framebuffer image inside the device
    fn framebuffer_size(&self) -> usize {
        (self.fb_width * self.fb_height * 4) as usize
    }

    /// Pauses the simulator so that future calls to update don't trigger any work
    fn pause(&mut self) {
        if self.state == SimulationState::Running {
            self.state = SimulationState::Paused;
        }
    }

    /// Resumes the simulator so that future calls to update trigger simulation work
    fn resume(&mut self) {
        if self.state == SimulationState::Paused {
            self.state = SimulationState::Running;
        }
    }

    /// If the simulator is running, pause it. Otherwise, resume it
    fn toggle(&mut self) {
        if self.is_running() {
            self.pause();
        } else {
            self.resume();
        }
    }

    /// Returns true if the simulator is running
    fn is_running(&self) -> bool {
        self.state == SimulationState::Running
    }

    /// Updates the simulation state if the simulation state is currently valid and returns the framebuffer data
    /// via the provided slice. The slice should be large enough to hold the framebuffer data from the device.
    fn update(&mut self, fb_data: &mut [u8]) {
        if let Some(device) = &mut self.device {
            // We only want to update the actual device simulation if the simulation is currently running
            if self.state == SimulationState::Running {
                device.enable();
                loop {
                    match device.query_is_halted() {
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
            }

            // The framebuffer data from the device needs to be dumped regardless of the current simulation state
            device
                .dump_framebuffer(fb_data)
                .expect("Failed to dump device framebuffer!");
        }
    }
}

/// Shows the simulation window with the provided options
fn show(opts: &SimOptions) -> ! {
    let mut sim = Simulation::new().expect("Failed to create simulation");
    if let Some(elf_path) = &opts.elf_path {
        sim.load_elf(elf_path).expect("Failed to load elf");
    }

    let window_width = 1280;
    let window_height = 720;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("DevSim View")
        .with_inner_size(winit::dpi::PhysicalSize::new(window_width, window_height))
        .build(&event_loop)
        .expect("Failed to create window");

    let mut context = imgui::Context::create();
    context.set_renderer_name(Some(imgui::ImString::from(String::from("DevSim"))));
    context
        .io_mut()
        .backend_flags
        .insert(imgui::BackendFlags::RENDERER_HAS_VTX_OFFSET);

    let mut platform = WinitPlatform::init(&mut context);
    platform.attach_window(context.io_mut(), &window, HiDpiMode::Default);

    let mut renderer = Renderer::new(
        &window,
        sim.framebuffer_width(),
        sim.framebuffer_height(),
        true,
        &mut context,
    )
    .expect("Failed to create renderer");

    unsafe {
        // TODO: Find a better way to initialize these vectors
        let mut imgui_vtx_buffers = Vec::new();
        for _i in 0..renderer.get_num_swapchain_images() {
            imgui_vtx_buffers.push(None);
        }
        let mut imgui_idx_buffers = Vec::new();
        for _i in 0..renderer.get_num_swapchain_images() {
            imgui_idx_buffers.push(None);
        }

        let mut last_frame = Instant::now();
        event_loop.run(move |event, _, control_flow| {
            platform.handle_event(context.io_mut(), &window, &event);
            match event {
                Event::NewEvents(StartCause::Init) => {
                    *control_flow = ControlFlow::Poll;
                }
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    WindowEvent::Resized(_new_size) => {
                        // TODO: This code needs to be updated to properly handle minimized windows
                        //       When a window is minimized, it resizes to 0x0 which causes all sorts of problems
                        //       inside the graphics api. This basically results in crashes on minimize. :/
                        //       This will be fixed in a future change.
                        renderer.recreate_swapchain(&window).unwrap();
                    }
                    WindowEvent::DroppedFile(path) => {
                        sim.load_elf(&path).expect("Failed to load elf");
                    }
                    _ => {}
                },
                Event::MainEventsCleared => {
                    let cmd_buffer = renderer.begin_frame();

                    let now = Instant::now();
                    context.io_mut().update_delta_time(now - last_frame);
                    last_frame = now;

                    platform
                        .prepare_frame(context.io_mut(), &window)
                        .expect("Failed to prepare frame");

                    let ui = context.frame();

                    let fb_upload_buffer = &renderer.fb_upload_buffer;
                    let p_fb_upload_buf_mem = fb_upload_buffer.info().get_mapped_data();
                    let p_current_fb_upload_buf_mem = p_fb_upload_buf_mem
                        .add(sim.framebuffer_size() * renderer.get_cur_swapchain_idx())
                        as *mut u8;
                    let mut current_fb_upload_buf_slice = core::slice::from_raw_parts_mut(
                        p_current_fb_upload_buf_mem,
                        sim.framebuffer_size(),
                    );

                    sim.update(&mut current_fb_upload_buf_slice);

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
                    let buffer_offset = renderer.get_cur_swapchain_idx() * sim.framebuffer_size();
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
                                width: sim.framebuffer_width(),
                                height: sim.framebuffer_height(),
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

                    let constant_data_offset =
                        renderer.get_cur_swapchain_idx() * (FRAME_MEMORY_SIZE as usize);

                    device.cmd_bind_descriptor_sets(
                        cmd_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        renderer.pipeline_layout.raw(),
                        0,
                        &[descriptor_set],
                        &[constant_data_offset as u32],
                    );

                    let mut constant_writer = ConstantDataWriter::new(
                        renderer
                            .frame_memory_buffer
                            .info()
                            .get_mapped_data()
                            .add(constant_data_offset),
                        FRAME_MEMORY_SIZE as usize,
                    );

                    device.cmd_draw(cmd_buffer, 3, 1, 0, 0);

                    if let Some(main_menu_bar) = ui.begin_main_menu_bar() {
                        if let Some(file_menu) = ui.begin_menu(imgui::im_str!("File"), true) {
                            if imgui::MenuItem::new(imgui::im_str!("Exit")).build(&ui) {
                                *control_flow = ControlFlow::Exit
                            }

                            file_menu.end(&ui);
                        }
                        if let Some(simulation_menu) =
                            ui.begin_menu(imgui::im_str!("Simulation"), true)
                        {
                            if imgui::MenuItem::new(imgui::im_str!("Reset")).build(&ui) {
                                sim.reset().expect("Failed to reset simulation");
                            }

                            let toggle_string = if sim.is_running() {
                                imgui::im_str!("Pause")
                            } else {
                                imgui::im_str!("Resume")
                            };
                            if imgui::MenuItem::new(toggle_string).build(&ui) {
                                sim.toggle();
                            }

                            simulation_menu.end(&ui);
                        }
                        main_menu_bar.end(&ui);
                    }

                    platform.prepare_render(&ui, &window);
                    let draw_data = ui.render();

                    let fb_width = draw_data.display_size[0] * draw_data.framebuffer_scale[0];
                    let fb_height = draw_data.display_size[1] * draw_data.framebuffer_scale[1];
                    if (fb_width > 0.0) && (fb_height > 0.0) && draw_data.total_idx_count > 0 {
                        let total_vtx_count = draw_data.total_vtx_count as usize;
                        let vtx_buffer_size =
                            total_vtx_count * std::mem::size_of::<imgui::DrawVert>();
                        let vtx_buffer = VkBuffer::new(
                            renderer.get_allocator(),
                            &ash::vk::BufferCreateInfo::builder()
                                .size(vtx_buffer_size as u64)
                                .usage(vk::BufferUsageFlags::VERTEX_BUFFER),
                            &vk_mem::AllocationCreateInfo {
                                usage: vk_mem::MemoryUsage::CpuToGpu,
                                flags: vk_mem::AllocationCreateFlags::MAPPED,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        let vtx_buffer_slice = slice::from_raw_parts_mut(
                            vtx_buffer.info().get_mapped_data(),
                            vtx_buffer_size,
                        );
                        let vtx_buffer_raw = vtx_buffer.raw();

                        let total_idx_count = draw_data.total_idx_count as usize;
                        let idx_buffer_size =
                            total_idx_count * std::mem::size_of::<imgui::DrawIdx>();
                        let idx_buffer = VkBuffer::new(
                            renderer.get_allocator(),
                            &ash::vk::BufferCreateInfo::builder()
                                .size(idx_buffer_size as u64)
                                .usage(vk::BufferUsageFlags::INDEX_BUFFER),
                            &vk_mem::AllocationCreateInfo {
                                usage: vk_mem::MemoryUsage::CpuToGpu,
                                flags: vk_mem::AllocationCreateFlags::MAPPED,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                        let idx_buffer_slice = slice::from_raw_parts_mut(
                            idx_buffer.info().get_mapped_data(),
                            idx_buffer_size,
                        );
                        let idx_buffer_raw = idx_buffer.raw();

                        let mut vtx_bytes_written: usize = 0;
                        let mut vtx_buffer_offsets = Vec::new();

                        let mut idx_bytes_written: usize = 0;
                        let mut idx_buffer_offsets = Vec::new();

                        for draw_list in draw_data.draw_lists() {
                            let vtx_data_src = draw_list.vtx_buffer().as_ptr() as *const u8;
                            let vtx_data_dst =
                                (vtx_buffer_slice.as_mut_ptr() as *mut u8).add(vtx_bytes_written);
                            let vtx_data_size = draw_list.vtx_buffer().len()
                                * std::mem::size_of::<imgui::DrawVert>();
                            core::ptr::copy_nonoverlapping(
                                vtx_data_src,
                                vtx_data_dst,
                                vtx_data_size,
                            );
                            vtx_buffer_offsets.push(vtx_bytes_written);
                            vtx_bytes_written += vtx_data_size;

                            let idx_data_src = draw_list.idx_buffer().as_ptr() as *const u8;
                            let idx_data_dst =
                                (idx_buffer_slice.as_mut_ptr() as *mut u8).add(idx_bytes_written);
                            let idx_data_size = draw_list.idx_buffer().len()
                                * std::mem::size_of::<imgui::DrawIdx>();
                            core::ptr::copy_nonoverlapping(
                                idx_data_src,
                                idx_data_dst,
                                idx_data_size,
                            );
                            idx_buffer_offsets.push(idx_bytes_written);
                            idx_bytes_written += idx_data_size;
                        }

                        imgui_vtx_buffers[renderer.get_cur_swapchain_idx()] = Some(vtx_buffer);
                        imgui_idx_buffers[renderer.get_cur_swapchain_idx()] = Some(idx_buffer);

                        device.cmd_bind_pipeline(
                            cmd_buffer,
                            vk::PipelineBindPoint::GRAPHICS,
                            renderer.imgui_pipeline.raw(),
                        );

                        let fb_scale = draw_data.framebuffer_scale;
                        device.cmd_set_viewport(
                            cmd_buffer,
                            0,
                            &[vk::Viewport::builder()
                                .x(draw_data.display_pos[0] * fb_scale[0])
                                .y(draw_data.display_pos[1] * fb_scale[1])
                                .width(draw_data.display_size[0] * fb_scale[0])
                                .height(draw_data.display_size[1] * fb_scale[1])
                                .build()],
                        );

                        let clip_off = draw_data.display_pos;
                        let clip_scale = draw_data.framebuffer_scale;

                        let left = draw_data.display_pos[0];
                        let right = draw_data.display_pos[0] + draw_data.display_size[0];
                        let top = draw_data.display_pos[1];
                        let bottom = draw_data.display_pos[1] + draw_data.display_size[1];
                        let matrix = [
                            [(2.0 / (right - left)), 0.0, 0.0, 0.0],
                            [0.0, (2.0 / (top - bottom)), 0.0, 0.0],
                            [0.0, 0.0, -1.0, 0.0],
                            [
                                (right + left) / (left - right),
                                (top + bottom) / (bottom - top),
                                0.0,
                                1.0,
                            ],
                        ];

                        // Identify the current constant buffer offset before we write any new data into it
                        let dword_offset = constant_writer.dword_offset();

                        // Write the imgui matrix into the buffer
                        for row in &matrix {
                            for val in row {
                                constant_writer.write_all(&val.to_le_bytes()).unwrap();
                            }
                        }

                        for (idx, draw_list) in draw_data.draw_lists().enumerate() {
                            device.cmd_bind_vertex_buffers(
                                cmd_buffer,
                                0,
                                &[vtx_buffer_raw],
                                &[vtx_buffer_offsets[idx] as u64],
                            );

                            device.cmd_bind_index_buffer(
                                cmd_buffer,
                                idx_buffer_raw,
                                idx_buffer_offsets[idx] as u64,
                                vk::IndexType::UINT16,
                            );

                            for cmd in draw_list.commands() {
                                match cmd {
                                    DrawCmd::Elements {
                                        count,
                                        cmd_params:
                                            DrawCmdParams {
                                                clip_rect,
                                                texture_id,
                                                vtx_offset,
                                                idx_offset,
                                            },
                                    } => {
                                        let clip_rect = [
                                            (clip_rect[0] - clip_off[0]) * clip_scale[0],
                                            (clip_rect[1] - clip_off[1]) * clip_scale[1],
                                            (clip_rect[2] - clip_off[0]) * clip_scale[0],
                                            (clip_rect[3] - clip_off[1]) * clip_scale[1],
                                        ];

                                        if clip_rect[0] < fb_width
                                            && clip_rect[1] < fb_height
                                            && clip_rect[2] >= 0.0
                                            && clip_rect[3] >= 0.0
                                        {
                                            let scissor_x =
                                                f32::max(0.0, clip_rect[0]).floor() as i32;
                                            let scissor_y =
                                                f32::max(0.0, clip_rect[1]).floor() as i32;
                                            let scissor_w =
                                                (clip_rect[2] - clip_rect[0]).abs().ceil() as u32;
                                            let scissor_h =
                                                (clip_rect[3] - clip_rect[1]).abs().ceil() as u32;

                                            device.cmd_set_scissor(
                                                cmd_buffer,
                                                0,
                                                &[vk::Rect2D::builder()
                                                    .offset(
                                                        vk::Offset2D::builder()
                                                            .x(scissor_x)
                                                            .y(scissor_y)
                                                            .build(),
                                                    )
                                                    .extent(
                                                        vk::Extent2D::builder()
                                                            .width(scissor_w)
                                                            .height(scissor_h)
                                                            .build(),
                                                    )
                                                    .build()],
                                            );

                                            // The texture slot index is stored inside the ImGui texture id
                                            let texture_index: u32 = texture_id.id() as u32;
                                            let push_constant_0 = ((texture_index & 0xff) << 24)
                                                | (dword_offset & 0x00ffffff);
                                            device.cmd_push_constants(
                                                cmd_buffer,
                                                renderer.pipeline_layout.raw(),
                                                vk::ShaderStageFlags::VERTEX
                                                    | vk::ShaderStageFlags::FRAGMENT
                                                    | vk::ShaderStageFlags::COMPUTE,
                                                0,
                                                &push_constant_0.to_le_bytes(),
                                            );

                                            device.cmd_draw_indexed(
                                                cmd_buffer,
                                                count as u32,
                                                1,
                                                idx_offset as u32,
                                                vtx_offset as i32,
                                                0,
                                            );
                                        }
                                    }
                                    DrawCmd::ResetRenderState => (), // NOTE: This doesn't seem necessary given how pipelines work?
                                    DrawCmd::RawCallback { callback, raw_cmd } => {
                                        callback(draw_list.raw(), raw_cmd)
                                    }
                                }
                            }
                        }
                    }

                    renderer.end_render();

                    renderer.end_frame(cmd_buffer);
                }
                Event::LoopDestroyed => {
                    renderer.wait_for_idle();

                    imgui_vtx_buffers.clear();
                    imgui_idx_buffers.clear();
                }
                event => match event {
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
                    _ => {}
                },
            }
        });
    }
}

#[derive(Debug, Clap)]
#[clap(version)]
struct SimOptions {
    /// Path to a RISC-V elf to execute
    elf_path: Option<String>,
}

fn main() {
    let opts = SimOptions::parse();
    show(&opts);
}
