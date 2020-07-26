use devsim::device::Device;
use gumdrop::Options;

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

    let mut device = Device::new();

    // Load an elf from the command line arguments
    device.load_elf(&opts.elf_path)?;

    // Enable the device
    device.enable();

    // Wait for execution to complete
    const MAX_TRIES: u64 = 4096;

    let mut progress = pbr::ProgressBar::new(MAX_TRIES);
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

    let framebuffer = device
        .dump_framebuffer()
        .expect("Failed to dump device framebuffer!");

    let image: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
        image::ImageBuffer::from_fn(framebuffer.width, framebuffer.height, |x, y| {
            let idx = y * framebuffer.width + x;
            let color = framebuffer.data[idx as usize];

            image::Rgb([color, color, color])
        });

    image
        .save("image.png")
        .expect("Failed to write image output!");

    Ok(())
}
