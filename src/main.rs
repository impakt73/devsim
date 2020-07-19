use goblin::Object;
use gumdrop::Options;
use std::error;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::ptr;

type ProtoBridgeHandle = *mut c_void;

#[repr(C)]
struct DataStatus {
    is_input_full: u8,
    is_output_empty: u8,
}

extern "C" {
    fn CreateProtoBridge(p_handle: *mut ProtoBridgeHandle) -> u32;
    fn DestroyProtoBridge(handle: ProtoBridgeHandle);

    fn QueryProtoBridgeDataStatus(handle: ProtoBridgeHandle) -> DataStatus;
    fn ClockProtoBridge(handle: ProtoBridgeHandle, p_input: *const u8, p_output: *mut u8);
}

#[repr(transparent)]
struct ProtoBridge(ProtoBridgeHandle);

const CMD_ID_READ: u8 = 1;
const CMD_ID_WRITE: u8 = 2;

const REG_IDX_DEV_EN: u16 = 0;

impl ProtoBridge {
    fn new() -> Self {
        let mut handle = ptr::null_mut();
        unsafe {
            CreateProtoBridge(&mut handle);
        }
        ProtoBridge(handle)
    }

    fn build_cmd(id: u8, addr: u16, size: u16) -> u32 {
        ((id as u32 & 0xf) << 28) | ((addr as u32 & 0x3fff) << 14) | ((size - 1) as u32 & 0x3fff)
    }

    fn build_reg_cmd(id: u8, idx: u16, data: u8) -> u32 {
        ((id as u32 & 0xf) << 28)
            | (((idx | 0x2000) as u32 & 0x3fff) << 14)
            | (data as u32 & 0x3fff)
    }

    fn send_cmd(&self, cmd: u32) -> bool {
        let mut ok = true;
        for byte_index in 0..4 {
            unsafe {
                let status = QueryProtoBridgeDataStatus(self.0);
                if status.is_input_full == 0 {
                    let byte_val = ((cmd >> (8 * byte_index)) & 0xff) as u8;
                    ClockProtoBridge(self.0, &byte_val, ptr::null_mut());
                } else {
                    ClockProtoBridge(self.0, ptr::null(), ptr::null_mut());
                    ok = false;
                    break;
                }
            }
        }
        ok
    }

    fn read_bytes(&self, addr: u16, buf: &mut [u8], max_wait_cycles: u32) -> usize {
        let mut num_wait_cycles = 0;
        let mut bytes_read = 0;

        let cmd = Self::build_cmd(CMD_ID_READ, addr, buf.len() as u16);
        if self.send_cmd(cmd) {
            while bytes_read < buf.len() {
                unsafe {
                    let status = QueryProtoBridgeDataStatus(self.0);
                    if status.is_output_empty == 0 {
                        ClockProtoBridge(self.0, ptr::null(), &mut buf[bytes_read]);
                        bytes_read += 1
                    } else {
                        ClockProtoBridge(self.0, ptr::null(), ptr::null_mut());
                        num_wait_cycles += 1;

                        if num_wait_cycles >= max_wait_cycles {
                            break;
                        }
                    }
                }
            }
        }
        bytes_read as usize
    }

    fn read_reg(&self, idx: u16) -> Option<u8> {
        let cmd = Self::build_reg_cmd(CMD_ID_READ, idx, 0);
        if self.send_cmd(cmd) {
            unsafe {
                let status = QueryProtoBridgeDataStatus(self.0);
                if status.is_output_empty == 0 {
                    let mut val = 0;
                    ClockProtoBridge(self.0, ptr::null(), &mut val);
                    return Some(val);
                } else {
                    ClockProtoBridge(self.0, ptr::null(), ptr::null_mut());
                }
            }
        }

        None
    }

    fn write_bytes(&self, addr: u16, buf: &[u8]) -> usize {
        let mut bytes_written = 0;

        let cmd = Self::build_cmd(CMD_ID_WRITE, addr, buf.len() as u16);
        if self.send_cmd(cmd) {
            while bytes_written < buf.len() {
                unsafe {
                    let status = QueryProtoBridgeDataStatus(self.0);
                    if status.is_input_full == 0 {
                        ClockProtoBridge(self.0, &buf[bytes_written], ptr::null_mut());
                        bytes_written += 1
                    } else {
                        break;
                    }
                }
            }
        }
        bytes_written as usize
    }

    fn write_reg(&self, idx: u16, data: u8) -> bool {
        let cmd = Self::build_reg_cmd(CMD_ID_WRITE, idx, data);
        self.send_cmd(cmd)
    }
}

impl Drop for ProtoBridge {
    fn drop(&mut self) {
        unsafe { DestroyProtoBridge(self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_test() {
        let memory_size = 16384;

        let bridge = ProtoBridge::new();

        let mut input_data = Vec::new();
        for i in 0..memory_size {
            input_data.push(i as u8);
        }

        assert_eq!(bridge.write_bytes(0, &input_data), memory_size);

        let mut output_data = vec![0; memory_size];

        assert_eq!(bridge.read_bytes(0, &mut output_data, 16), memory_size);

        for i in 0..memory_size {
            assert_eq!(input_data[i], output_data[i]);
        }
    }
}

#[derive(Debug, Options)]
struct SimOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(help = "path to an elf file to execute")]
    elf_path: String,
}

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;

fn main() -> Result<()> {
    let opts = SimOptions::parse_args_default_or_exit();

    let path = Path::new(&opts.elf_path);
    let buffer = fs::read(path)?;
    if let Object::Elf(elf) = Object::parse(&buffer)? {
        let bridge = ProtoBridge::new();

        for header in elf.program_headers {
            if header.p_type == goblin::elf::program_header::PT_LOAD {
                let program_data =
                    &buffer[header.p_offset as usize..(header.p_offset + header.p_filesz) as usize];
                let program_addr = header.p_paddr as u16;

                let bytes_written = bridge.write_bytes(program_addr, program_data);

                println!(
                    "Uploaded {} byte loadable program segment to address {:#06x} in device memory",
                    bytes_written, program_addr
                );
            }
        }

        let ok = bridge.write_reg(REG_IDX_DEV_EN, 1);
        assert!(ok);
        let mut current_cycle = 0;
        let max_cycles = 1024;
        loop {
            let device_enabled = bridge.read_reg(REG_IDX_DEV_EN);
            if device_enabled.is_some() && device_enabled.unwrap() == 0 {
                println!("Device stopped!");
                break;
            } else {
                println!("Device still running...");
            }
            current_cycle += 1;
            if current_cycle >= max_cycles {
                println!("Breaking out of execution loop due to timeout");
                break;
            }
        }
    } else {
        eprint!("Invalid elf input file!");
    }

    Ok(())
}
