#![feature(naked_functions, asm_sym, asm_const)]
#![no_std]
#![no_main]
// mod hal;

// use crate::hal::{pac_encoding::UART0_BASE, Serial};
use core::arch::asm;
use core::panic::PanicInfo;
use core::{fmt::Write, str};
use d1_pac::{ccu, Peripherals};

const CCU_BASE: usize = 0x0200_1000;
const CCU_UART_BGR: usize = CCU_BASE + 0x090C;

const APB0_CLK: usize = CCU_BASE + 0x0520; // 0x0200_1520
const APB1_CLK: usize = CCU_BASE + 0x0524; // 0x0200_1524

const GPIO_BASE_ADDR: u32 = 0x0200_0000;
const GPIO_PB_CFG0: u32 = GPIO_BASE_ADDR + 0x0030;
const GPIO_PB_CFG1: u32 = GPIO_BASE_ADDR + 0x0034;
const GPIO_PB_PULL: u32 = GPIO_BASE_ADDR + 0x0054;
const GPIO_PC_CFG0: u32 = GPIO_BASE_ADDR + 0x0060;
const GPIO_PC_DAT: u32 = GPIO_BASE_ADDR + 0x0070;

const UART0_BASE: u32 = 0x0250_0000;
const UART0_THR: u32 = UART0_BASE;
const UART0_DLH: u32 = UART0_BASE + 0x0004;
const UART0_FCR: u32 = UART0_BASE + 0x0008;
const UART0_LCR: u32 = UART0_BASE + 0x000C;
const UART0_MCR: u32 = UART0_BASE + 0x0010;
const UART0_LSR: u32 = UART0_BASE + 0x0014;

const UART_BAUD: u32 = 115200;

const PER_HART_STACK_SIZE: usize = 4 * 4096; // 16KiB
const SBI_STACK_SIZE: usize = 1 * PER_HART_STACK_SIZE;
#[link_section = ".bss.uninit"]
static mut SBI_STACK: [u8; SBI_STACK_SIZE] = [0; SBI_STACK_SIZE];

#[naked]
#[link_section = ".head.text"]
#[export_name = "head_jump"]
pub unsafe extern "C" fn head_jump() {
    asm!(
        ".option push",
        ".option rvc",
        "c.j    0x60",
        ".option pop",
        // sym start,
        options(noreturn)
    )
}

// todo: option(noreturn) generates an extra `unimp` insn

#[repr(C)]
pub struct HeadData {
    magic: [u8; 8],
    checksum: u32,
    length: u32,
    pub_head_size: u32,
    fel_script_address: u32,
    fel_uenv_length: u32,
    dt_name_offset: u32,
    dram_size: u32,
    boot_media: u32,
    string_pool: [u32; 13],
}

const STAMP_CHECKSUM: u32 = 0x5F0A6C39;

// clobber used by KEEP(*(.head.data)) in link script
#[link_section = ".head.data"]
pub static HEAD_DATA: HeadData = HeadData {
    magic: *b"eGON.BT0",
    checksum: STAMP_CHECKSUM, // real checksum filled by blob generator
    length: 0,                // real size filled by blob generator
    pub_head_size: 0,
    fel_script_address: 0,
    fel_uenv_length: 0,
    dt_name_offset: 0,
    dram_size: 0,
    boot_media: 0,
    string_pool: [0; 13],
};

