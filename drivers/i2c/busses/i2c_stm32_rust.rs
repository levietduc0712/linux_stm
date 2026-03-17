// SPDX-License-Identifier: GPL-2.0
//
// Experimental Rust platform driver for STM32 I2C controller
//
// This is a learning/experimentation module. It does NOT implement full I2C
// bus controller functionality. It demonstrates how a Rust kernel module can
// bind to an STM32 I2C controller node in the Device Tree, retrieve memory
// resources, map registers, and log basic hardware information.
//
// This mirrors what the C driver i2c-stm32f7.c does in its early probe(),
// but stops before any I2C adapter registration or transfer logic.
//
// Copyright (C) 2024 - Experimental / Learning
// Author: Rust-for-Linux Experimenter

//! Experimental Rust I2C platform driver for STM32MP25
//!
//! # Overview
//!
//! This module binds to the STM32 I2C controller via Device Tree using the
//! compatible string `"st,stm32mp25-i2c"`. It performs the early steps of
//! what the C driver `i2c-stm32f7.c` does:
//!
//! 1. Match against the Device Tree compatible string
//! 2. Retrieve the register memory resource (base address and size)
//! 3. Map the registers into kernel virtual address space (ioremap)
//! 4. Read and log the I2C_CR1 and I2C_TIMINGR registers
//! 5. Retrieve IRQ numbers (but does not install handlers)
//! 6. Unmap registers on removal
//!
//! # What This Is NOT
//!
//! - Not a full I2C controller driver
//! - Does not register an `i2c_adapter`
//! - Does not handle I2C transfers
//! - Does not manage clocks, resets, DMA, or power management
//!
//! # Why Unsafe?
//!
//! Linux 6.6 Rust support provides only basic kernel abstractions (printing,
//! error codes, synchronization, memory allocation). There are **no** safe
//! Rust wrappers for:
//! - Platform devices (`struct platform_device`)
//! - Device Tree / OF APIs
//! - ioremap / iounmap
//! - Resource management (struct resource)
//! - IRQ APIs
//!
//! Therefore, this module uses `unsafe` FFI calls through the auto-generated
//! `kernel::bindings` module to call C kernel functions directly.
//! Each `unsafe` block is annotated with a `// SAFETY:` comment explaining
//! why the call is sound.

use kernel::prelude::*;
use kernel::error::code::*;

module! {
    type: Stm32I2cRust,
    name: "i2c_stm32_rust",
    author: "Rust-for-Linux Experimenter",
    description: "Experimental Rust platform driver for STM32 I2C (learning only)",
    license: "GPL",
}

// ---------------------------------------------------------------------------
// STM32F7 I2C Register Offsets (mirrored from i2c-stm32f7.c)
// ---------------------------------------------------------------------------
// These are the same offsets used by the C driver. We only read a few
// registers to demonstrate ioremap + readl access.

/// Control Register 1
const STM32F7_I2C_CR1: usize = 0x00;
/// Control Register 2
const STM32F7_I2C_CR2: usize = 0x04;
/// Timing Register
const STM32F7_I2C_TIMINGR: usize = 0x10;
/// Interrupt and Status Register
const STM32F7_I2C_ISR: usize = 0x18;

/// Module state kept alive between init (probe) and drop (remove).
///
/// In a real driver this would hold clocks, reset controls, DMA channels,
/// the `struct device *`, etc. For this experiment we only keep the mapped
/// register base and its size so we can unmap on removal.
struct Stm32I2cRust {
    /// Virtual address returned by ioremap (NULL means not mapped).
    base: *mut core::ffi::c_void,
    /// Physical base address (for logging).
    phys_addr: u64,
    /// Size of the I/O region.
    size: usize,
    /// IRQ event number (or negative if unavailable).
    irq_event: i32,
}

// SAFETY: The `base` pointer is only accessed through readl/writel which are
// inherently thread-safe at the hardware level (single-word MMIO). A real
// driver would use a lock for multi-register sequences.
unsafe impl Send for Stm32I2cRust {}
unsafe impl Sync for Stm32I2cRust {}

