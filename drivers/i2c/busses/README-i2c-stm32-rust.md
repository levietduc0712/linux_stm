# Experimental Rust I2C Platform Driver for STM32MP25

## Overview

This is an **experimental Rust kernel module** that binds to an STM32 I2C
controller via Device Tree on the **STM32MP257-DK** board. It runs on
**Linux 6.6** with Rust support enabled.

**This is for learning and experimentation only.** It does NOT implement full
I2C bus controller functionality.

### What This Module Does

| Step | What | C Equivalent (`i2c-stm32f7.c`) |
|------|------|-------------------------------|
| 1 | Find DT node by compatible string | Platform bus match via `of_device_id` table |
| 2 | Read memory resource (base addr) | `devm_platform_get_and_ioremap_resource()` |
| 3 | ioremap register space | `devm_platform_get_and_ioremap_resource()` |
| 4 | Read CR1, CR2, TIMINGR, ISR regs | Various `readl_relaxed()` in probe/runtime |
| 5 | Get IRQ number (no handler) | `platform_get_irq()` + `devm_request_threaded_irq()` |
| 6 | Clean up on unload (iounmap) | `devm_*` automatic cleanup |

### What This Module Does NOT Do

- Register an `i2c_adapter`
- Implement `i2c_algorithm` / `master_xfer`
- Handle I2C transfers
- Manage clocks, resets, or DMA
- Handle interrupts
- Implement power management

---

## Files

```
drivers/i2c/busses/
├── i2c-stm32-rust.rs              # Rust kernel module source
├── i2c-stm32-rust-overlay-example.dtso  # DT overlay example (documentation)
├── Kconfig                         # Added CONFIG_I2C_STM32_RUST
├── Makefile                        # Added build rule for i2c_stm32_rust.o
└── i2c-stm32f7.c                  # Original C driver (unchanged)
```

---

## Relationship to i2c-stm32f7.c

The production C driver (`drivers/i2c/busses/i2c-stm32f7.c`) is a full-featured
I2C bus controller driver. Our Rust module does only the first few steps of what
the C driver's `stm32f7_i2c_probe()` function does:

### C probe() flow (simplified):

```
stm32f7_i2c_probe(struct platform_device *pdev)
│
├── 1. devm_kzalloc()                          ← We: stack struct
├── 2. of_device_get_match_data()               ← We: of_find_compatible_node()
├── 3. devm_platform_get_and_ioremap_resource() ← We: of_address_to_resource() + ioremap()
├── 4. platform_get_irq()                       ← We: of_irq_get() (no handler)
├── 5. devm_clk_get_enabled()                   ← NOT IMPLEMENTED
├── 6. devm_reset_control_get() + deassert      ← NOT IMPLEMENTED
├── 7. devm_request_threaded_irq()              ← NOT IMPLEMENTED
├── 8. stm32f7_i2c_setup_timing()              ← NOT IMPLEMENTED
├── 9. i2c_add_adapter()                        ← NOT IMPLEMENTED
├── 10. DMA init                                ← NOT IMPLEMENTED
└── 11. PM runtime setup                        ← NOT IMPLEMENTED
```

### Key Difference: Platform Driver Registration

The C driver uses `module_platform_driver()` which:
1. Registers a `struct platform_driver` with the kernel
2. The kernel matches DT nodes to the `of_device_id` table
3. Calls `probe()` with a valid `struct platform_device *`

**Linux 6.6 Rust support does NOT have platform driver abstractions.** So our
Rust module uses a workaround:
- Loads as a plain kernel module (`kernel::Module` trait)
- Manually searches the DT using `of_find_compatible_node()`
- This finds the first matching node and extracts resources from it

In newer kernel versions (6.8+), Rust platform driver abstractions are being
developed upstream, which would allow proper `probe()`/`remove()` callbacks.

---

## Device Tree

The STM32MP25x SoC already defines I2C nodes in the upstream device tree:

**File:** `arch/arm64/boot/dts/st/stm32mp251.dtsi`