#[naked]
#[export_name = "start"]
#[link_section = ".text.entry"]
pub unsafe extern "C" fn start() -> ! {
    asm!(
        "csrw   mie, zero",
        "li     t2, 0x30013",
        "csrs   0x7c2, t2", // MCOR
        "la     sp, {stack}",
        "li     t0, {per_hart_stack_size}",
        "add    sp, sp, t0",
        "la     a0, {head_data}",
        "j      {main}",
        stack = sym SBI_STACK,
        per_hart_stack_size = const PER_HART_STACK_SIZE,
        head_data = sym HEAD_DATA,
        main = sym main,
        options(noreturn)
    )
    // asm!(
    //     // open uart clock gate and reset gate
    //     "li     t0, 0x0200190C",
    //     "li     t1, (1 << 0) | (1 << 16)",
    //     "sw     t1, 0(t0)",
    //     // set gpio B8,B9 to uart0, B9 drive level 3
    //     "li     t0, 0x02000000",
    //     "lw     t1, 0x34(t0)",
    //     "ori    t1, t1, 0b01100110",
    //     "sw     t1, 0x34(t0)",
    //     "lw     t1, 0x48(t0)",
    //     "ori    t1, t1, 0b00110000",
    //     "sw     t1, 0x48(t0)",
    //     // write one char to uart
    //     "li     t0, 0x02500000",
    //     "li     t1, 82", // R
    //     "1:",
    //     "sb     t1, 0(t0)",
    //     "j      1b", // todo: remove when there's uart output
    //     "j      {}",
    //     sym main,
    //     options(noreturn)
    // )
}

extern "C" fn main() {
    init_bss();
    light_up_led();
    // configure_gpio_pf_port();
    configure_ccu_clocks();
    configure_uart_peripheral();
    uart0_putchar(b'T');
    uart0_putchar(b'e');
    uart0_putchar(b's');
    uart0_putchar(b't');
    uart0_putchar(b'\r');
    uart0_putchar(b'\n');
    loop {
        uart0_putchar(b'R');
        uart0_putchar(b'u');
        uart0_putchar(b's');
        uart0_putchar(b't');
        uart0_putchar(b'\r');
        uart0_putchar(b'\n');
        for _ in 0..50000000 {
            // delay
            unsafe { asm!("nop") };
        }
    }

    // let p = d1_pac::Peripherals::take().unwrap();
    // let uart = p.UART0;
    // loop {
    //     uart.thr().write(|w| unsafe { w.thr().bits(b'R') });
    //     while !uart.usr.read().rfne().bit_is_set() {}
    // }

    /*
    unsafe { asm!("la a0, {}", sym HEAD_DATA) };
    init_bss();
    let p = Peripherals::take().unwrap();
    let s = Serial::new(UART0_BASE);
    // let s = Serial::new(p.UART0).unwrap();
    writeln!(s, "Nezha"); // TODO

    /*
    let ubgr = ccu::UART_BGR;
    let c = CCU::new(p.UART0).unwrap();
    // CCU init
    // reset
    // p.CCU.UART_BGR.modify();

    // ccu::uart_bgr::W::uart0_rst
    ccu::uart_bgr::.clear_bit(0);
    for i in 1..100 {}
    p.CCU.bgr.modify(CCU_UART_BGR::UART0_RST.val(1));
    */

    // gate
    /*
    p.CCU.bgr.modify(CCU_UART_BGR::UART0_GATING.val(0));
    for i in 1..100 {}
    p.CCU.bgr.modify(CCU_UART_BGR::UART0_GATING.val(1));
    */

    let uart = p.UART0;
    loop {
        uart.thr().write(|w| unsafe { w.thr().bits(b'R') });
        while !uart.usr.read().rfne().bit_is_set() {}
    }
    */
}

use core::ptr::{read_volatile, write_volatile};

fn light_up_led() {
    // GPIO port C pin 1 (LED on Lichee RV module)
    // Change into output mode
    let pc_cfg0 = unsafe { read_volatile(GPIO_PC_CFG0 as *const u32) };
    let mut val = pc_cfg0 | 0b0001 << 4;
    unsafe { write_volatile(GPIO_PC_CFG0 as *mut u32, val) };
    // Set pin to HIGH
    let pc_dat0 = unsafe { read_volatile(GPIO_PC_DAT as *const u32) };
    val = pc_dat0 | 0b1 << 1;
    unsafe { write_volatile(GPIO_PC_DAT as *mut u32, val) };

    // GPIO port B pin 5 (available on Nezha)
    let pb_cfg0 = unsafe { read_volatile(GPIO_PB_CFG0 as *const u32) };
    let new_value = (pb_cfg0 & 0xff0fffff) | 0b0001 << 20;
    unsafe { write_volatile(GPIO_PB_CFG0 as *mut u32, new_value) };
    // TODO: set high
}

