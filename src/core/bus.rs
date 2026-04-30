use super::ppu::Ppu;
use super::cartridge::Cartridge;
use super::apu::Apu;
use super::joypad::Joypad;

pub struct Bus {
    pub ram: [u8; 2048],
    pub ppu: Ppu,
    pub cartridge: Cartridge,
    pub apu: Apu,
    pub joypad1: Joypad,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut ppu = Ppu::new(cartridge.chr_rom.clone());
        ppu.vertical_mirroring = cartridge.vertical_mirroring;
        Bus {
            ram: [0; 2048],
            ppu,
            cartridge,
            apu: Apu::new(),
            joypad1: Joypad::new(),
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read(addr & 0x2007),
            0x4015 => self.apu.read(addr),
            0x4016 => self.joypad1.read(),
            0x4017 => 0, // Joypad 2 not implemented
            0x8000..=0xFFFF => {
                let prg_len = self.cartridge.prg_rom.len() as u16;
                let mapped_addr = if prg_len == 16384 {
                    addr & 0x3FFF
                } else {
                    addr & 0x7FFF
                };
                self.cartridge.prg_rom[mapped_addr as usize]
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize] = data;
            }
            0x2000..=0x3FFF => {
                self.ppu.write(addr & 0x2007, data);
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => {
                self.apu.write(addr, data);
            }
            0x4014 => {
                let mut buffer = [0; 256];
                let hi = (data as u16) << 8;
                for i in 0..256u16 {
                    buffer[i as usize] = self.read(hi + i);
                }
                self.ppu.oam_data = buffer;
            }
            0x4016 => {
                self.joypad1.write(data);
            }
            0x8000..=0xFFFF => {}
            _ => {}
        }
    }
}
