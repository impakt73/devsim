use goblin::Object;
use gumdrop::Options;
use std::error;
use std::ffi::c_void;
use std::fs;
use std::path::Path;
use std::ptr;
use std::collections::VecDeque;
use std::io;
use std::io::{Read, Write};
use std::cmp;

type ProtoBridgeHandle = *mut c_void;

const WAIT_INFINITE_CYCLES: usize = 0xffffffff;

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

struct ProtoBridge {
    handle: ProtoBridgeHandle,
    clocks: u64,
    input_queue: VecDeque<u8>,
    output_queue: VecDeque<u8>,
}

const CMD_ID_READ: u8 = 1;
const CMD_ID_WRITE: u8 = 2;

const REG_IDX_DEV_EN: u16 = 0;

impl io::Read for ProtoBridge {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = cmp::min(self.output_queue.len(), buf.len());
        for (index, byte) in self.output_queue.drain(0..bytes_read).enumerate() {
            buf[index] = byte;
        }
        Ok(bytes_read)
    }
}

impl io::Write for ProtoBridge {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.input_queue.extend(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ProtoBridge {
    fn new() -> Self {
        let mut handle = ptr::null_mut();
        unsafe {
            CreateProtoBridge(&mut handle);
        }
        ProtoBridge {
            handle,
            clocks: 0,
            input_queue: VecDeque::new(),
            output_queue: VecDeque::new()
        }
    }

    fn clocks(&self) -> u64 {
        self.clocks
    }

    // Internal helper functions
    fn build_cmd(id: u8, addr: u16, size: u16) -> u32 {
        ((id as u32 & 0xf) << 28) | ((addr as u32 & 0x3fff) << 14) | ((size - 1) as u32 & 0x3fff)
    }

    fn build_reg_cmd(id: u8, idx: u16, data: u8) -> u32 {
        ((id as u32 & 0xf) << 28)
            | (((idx | 0x2000) as u32 & 0x3fff) << 14)
            | (data as u32 & 0x3fff)
    }

    fn clock(&mut self) {
        let status = unsafe { QueryProtoBridgeDataStatus(self.handle) };

        let mut p_in = ptr::null();
        let mut p_out = ptr::null_mut();

        if status.is_input_full == 0 {
            if let Some(input) = self.input_queue.front() {
                p_in = input;
            }
        }

        let mut output = 0;
        if status.is_output_empty == 0 {
            p_out = &mut output;
        }

        unsafe {
            ClockProtoBridge(self.handle, p_in, p_out);
        }

        if !p_in.is_null() {
            self.input_queue.pop_front();
        }

        if !p_out.is_null() {
            self.output_queue.push_back(output);
        }

        self.clocks += 1;
    }

    fn wait_for_output(&mut self, num_bytes: usize, max_wait_cycles: usize) -> Result<usize> {
        // If we don't have enough data, we'll attempt to clock the device until we have enough.
        if self.output_queue.len() < num_bytes {
            for _wait_cycle_idx in 0..max_wait_cycles {
                self.clock();
                if self.output_queue.len() >= num_bytes {
                    break;
                }
            }
        }

        if self.output_queue.len() >= num_bytes {
            Ok(self.output_queue.len())
        } else {
            Err(io::Error::from(io::ErrorKind::TimedOut).into())
        }
    }

    // Command helper functions
    fn write_cmd(&mut self, cmd: u32) {
        self.write_all(&cmd.to_le_bytes()).expect("Failed to write command into internal buffer!");
    }

    fn cmd_read_bytes(&mut self, addr: u16, size: u16) {
        self.write_cmd(Self::build_cmd(CMD_ID_READ, addr, size));
    }

    fn cmd_read_reg(&mut self, idx: u16) {
        self.write_cmd(Self::build_reg_cmd(CMD_ID_READ, idx, 0));
    }

    fn cmd_write_bytes(&mut self, addr: u16, size: u16) {
        self.write_cmd(Self::build_cmd(CMD_ID_WRITE, addr, size));
    }

    fn cmd_write_reg(&mut self, idx: u16, data: u8) {
        self.write_cmd(Self::build_reg_cmd(CMD_ID_WRITE, idx, data));
    }

    // High level functions
    fn write_bytes(&mut self, addr: u16, buf: &[u8]) {
        self.cmd_write_bytes(addr, buf.len() as u16);
        self.write_all(buf).expect("Failed to write bytes into internal buffer!");
    }

    fn read_bytes(&mut self, addr: u16, buf: &mut [u8], max_wait_cycles: usize) -> Result<()> {
        self.cmd_read_bytes(addr, buf.len() as u16);
        match self.wait_for_output(buf.len(), max_wait_cycles) {
            Ok(_) => {
                self.read_exact(buf).expect("Failed to read bytes from internal buffer after waiting!");
                Ok(())
            }
            Err(err) => Err(err)
        }
    }

    fn read_reg(&mut self, idx: u16, max_wait_cycles: usize) -> Result<u8> {
        self.cmd_read_reg(idx);
        match self.wait_for_output(1, max_wait_cycles) {
            Ok(_) => {
                let mut buf = [0];
                self.read_exact(&mut buf).expect("Failed to read bytes from internal buffer after waiting!");
                Ok(buf[0])
            }
            Err(err) => Err(err)
        }
    }
}

impl Drop for ProtoBridge {
    fn drop(&mut self) {
        unsafe { DestroyProtoBridge(self.handle) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_test() {
        let memory_size = 16384;

        let mut bridge = ProtoBridge::new();

        let mut input_data = Vec::new();
        for i in 0..memory_size {
            input_data.push(i as u8);
        }

        bridge.write_bytes(0, &input_data);

        let mut output_data = vec![0; memory_size];

        assert!(bridge.read_bytes(0, &mut output_data, WAIT_INFINITE_CYCLES).is_ok());

        for i in 0..memory_size {
            assert_eq!(input_data[i], output_data[i]);
        }
    }
}

#[derive(Debug, Options)]
struct SimOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(free, required, help = "path to an elf file to execute")]
    elf_path: String,
}

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;

fn main() -> Result<()> {
    let opts = SimOptions::parse_args_default_or_exit();

    let path = Path::new(&opts.elf_path);
    let buffer = fs::read(path)?;
    if let Object::Elf(elf) = Object::parse(&buffer)? {
        let mut bridge = ProtoBridge::new();

        for header in elf.program_headers {
            if header.p_type == goblin::elf::program_header::PT_LOAD {
                let program_data =
                    &buffer[header.p_offset as usize..(header.p_offset + header.p_filesz) as usize];
                let program_addr = header.p_paddr as u16;

                bridge.write_bytes(program_addr, program_data);

                println!(
                    "Uploaded {} byte loadable program segment to address {:#06x} in device memory",
                    program_data.len(), program_addr
                );
            }
        }

        // Enable the device
        bridge.cmd_write_reg(REG_IDX_DEV_EN, 1);

        const MAX_TRIES: u64 = 4096;

        let mut progress = pbr::ProgressBar::new(MAX_TRIES);
        let mut stopped = false;

        for _ in 0..MAX_TRIES {
            progress.set(bridge.clocks());

            // Check if the device is still executing
            match bridge.read_reg(REG_IDX_DEV_EN, WAIT_INFINITE_CYCLES) {
                Ok(reg) => {
                    if reg != 0 {
                        // Still executing...
                    } else {
                        // The device has halted, break out of the loop
                        progress.total = bridge.clocks();
                        stopped = true;
                        break;
                    }
                }
                Err(err) => {
                    println!("Device error: {}", err);
                    break;
                }
            }
        }

        progress.total = bridge.clocks();
        progress.finish_println(&format!("Clocks: {}\n", bridge.clocks()));

        if stopped {
            println!("Execution stopped due to device halt");
        }
        else {
            println!("Execution stopped due to timeout");
        }

        let mut image_data = vec![0; 256];
        bridge.read_bytes(0x1F00, &mut image_data, WAIT_INFINITE_CYCLES).expect("Failed to read image data back from device!");

        let image: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> = image::ImageBuffer::from_fn(16, 16, |x, y| {
            let idx = y * 16 + x;
            let color = image_data[idx as usize];

            image::Rgb([color, color, color])
        });

        image.save("image.png").expect("Failed to write image output!");

    } else {
        eprint!("Invalid elf input file!");
    }

    Ok(())
}