```dts
i2c1: i2c@40120000 {
    compatible = "st,stm32mp25-i2c";
    reg = <0x40120000 0x400>;
    interrupt-names = "event";
    interrupts = <GIC_SPI 108 IRQ_TYPE_LEVEL_HIGH>;
    clocks = <&rcc CK_KER_I2C1>;
    resets = <&rcc I2C1_R>;
    #address-cells = <1>;
    #size-cells = <0>;
    dmas = <&hpdma 27 0x20 0x00003012>,
           <&hpdma 28 0x20 0x00003021>;
    dma-names = "rx", "tx";
    i2c-analog-filter;
    status = "disabled";
};
```

**Board file:** `arch/arm64/boot/dts/st/stm32mp257f-dk.dts` enables I2C2:

```dts
&i2c2 {
    pinctrl-names = "default", "sleep";
    pinctrl-0 = <&i2c2_pins_b>;
    pinctrl-1 = <&i2c2_sleep_pins_b>;
    i2c-scl-rising-time-ns = <108>;
    i2c-scl-falling-time-ns = <12>;
    clock-frequency = <400000>;
    status = "okay";
};
```

### What the Rust driver reads from Device Tree:
- `reg` property → Physical base address (`0x40120000` for I2C1, `0x40130000` for I2C2, etc.)
- `interrupts` property → IRQ number (via `of_irq_get()`)
- The module finds the FIRST node with `compatible = "st,stm32mp25-i2c"`

**Important conflict note:** If the original C driver (`i2c-stm32f7`) is also loaded,
both drivers will try to access the same hardware. For testing, either:
- Disable `CONFIG_I2C_STM32F7` when using the Rust module, OR
- Use a different I2C instance (e.g., enable I2C3 and disable the C driver for it)

---

## What Is Missing to Make This a Real I2C Controller Driver

To turn this into a functional I2C bus controller driver in Rust, you would need:

### 1. Rust Platform Driver Abstraction (kernel infrastructure)
```
Need:  platform_driver_register() / platform_driver_unregister()
       struct platform_driver with probe/remove callbacks
       of_device_id matching
Status: Not available in Linux 6.6, being developed for 6.8+
```

### 2. Clock Framework Bindings
```
Need:  clk_get() / clk_prepare_enable() / clk_disable_unprepare()
       devm_clk_get_enabled()
Status: Not available in Rust
```

### 3. Reset Controller Bindings
```
Need:  reset_control_get() / reset_control_assert() / deassert()
Status: Not available in Rust
```

### 4. IRQ Management
```
Need:  request_threaded_irq() / free_irq()
       Top-half + threaded handlers
       IRQ flags (IRQF_ONESHOT, etc.)
Status: Not available in Rust
```

### 5. I2C Subsystem Bindings
```
Need:  struct i2c_adapter (Rust wrapper)
       struct i2c_algorithm with master_xfer callback
       i2c_add_adapter() / i2c_del_adapter()
Status: Not available in Rust
```

### 6. DMA Engine Bindings (optional)
```
Need:  dma_request_chan() / dmaengine_submit() / dma_async_issue_pending()
Status: Not available in Rust
```

### 7. Power Management
```
Need:  dev_pm_ops (suspend/resume/runtime_pm)
       pm_runtime_*() helpers
Status: Not available in Rust
```

### 8. Safe MMIO Abstractions
```
Need:  IoMem<SIZE> or similar type-safe MMIO wrapper
       readl/writel with compile-time offset checking
Status: Proposed upstream (IoMem), not yet merged in 6.6
```

---

## Build Instructions

### Prerequisites

1. **Rust toolchain** — The kernel requires a specific Rust compiler version.
   Check with:
   ```bash
   cd /home/vietduc/linux_stm
   make rustavailable
   ```

   If Rust is not configured, install it:
   ```bash
   rustup override set $(scripts/min-tool-version.sh rustc)
   rustup component add rust-src
   cargo install --locked --version $(scripts/min-tool-version.sh bindgen) bindgen-cli
   ```

