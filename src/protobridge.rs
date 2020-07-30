use std::cmp;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::io;
use std::io::{Read, Write};
use std::ptr;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

type ProtoBridgeHandle = *mut c_void;

pub const WAIT_INFINITE_CYCLES: usize = 0xffffffff;

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

pub struct ProtoBridge {
    handle: ProtoBridgeHandle,
    clocks: u64,
    input_queue: VecDeque<u8>,
    output_queue: VecDeque<u8>,
}

const CMD_ID_READ: u8 = 1;
const CMD_ID_WRITE: u8 = 2;

pub const REG_IDX_DEV_EN: u16 = 0;
pub const REG_IDX_FB_ADDR: u16 = 1;
pub const REG_IDX_FB_CONFIG: u16 = 2;

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
    pub fn new() -> Self {
        let mut handle = ptr::null_mut();
        unsafe {
            CreateProtoBridge(&mut handle);
        }
        ProtoBridge {
            handle,
            clocks: 0,
            input_queue: VecDeque::new(),
            output_queue: VecDeque::new(),
        }
    }

    pub fn clocks(&self) -> u64 {
        self.clocks
    }

    // Internal helper functions
    fn build_cmd(id: u8, addr: u32, size: u32) -> u64 {
        ((id as u64 & 0xf) << 60) | ((addr as u64 & 0x3fffffff) << 30) | (size as u64 & 0x3fffffff)
    }

    fn build_reg_cmd(id: u8, idx: u16, data: u32) -> u64 {
        ((id as u64 & 0xf) << 60)
            | ((((idx << 2) as u64 | 0x3ffff000) & 0x3fffffff) << 30)
            | (data as u64 & 0x3fffffff)
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

    pub fn wait_for_output(&mut self, num_bytes: usize, max_wait_cycles: usize) -> Result<usize> {
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
    fn write_cmd(&mut self, cmd: u64) {
        self.write_all(&cmd.to_le_bytes())
            .expect("Failed to write command into internal buffer!");
    }

    fn cmd_read_bytes(&mut self, addr: u32, size: u32) {
        self.write_cmd(Self::build_cmd(CMD_ID_READ, addr, size));
    }

    fn cmd_read_reg(&mut self, idx: u16) {
        self.write_cmd(Self::build_reg_cmd(CMD_ID_READ, idx, 0xffffffff));
    }

    fn cmd_write_bytes(&mut self, addr: u32, size: u32) {
        self.write_cmd(Self::build_cmd(CMD_ID_WRITE, addr, size));
    }

    fn cmd_write_reg(&mut self, idx: u16, data: u32) {
        self.write_cmd(Self::build_reg_cmd(CMD_ID_WRITE, idx, data));
    }

    // High level functions
    pub fn write_bytes(&mut self, addr: u32, buf: &[u8]) {
        self.cmd_write_bytes(addr, buf.len() as u32);
        self.write_all(buf)
            .expect("Failed to write bytes into internal buffer!");
    }

    pub fn read_bytes(&mut self, addr: u32, buf: &mut [u8], max_wait_cycles: usize) -> Result<()> {
        self.cmd_read_bytes(addr, buf.len() as u32);
        match self.wait_for_output(buf.len(), max_wait_cycles) {
            Ok(_) => {
                self.read_exact(buf)
                    .expect("Failed to read bytes from internal buffer after waiting!");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    pub fn read_reg(&mut self, idx: u16, max_wait_cycles: usize) -> Result<u32> {
        self.cmd_read_reg(idx);
        match self.wait_for_output(4, max_wait_cycles) {
            Ok(_) => {
                let mut buf = [0; 4];
                self.read_exact(&mut buf)
                    .expect("Failed to read bytes from internal buffer after waiting!");
                Ok(u32::from_le_bytes(buf))
            }
            Err(err) => Err(err),
        }
    }

    pub fn write_reg(&mut self, idx: u16, data: u32) {
        self.cmd_write_reg(idx, data);
    }
}

impl Default for ProtoBridge {
    fn default() -> Self {
        Self::new()
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

        assert!(bridge
            .read_bytes(0, &mut output_data, WAIT_INFINITE_CYCLES)
            .is_ok());

        for i in 0..memory_size {
            assert_eq!(input_data[i], output_data[i]);
        }
    }
}