/// Read a 32-bit MMIO register at `base + offset`.
///
/// # Safety
///
/// Caller must ensure `base` is a valid ioremap'd pointer and `offset` is
/// within the mapped region.
unsafe fn mmio_read32(base: *mut core::ffi::c_void, offset: usize) -> u32 {
    // SAFETY: `readl` is the kernel's MMIO read accessor (provided via
    // rust_helper_readl in helpers.c). The caller guarantees the pointer
    // arithmetic is within the mapped region.
    // We cast to *mut u8 for byte-level offset arithmetic, then back to
    // *const c_void for the readl call.
    unsafe {
        let addr = (base as *const u8).add(offset) as *const core::ffi::c_void;
        kernel::bindings::readl(addr)
    }
}

impl kernel::Module for Stm32I2cRust {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("=== STM32 I2C Rust Experimental Driver ===\n");
        pr_info!("This is a learning module — NOT a production I2C driver.\n");
        pr_info!("It demonstrates Rust platform-device concepts on Linux 6.6.\n");

        // -----------------------------------------------------------------
        // NOTE ON DEVICE TREE MATCHING
        // -----------------------------------------------------------------
        // In a proper Rust platform driver (once abstractions exist), the
        // kernel would call our probe() with a `struct platform_device *`
        // matching our of_device_id table.
        //
        // Since Linux 6.6 does NOT have Rust platform_driver abstractions,
        // this module is loaded as a plain kernel module. To actually bind
        // to a real Device Tree node, you must use the companion C shim
        // described in the documentation (see README).
        //
        // For demonstration, we directly look up the DT node by compatible
        // string and extract resources from it. This is NOT how production
        // drivers work, but it illustrates the concepts.
        // -----------------------------------------------------------------

        let mut phys_addr: u64 = 0;
        let mut size: u64 = 0;
        let mut irq_event: i32 = -1;
        let base: *mut core::ffi::c_void;

        // Step 1: Find the DT node with compatible = "st,stm32mp25-i2c"
        //
        // In the C driver, the platform bus does this automatically.
        // Here we use of_find_compatible_node() for demonstration.
        let compat = b"st,stm32mp25-i2c\0";

        // SAFETY: of_find_compatible_node is safe to call with (NULL, NULL, compat)
        // to search the entire device tree. The returned pointer is either NULL or
        // a valid device_node with incremented refcount.
        let node = unsafe {
            kernel::bindings::of_find_compatible_node(
                core::ptr::null_mut(), // from: search entire tree
                core::ptr::null(),     // type: any
                compat.as_ptr() as *const core::ffi::c_char,
            )
        };

        if node.is_null() {
            pr_info!("No DT node found with compatible \"st,stm32mp25-i2c\"\n");
            pr_info!("This is expected if running on non-STM32MP25 hardware.\n");
            pr_info!("Module loaded in demo mode (no hardware access).\n");
            return Ok(Stm32I2cRust {
                base: core::ptr::null_mut(),
                phys_addr: 0,
                size: 0,
                irq_event: -1,
            });
        }

        pr_info!("Found DT node for \"st,stm32mp25-i2c\"\n");

        // Step 2: Retrieve the register base address from the DT node
        //
        // In C: platform_get_resource() or of_address_to_resource()
        // The C driver uses devm_platform_get_and_ioremap_resource().

        // We use of_address_to_resource() to get the physical address info.
        let mut res: kernel::bindings::resource = unsafe { core::mem::zeroed() };

        // SAFETY: node is a valid device_node pointer (checked non-null above).
        // res is a valid stack-allocated resource struct.
        let ret = unsafe {
            kernel::bindings::of_address_to_resource(node, 0, &mut res)
        };

        if ret != 0 {
            pr_err!("Failed to get resource from DT node (err={})\n", ret);
            // SAFETY: node was obtained from of_find_compatible_node with
            // incremented refcount; of_node_put decrements it.
            unsafe { kernel::bindings::of_node_put(node) };
            return Err(ENODEV);
        }

        phys_addr = res.start;
        size = (res.end - res.start + 1) as u64;

        pr_info!("I2C register base: {:#010x}, size: {:#x}\n", phys_addr, size);

        // Step 3: Map the registers into kernel virtual address space
        //
        // In C: ioremap() or devm_ioremap_resource()
        // We use ioremap() directly since we have no devm context.

        // SAFETY: phys_addr and size come from a valid DT resource entry.
        // ioremap returns either a valid virtual address or NULL on failure.
        base = unsafe {
            kernel::bindings::ioremap(phys_addr, size as usize)
        };

