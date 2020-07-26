use crate::protobridge::{
    ProtoBridge, DEV_FB_ADDR, DEV_FB_HEIGHT, DEV_FB_WIDTH, REG_IDX_DEV_EN, WAIT_INFINITE_CYCLES,
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
        if let Object::Elf(elf) = Object::parse(&buffer)? {
            for header in elf.program_headers {
                if header.p_type == goblin::elf::program_header::PT_LOAD {
                    let program_data = &buffer
                        [header.p_offset as usize..(header.p_offset + header.p_filesz) as usize];
                    let program_addr = header.p_paddr as u16;

                    self.bridge.write_bytes(program_addr, program_data);

                    println!(
                        "Uploaded {} byte loadable program segment to address {:#06x} in device memory",
                        program_data.len(),
                        program_addr
                    );
                }
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
        // Create a new framebuffer snapshot to store the framebuffer data in
        let mut snapshot = FramebufferSnapshot {
            width: DEV_FB_WIDTH,
            height: DEV_FB_HEIGHT,
            data: vec![0; (DEV_FB_WIDTH * DEV_FB_HEIGHT) as usize],
        };

        self.bridge
            .read_bytes(DEV_FB_ADDR, &mut snapshot.data, WAIT_INFINITE_CYCLES)?;

        Ok(snapshot)
    }
}