2. **ARM64 cross-compiler** — For STM32MP25x:
   ```bash
   # If using ST's SDK:
   source /opt/st/stm32mp2/*/environment-setup-cortexa35-ostl-linux-gnueabi

   # Or set manually:
   export ARCH=arm64
   export CROSS_COMPILE=aarch64-linux-gnu-
   ```

### Step 1: Configure the Kernel

```bash
cd /home/vietduc/linux_stm

# Start from the STM32MP25 defconfig
make ARCH=arm64 multi_v7_defconfig  # or your STM32MP25 defconfig

# Enable Rust support
make ARCH=arm64 menuconfig
```

In menuconfig, enable:
```
General setup --->
    [*] Rust support

Device Drivers --->
    I2C support --->
        I2C Hardware Bus support --->
            <M> STM32 I2C experimental Rust driver (learning only)
```

**Important:** `CONFIG_RUST=y` must be enabled first. The Rust driver option
(`CONFIG_I2C_STM32_RUST`) only appears when `CONFIG_RUST` is enabled.

Or add directly to `.config`:
```
CONFIG_RUST=y
CONFIG_I2C_STM32_RUST=m
```

### Step 2: Build

```bash
# Build the full kernel (first time — generates Rust bindings)
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j$(nproc)

# Or build just the Rust I2C module
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- M=drivers/i2c/busses modules
```

The output module will be:
```
drivers/i2c/busses/i2c_stm32_rust.ko
```

### Step 3: Deploy to Target

Copy the module to the STM32MP257-DK:
```bash
scp drivers/i2c/busses/i2c_stm32_rust.ko root@<board-ip>:/lib/modules/$(make kernelrelease)/extra/
```

### Step 4: Load and Test

On the STM32MP257-DK board:
```bash
# Optional: unload the C I2C driver if it's bound to the instance you want
#   (only needed if both drivers would conflict on the same I2C controller)
# rmmod i2c_stm32f7

# Load the Rust module
insmod /lib/modules/$(uname -r)/extra/i2c_stm32_rust.ko

# Check kernel log
dmesg | tail -30
```

### Expected Output

On the STM32MP257-DK with I2C2 enabled at `0x40130000`:

```
[  123.456789] i2c_stm32_rust: === STM32 I2C Rust Experimental Driver ===
[  123.456790] i2c_stm32_rust: This is a learning module — NOT a production I2C driver.
[  123.456791] i2c_stm32_rust: It demonstrates Rust platform-device concepts on Linux 6.6.
[  123.456792] i2c_stm32_rust: Found DT node for "st,stm32mp25-i2c"
[  123.456793] i2c_stm32_rust: I2C register base: 0x40120000, size: 0x400
[  123.456794] i2c_stm32_rust: ioremap successful: virt=0xffff800012340000
[  123.456795] i2c_stm32_rust: Register dump (reset state):
[  123.456796] i2c_stm32_rust:   CR1     (0x00) = 0x00000000
[  123.456797] i2c_stm32_rust:   CR2     (0x04) = 0x00000000
[  123.456798] i2c_stm32_rust:   TIMINGR (0x10) = 0x00000000
[  123.456799] i2c_stm32_rust:   ISR     (0x18) = 0x00000001
[  123.456800] i2c_stm32_rust:   -> I2C peripheral is DISABLED (expected at reset)
[  123.456801] i2c_stm32_rust: Event IRQ: 140 (not installing handler — demo only)
[  123.456802] i2c_stm32_rust: === STM32 I2C Rust driver probe complete ===
[  123.456803] i2c_stm32_rust: Next steps for a real driver:
[  123.456804] i2c_stm32_rust:   - Enable clocks (clk_prepare_enable)
[  123.456805] i2c_stm32_rust:   - Deassert reset (reset_control_deassert)
[  123.456806] i2c_stm32_rust:   - Configure timing registers
[  123.456807] i2c_stm32_rust:   - Install IRQ handlers
[  123.456808] i2c_stm32_rust:   - Register i2c_adapter
[  123.456809] i2c_stm32_rust:   - Implement i2c_algorithm (.master_xfer)
```

**Note:** Register values depend on whether the I2C peripheral has been
previously initialized by the C driver or bootloader. If the clock is not
enabled, register reads may return `0x00000000` or cause a bus fault.

