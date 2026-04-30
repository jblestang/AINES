use std::fs;
mod core;
use crate::core::cartridge::Cartridge;
use crate::core::bus::Bus;
use crate::core::cpu::Cpu;
use image::{ImageBuffer, Rgba};

struct NesEmulator {
    cpu: Cpu,
    bus: Bus,
}

impl NesEmulator {
    fn new(cartridge: Cartridge) -> Self {
        NesEmulator {
            cpu: Cpu::new(),
            bus: Bus::new(cartridge),
        }
    }
    
    fn step_frame(&mut self) {
        let mut _frame_complete = false;
        while !_frame_complete {
            self.cpu.step(&mut self.bus);
            if self.bus.ppu.nmi_interrupt {
                self.cpu.nmi(&mut self.bus);
                self.bus.ppu.nmi_interrupt = false;
            }
            _frame_complete = self.bus.ppu.step();
            if _frame_complete { break; }
            _frame_complete = self.bus.ppu.step();
            if _frame_complete { break; }
            _frame_complete = self.bus.ppu.step();
        }
    }
}

fn main() {
    let data = fs::read("src/assets/Super Mario Bros. (World).nes").unwrap();
    let cart = Cartridge::load_rom(&data).unwrap();
    let mut emu = NesEmulator::new(cart);
    emu.cpu.reset(&mut emu.bus);
    
    for _ in 0..120 {
        emu.step_frame();
    }
    
    let fb = &emu.bus.ppu.frame_buffer;
    let img = ImageBuffer::<Rgba<u8>, _>::from_raw(256, 240, fb.to_vec()).unwrap();
    img.save("frame.png").unwrap();
}
