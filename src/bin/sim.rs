use clap::Clap;
use devsim::device::Device;
use image::RgbaImage;
use std::time::Duration;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clap)]
#[clap(version)]
struct SimOptions {
    /// Path to a RISC-V elf to execute
    elf_path: String,

    /// Path to write out the framebuffer as a png
    #[clap(short = 'o')]
    image_path: Option<String>,
}

fn main() -> Result<()> {
    let opts = SimOptions::parse();

    let mut device = Device::new();

    // Load an elf from the command line arguments
    device.load_elf(&opts.elf_path)?;

    // Enable the device
    device.enable();

    // Wait for execution to complete
    const MAX_TRIES: u64 = 0xffffffff;

    let mut progress = pbr::ProgressBar::new(MAX_TRIES);
    progress.set_max_refresh_rate(Some(Duration::from_millis(100)));

    let mut stopped = false;

    for _ in 0..MAX_TRIES {
        progress.set(device.clocks());

        // Check if the device is still executing
        match device.query_is_halted() {
            Ok(is_halted) => {
                if !is_halted {
                    // Still executing...
                } else {
                    // The device has halted, break out of the loop
                    progress.total = device.clocks();
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

    progress.total = device.clocks();
    progress.finish_println(&format!("Clocks: {}\n", device.clocks()));

    if stopped {
        println!("Execution stopped due to device halt");
    } else {
        println!("Execution stopped due to timeout");
    }

    let (width, height) = device.query_framebuffer_size()?;
    let fb_size = (width * height * 4) as usize;
    let mut fb_data = vec![0; fb_size];

    device
        .dump_framebuffer(&mut fb_data)
        .expect("Failed to dump device framebuffer!");

    let image = RgbaImage::from_raw(width, height, fb_data)
        .expect("Failed to create image from framebuffer");

    let image_path: Option<&str> = opts.image_path.as_deref();
    image
        .save(image_path.unwrap_or("image.png"))
        .expect("Failed to write image output!");

    Ok(())
}