### Step 5: Unload

```bash
rmmod i2c_stm32_rust
dmesg | tail -5
```

Expected:
```
[  456.789012] i2c_stm32_rust: === STM32 I2C Rust driver remove ===
[  456.789013] i2c_stm32_rust: Unmapping registers at 0xffff800012340000 (phys 0x40120000, size 0x400)
[  456.789014] i2c_stm32_rust: STM32 I2C Rust driver unloaded.
```

---

## Testing Without Hardware (QEMU)

If you don't have an STM32MP257-DK board, you can partially test on any
ARM64 QEMU target (the DT lookup will report "no node found" and the module
will load in demo mode):

```bash
# Build for generic arm64
make ARCH=arm64 defconfig
# Enable CONFIG_RUST=y and CONFIG_I2C_STM32_RUST=m
make ARCH=arm64 -j$(nproc)

# Boot with QEMU (no STM32 DT, so demo mode)
qemu-system-aarch64 -M virt -cpu cortex-a53 \
    -kernel arch/arm64/boot/Image \
    -initrd <your-initramfs> \
    -append "console=ttyAMA0" \
    -nographic
```

In QEMU, you'll see:
```
i2c_stm32_rust: No DT node found with compatible "st,stm32mp25-i2c"
i2c_stm32_rust: This is expected if running on non-STM32MP25 hardware.
i2c_stm32_rust: Module loaded in demo mode (no hardware access).
```

---

## Understanding the Unsafe Code

Linux 6.6 Rust support provides these safe abstractions:
- `kernel::prelude::*` — printing macros, error types, module macro
- `kernel::sync::*` — Arc, Mutex, SpinLock
- `kernel::error::*` — Error codes, Result type
- `kernel::types::*` — Opaque, ForeignOwnable

It does **not** provide safe wrappers for:
- Platform devices → We use `bindings::of_find_compatible_node()`
- Device Tree parsing → We use `bindings::of_address_to_resource()`
- MMIO mapping → We use `bindings::ioremap()` / `bindings::iounmap()`
- MMIO access → We use `bindings::readl()`
- IRQ retrieval → We use `bindings::of_irq_get()`
- Device node refcount → We use `bindings::of_node_put()`

Each `unsafe` block has a `// SAFETY:` comment explaining why the operation
is sound. The general principle is:
1. We get a pointer from a kernel API (e.g., `of_find_compatible_node`)
2. We check it for NULL before use
3. We pass it to related APIs that expect that pointer type
4. We clean up properly (e.g., `of_node_put`, `iounmap`)

### Where Safe Wrappers Would Help

A future Rust kernel (6.8+) might provide:

```rust
// Hypothetical safe API (NOT available in 6.6):
impl PlatformDriver for Stm32I2cRust {
    const OF_MATCH_TABLE: &[OfDeviceId] = &[
        OfDeviceId::new(c_str!("st,stm32mp25-i2c")),
    ];

    fn probe(pdev: &PlatformDevice) -> Result<Self> {
        let base = pdev.ioremap_resource(0)?;  // Safe MMIO wrapper
        let irq = pdev.get_irq(0)?;            // Safe IRQ number
        let cr1 = base.readl(0x00);            // Type-safe MMIO read
        // ...
    }
}
```

---

## References

- **C driver:** `drivers/i2c/busses/i2c-stm32f7.c`
- **DT bindings:** `Documentation/devicetree/bindings/i2c/st,stm32-i2c.yaml`
- **SoC DT:** `arch/arm64/boot/dts/st/stm32mp251.dtsi`
- **Board DT:** `arch/arm64/boot/dts/st/stm32mp257f-dk.dts`
- **Rust samples:** `samples/rust/rust_minimal.rs`, `samples/rust/rust_print.rs`
- **Rust bindings:** `rust/bindings/bindings_helper.h`
- **Rust kernel crate:** `rust/kernel/lib.rs`
- **Rust for Linux:** https://rust-for-linux.com
- **STM32MP25 Reference Manual:** ST RM0457
