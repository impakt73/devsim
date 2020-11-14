# devsim

Quick hardware development-cycle using [Verilator](https://github.com/verilator/verilator),
[Rust](https://www.rust-lang.org/learn/get-started), and [ImGui](https://github.com/ocornut/imgui).

The goal here is to bring the turn-around-time for developing and simulating hardware down as much
as possible. You should be able to save the verilog and see the results instantly. With the `view`
Rust bin here, we are able to simulate the hardware and let it render to a framebuffer in real time.
See [`Device`](https://github.com/impakt73/devsim/blob/master/src/device.rs) in the Rust code for
details.

## Getting started

### Installing Build Pre-Reqs

Building is straight forward but has a couple of steps. The common theme here is to install the
following components:

1. Rust
2. CMake
3. Verilator with CMake support
4. VulkanSDK
5. Optionally, prebuild [`shaderc`](https://github.com/google/shaderc) and direct `cargo` to use that instead. Instructions for this are located [here](https://github.com/google/shaderc-rs#setup).

Note on Apple Silicon: The process should be the same on Apple Silicon, but you will have to decide
between building for Intel and using Rosetta, or building for native AS.

#### Rust
Rust has a nice getting started page [here](https://www.rust-lang.org/learn/get-started).

#### CMake
You can use your favorite package manager to install `cmake`, or get it [here](https://cmake.org/download/).

* on Windows, try [chocolatey](https://chocolatey.org/install)
* on MacOS, try [Homebrew](https://brew.sh/)

#### Verilator


Verilator is available in most package managers. You will need version @TODO or later, for CMake
support.

- **MacOS**: The `verilator` bottle on `brew` works fine.
- **Windows**: @TODO hahahahaha suffer
- **Ubuntu**: Starting with Ubuntu 20.04, the `verilator` package works fine.
- Other platforms should work but we hav
en't tested yet.

#### VulkanSDK

Installing Vulkan is detailed on [the LunarG site](https://vulkan.lunarg.com/sdk/home).
You can test that Vulkan is setup correctly with:
```bash
$ vulkaninfo
```

### Building

`devsim` builds with Cargo.
```bash
$ cargo build
$ cargo run
```

You can then drag and
 drop a RISC-V elf binary onto the window to begin executing. You can also
simulate without the UI by specifying the `sim` binary.
```bash
$ cargo run --bin sim
```

@TODO: Detail how to make a RISC-V elf binary using Rust.