fn configure_gpio_pf_port() {
    let pf_cfg0 = unsafe { read_volatile(0x0200_00f0 as *const u32) };
    // PF5 Select: R-JRAG-CK
    // PF3 Select: R-JRAG-DO
    // PF1 Select: R-JRAG-DI
    // PF0 Select: R-JRAG-MS
    let new_value = (pf_cfg0 & 0xff0f0f00) | 0x00404044;
    unsafe { write_volatile(0x0200_00f0 as *mut u32, new_value) };
}

const PARITY: u32 = 0;
const STOP: u32 = 0;
const DLEN: u32 = 3;
const UART_SET: u32 = ((PARITY & 0x03) << 3) | ((STOP & 0x01) << 2) | (DLEN & 0x03);

fn configure_uart_peripheral() {
    // PB1 Select: UART0-RX
    // PB0 Select: UART0-TX
    let pb_cfg1 = unsafe { read_volatile(GPIO_PB_CFG1 as *const u32) };
    let new_value = (pb_cfg1 & 0xffffff00) | 0x66;
    unsafe { write_volatile(GPIO_PB_CFG1 as *mut u32, new_value) };

    // pull-ups
    let mut val = unsafe { read_volatile(GPIO_PB_PULL as *mut u32) };
    val = val | 0x01 << 16 | 0x01 << 18;
    unsafe { write_volatile(GPIO_PB_PULL as *mut u32, val) };

    let ccu_uart_bgr = unsafe { read_volatile(CCU_UART_BGR as *const u32) };
    // UART4_GATING: Pass
    // UART0_GATING: Pass
    /* UART bus gating reset */

    // reset
    let mut new_value = ccu_uart_bgr | 0x0 << 16;
    unsafe { write_volatile(CCU_UART_BGR as *mut u32, new_value) };
    for _ in 1..100 {}
    new_value = ccu_uart_bgr | 0x1 << 16;
    unsafe { write_volatile(CCU_UART_BGR as *mut u32, new_value) };
    // gating
    new_value = ccu_uart_bgr | 0x0;
    unsafe { write_volatile(CCU_UART_BGR as *mut u32, new_value) };
    for _ in 1..100 {}
    new_value = ccu_uart_bgr | 0x1;
    unsafe { write_volatile(CCU_UART_BGR as *mut u32, new_value) };

    // from xboot
    unsafe { write_volatile(UART0_MCR as *mut u32, 0x3) };

    let mut lcr = unsafe { read_volatile(UART0_LCR as *mut u32) };
    lcr = lcr | 0x80;
    unsafe { write_volatile(UART0_LCR as *mut u32, lcr) };

    let uart_clk = (24000000 + 8 * UART_BAUD) / (16 * UART_BAUD);
    let dlh = uart_clk >> 8;
    unsafe { write_volatile(UART0_DLH as *mut u32, dlh) };
    let dll = uart_clk & 0xff;
    unsafe { write_volatile(UART0_DLH as *mut u32, dll) };

    let mut lcr = unsafe { read_volatile(UART0_LCR as *mut u32) };
    lcr = lcr & 0b01111111; // ~0x80
    unsafe { write_volatile(UART0_LCR as *mut u32, lcr) };

    lcr = UART_SET;
    unsafe { write_volatile(UART0_LCR as *mut u32, lcr) };

    let fcr = 0x7;
    unsafe { write_volatile(UART0_FCR as *mut u32, fcr) };

    /*
    // Uart0 DivisorLatch LO: 0xD
    // Uart0 DivisorLatch HI: 0x0
    // disable interrupts
    unsafe { write_volatile(UART0_DLH as *mut u32, 0) };
    // Uart0 FifoControl
    // RCVR Trigger: FIFO-2 less than full
    // TX Empty Trigger: FIFO 1/2 Full
    // DMA Mode: Mode 0
    // XMIT FIFO Reset: 1
    // RCVR FIFO Reset: 1
    // Fifo Enable: 1
    unsafe { write_volatile(UART0_FCR as *mut u32, 0xF7) };
    unsafe { write_volatile(UART0_THR as *mut u32, 0xD) };

    // Uart0 Line control
    // Divisor latch access, break control: unmodified
    // Parity: disabled
    // Stop bit: 1 bit
    // Data length: 8 bits
    let uart0_line_control = unsafe { read_volatile(UART0_LCR as *const u32) };
    let new_value = (uart0_line_control & 0xffffff60) | 3;
    unsafe { write_volatile(UART0_LCR as *mut u32, new_value) };
    // Uart0 modem control
    // Uart function: UART mode
    // Auto flow control: disabled
    // Loop back or normal mode: normal mode
    // RTS value: 0
    // DTR value: 0
    unsafe { write_volatile(UART0_MCR as *mut u32, 0) };

    // enable tx
    let tx = unsafe { read_volatile(UART0_THR as *mut u32) };
    unsafe { write_volatile(UART0_THR as *mut u32, tx | 0x0) };
    */
}

