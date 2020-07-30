use crate::protobridge::{
    ProtoBridge, REG_IDX_DEV_EN, REG_IDX_FB_ADDR, REG_IDX_FB_CONFIG, WAIT_INFINITE_CYCLES,
};
use goblin::Object;
use std::fs;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
pub struct FramebufferSnapshot {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub struct Device {
    bridge: ProtoBridge,
}

impl Device {
    /// Creates a new device
    pub fn new() -> Self {
        let bridge = ProtoBridge::new();
        Device { bridge }
    }

    /// Returns the number of clock cycles that have elapsed on the device
    pub fn clocks(&self) -> u64 {
        self.bridge.clocks()
    }

    /// Loads an elf into device memory from the path provided
    pub fn load_elf(&mut self, elf_path: impl AsRef<Path>) -> Result<()> {
        let buffer = fs::read(elf_path)?;
        match Object::parse(&buffer)? {
            Object::Elf(elf) => {
                for header in elf.program_headers {
                    if header.p_type == goblin::elf::program_header::PT_LOAD {
                        let program_data = &buffer[header.p_offset as usize
                            ..(header.p_offset + header.p_filesz) as usize];
                        let program_addr = header.p_paddr as u32;

                        self.bridge.write_bytes(program_addr, program_data);

                        println!(
                            "Uploaded {} byte loadable program segment to address {:#06x} in device memory",
                            program_data.len(),
                            program_addr
                        );
                    }
                }
            }
            _ => {
                return Err(
                    goblin::error::Error::Malformed("Invalid elf specified".to_owned()).into(),
                )
            }
        }

        Ok(())
    }

    /// Enables the device
    /// This allows the device to begin executing any code that was previously loaded into memory
    pub fn enable(&mut self) {
        // Enable the device
        self.bridge.write_reg(REG_IDX_DEV_EN, 1);
    }

    /// Disables the device
    /// This can be used to temporarily pause execution of the device.
    /// Execution can be resumed later with enable()
    pub fn disable(&mut self) {
        // Disable the device
        self.bridge.write_reg(REG_IDX_DEV_EN, 0);
    }

    /// Queries the device to determine if it's still executing
    pub fn query_is_halted(&mut self) -> Result<bool> {
        match self.bridge.read_reg(REG_IDX_DEV_EN, WAIT_INFINITE_CYCLES) {
            Ok(reg) => {
                if reg != 0 {
                    // Still executing...
                    Ok(false)
                } else {
                    // Execution stopped
                    Ok(true)
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn dump_framebuffer(&mut self) -> Result<FramebufferSnapshot> {
        let fb_addr = self
            .bridge
            .read_reg(REG_IDX_FB_ADDR, WAIT_INFINITE_CYCLES)
            .expect("Failed to read framebuffer address from device!");
        let fb_config = self
            .bridge
            .read_reg(REG_IDX_FB_CONFIG, WAIT_INFINITE_CYCLES)
            .expect("Failed to read framebuffer config from device!");
        println!("FB_ADDR: {}, FB_CONFIG: {}", fb_addr, fb_config);

        let fb_width = 1 << ((fb_config & 0x7) + 1);
        let fb_height = 1 << (((fb_config >> 3) & 0x7) + 1);

        // Create a new framebuffer snapshot to store the framebuffer data in
        let mut snapshot = FramebufferSnapshot {
            width: fb_width,
            height: fb_height,
            data: vec![0; (fb_width * fb_height) as usize],
        };

        self.bridge
            .read_bytes(fb_addr, &mut snapshot.data, WAIT_INFINITE_CYCLES)?;

        Ok(snapshot)
    }
}

impl Default for Device {
    fn default() -> Self {
        Self::new()
    }
}