        if base.is_null() {
            pr_err!("ioremap failed for {:#010x}\n", phys_addr);
            // SAFETY: valid node pointer from of_find_compatible_node.
            unsafe { kernel::bindings::of_node_put(node) };
            return Err(ENOMEM);
        }

        pr_info!("ioremap successful: virt={:p}\n", base);

        // Step 4: Read some I2C registers to demonstrate MMIO access
        //
        // These are the same registers defined in i2c-stm32f7.c.
        // On a real board, these values reflect the hardware reset state.

        // SAFETY: base is a valid ioremap'd pointer, and all offsets are
        // within the mapped region (size >= 0x400 for STM32 I2C).
        let cr1 = unsafe { mmio_read32(base, STM32F7_I2C_CR1) };
        let cr2 = unsafe { mmio_read32(base, STM32F7_I2C_CR2) };
        let timingr = unsafe { mmio_read32(base, STM32F7_I2C_TIMINGR) };
        let isr = unsafe { mmio_read32(base, STM32F7_I2C_ISR) };

        pr_info!("Register dump (reset state):\n");
        pr_info!("  CR1     (0x{:02x}) = {:#010x}\n", STM32F7_I2C_CR1, cr1);
        pr_info!("  CR2     (0x{:02x}) = {:#010x}\n", STM32F7_I2C_CR2, cr2);
        pr_info!("  TIMINGR (0x{:02x}) = {:#010x}\n", STM32F7_I2C_TIMINGR, timingr);
        pr_info!("  ISR     (0x{:02x}) = {:#010x}\n", STM32F7_I2C_ISR, isr);

        // Decode CR1 Peripheral Enable bit
        if cr1 & 0x1 != 0 {
            pr_info!("  -> I2C peripheral is ENABLED\n");
        } else {
            pr_info!("  -> I2C peripheral is DISABLED (expected at reset)\n");
        }

        // Step 5: Retrieve IRQ number
        //
        // In C: platform_get_irq(pdev, 0)
        // Here we use of_irq_get() on the DT node.
        // We do NOT install a handler — just log the IRQ number.

        // SAFETY: node is valid. of_irq_get returns the Linux IRQ number
        // or a negative error code.
        irq_event = unsafe {
            kernel::bindings::of_irq_get(node, 0)
        };

        if irq_event > 0 {
            pr_info!("Event IRQ: {} (not installing handler — demo only)\n", irq_event);
        } else {
            pr_info!("Could not get event IRQ (ret={}), continuing without it\n", irq_event);
            irq_event = -1;
        }

        // Step 6: Release the DT node reference
        //
        // SAFETY: node was obtained from of_find_compatible_node which
        // increments the refcount. We must call of_node_put to balance it.
        unsafe { kernel::bindings::of_node_put(node) };

        pr_info!("=== STM32 I2C Rust driver probe complete ===\n");
        pr_info!("Next steps for a real driver:\n");
        pr_info!("  - Enable clocks (clk_prepare_enable)\n");
        pr_info!("  - Deassert reset (reset_control_deassert)\n");
        pr_info!("  - Configure timing registers\n");
        pr_info!("  - Install IRQ handlers\n");
        pr_info!("  - Register i2c_adapter\n");
        pr_info!("  - Implement i2c_algorithm (.master_xfer)\n");

        Ok(Stm32I2cRust {
            base,
            phys_addr,
            size: size as usize,
            irq_event,
        })
    }
}

impl Drop for Stm32I2cRust {
    fn drop(&mut self) {
        pr_info!("=== STM32 I2C Rust driver remove ===\n");

        if !self.base.is_null() {
            pr_info!("Unmapping registers at {:p} (phys {:#010x}, size {:#x})\n",
                     self.base, self.phys_addr, self.size);

            // SAFETY: self.base was obtained from a successful ioremap() call
            // in init(). iounmap is the correct cleanup.
            unsafe {
                kernel::bindings::iounmap(self.base);
            }
            self.base = core::ptr::null_mut();
        } else {
            pr_info!("No registers mapped (demo mode) — nothing to unmap.\n");
        }

        pr_info!("STM32 I2C Rust driver unloaded.\n");
    }
}