fn configure_ccu_clocks() {
    let pll_cpu_ctrl = unsafe { read_volatile(0x0200_1000 as *const u32) };
    // 11010111 11111100 00000000 11111100
    // 11001000 00000000 00101001 00000000
    // PLL CPU control
    // Enable: 1
    // LDO Enable: 1
    // Lock enable: 0
    // PLL output gate: enable
    // PLL N: 42
    // PLL Unlock level: 21-29 clock cycles
    // PLL Lock level: 24-26 clock cycles
    // PLL M: 1
    let new_value = (pll_cpu_ctrl & 0xD7FC00FC) | 0xC8002900;
    unsafe { write_volatile(0x0200_1000 as *mut u32, new_value) };
    // APB0 clock configuration; APB0_CLK = source frequency / (N * M)
    // Clock source: PLL_PERI(1x)
    // Divide factor N: 2 (1 << _0x1_)
    // Divide factor M: 3 (_0x2_ + 1)
    unsafe { write_volatile(APB0_CLK as *mut u32, 0x0300_0102) };
    unsafe { write_volatile(APB1_CLK as *mut u32, 0x0300_0102) };
    // RISC-V Clock
    // Clock source: PLL_CPU
    // Divide factor N: 2
    // Divide factor M: 1
    unsafe { write_volatile(0x0200_1d00 as *mut u32, 0x0500_0100) };
}

fn uart0_putchar(a: u8) {
    loop {
        let uart0_lsr = unsafe { read_volatile(UART0_LSR as *const u32) };
        if uart0_lsr & (1 << 6) != 0 {
            break;
        }
        /*
        let uart0_status = unsafe { read_volatile(0x0250_007C as *const u32) };
        if uart0_status & 0x2 != 0 {
            // TX FIFO is empty
            break;
        }
        */
    }
    // write to uart transmitting holding register
    unsafe { write_volatile(UART0_BASE as *mut u32, a as u32) };
}

#[cfg_attr(not(test), panic_handler)]
#[allow(unused)]
fn panic(info: &PanicInfo) -> ! {
    loop {}
}

#[inline]
fn init_bss() {
    extern "C" {
        static mut ebss: u32;
        static mut sbss: u32;
        static mut edata: u32;
        static mut sdata: u32;
        static sidata: u32;
    }
    unsafe {
        r0::zero_bss(&mut sbss, &mut ebss);
        r0::init_data(&mut sdata, &mut edata, &sidata);
    }
}
