# Rusty Vulkan First Triangle

A personal learning project. Primarily about learning Rust, secondarily about learning Vulkan.

It just draws a triangle in a window, as everyone's first code with a new rendering API should do.

For the sake of Rust learning, I intentionally limited myself to working only from Vulkan's C documentation, and worked out my own Rust equivalents. The unsafe [ash](https://github.com/ash-rs/ash) crate was used instead of something higher-level like [vulkano](https://github.com/vulkano-rs/vulkano). I suspect at the end of this I was starting to wrap Ash in my own piecemeal reimplemention of Vulkano.

One interesting complication is that the code supports multiple resizable windows (press N to open, ESC to close), which means multiple Vulkan objects with various lifetimes and depencies. I got the Rust code structured in a way where all Vulkan object lifetimes are directly and automatically tied to Rust lifetimes. All Vulkan deallocation and cleanup happens solely from Rust `drop()` implementations, with 100% clean diagnostics from the validation layers, so I think I'm starting to have a solid grip on lifetimes and the borrow checker. :-)

