use devsim::{ProtoBridge, REG_IDX_DEV_EN, WAIT_INFINITE_CYCLES};
use goblin::Object;
use gumdrop::Options;
use std::fs;
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Options)]
struct SimOptions {
    #[options(help = "print help message")]
    help: bool,

    #[options(free, required, help = "path to an elf file to execute")]
    elf_path: String,
}

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
                    program_data.len(),
                    program_addr
                );
            }
        }

        // Enable the device
        bridge.write_reg(REG_IDX_DEV_EN, 1);

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
        } else {
            println!("Execution stopped due to timeout");
        }

        let mut image_data = vec![0; 256];
        bridge
            .read_bytes(0x1F00, &mut image_data, WAIT_INFINITE_CYCLES)
            .expect("Failed to read image data back from device!");

        let image: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(16, 16, |x, y| {
                let idx = y * 16 + x;
                let color = image_data[idx as usize];

                image::Rgb([color, color, color])
            });

        image
            .save("image.png")
            .expect("Failed to write image output!");
    } else {
        eprint!("Invalid elf input file!");
    }

    Ok(())
}
