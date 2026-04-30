use super::ppu::Ppu;
use super::cartridge::Cartridge;
use super::apu::Apu;
use super::joypad::Joypad;

/// Size of the CPU internal RAM (2KB)
pub const RAM_SIZE: usize = 2048;
/// Mask to handle CPU RAM mirroring (0x0000-0x07FF mirrored up to 0x1FFF)
pub const RAM_MIRROR_MASK: u16 = 0x07FF;
/// Mask to handle PPU register mirroring (0x2000-0x2007 mirrored up to 0x3FFF)
pub const PPU_REG_MIRROR_MASK: u16 = 0x2007;

/// Memory-mapped address for OAM DMA
pub const ADDR_OAM_DMA: u16 = 0x4014;
/// Memory-mapped address for APU status and channel control
pub const ADDR_APU_STATUS: u16 = 0x4015;
/// Memory-mapped address for Joypad 1
pub const ADDR_JOYPAD1: u16 = 0x4016;
/// Memory-mapped address for Joypad 2 and APU frame counter
pub const ADDR_JOYPAD2: u16 = 0x4017;

/// Size of a single PRG-ROM bank (16KB)
pub const PRG_ROM_BANK_SIZE: u16 = 16384;

/// Start of the PRG-ROM address space
pub const PRG_ROM_START: u16 = 0x8000;
/// End of the PRG-ROM address space
pub const PRG_ROM_END: u16 = 0xFFFF;

/// Address mask for 16KB PRG-ROM mirroring
pub const PRG_ROM_MASK_16K: u16 = 0x3FFF;
/// Address mask for 32KB PRG-ROM mapping
pub const PRG_ROM_MASK_32K: u16 = 0x7FFF;

/// Size of the OAM data buffer (256 bytes)
pub const OAM_DATA_SIZE: usize = 256;
/// Number of CPU cycles stalled during OAM DMA
pub const DMA_STALL_CYCLES: u32 = 513;

/// Start of CPU internal RAM range
pub const RAM_RANGE_START: u16 = 0x0000;
/// End of CPU internal RAM range (including mirrors)
pub const RAM_RANGE_END: u16 = 0x1FFF;

/// Start of PPU register range
pub const PPU_REG_RANGE_START: u16 = 0x2000;
/// End of PPU register range (including mirrors)
pub const PPU_REG_RANGE_END: u16 = 0x3FFF;

/// Start of APU register range
pub const APU_REG_RANGE_START: u16 = 0x4000;
/// End of APU register range (excluding status/frame counter)
pub const APU_REG_RANGE_END: u16 = 0x4013;

pub struct Bus {
    pub ram: [u8; RAM_SIZE],
    pub ppu: Ppu,
    pub cartridge: Cartridge,
    pub apu: Apu,
    pub joypad1: Joypad,
    pub dma_cycles: u32,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        let mut ppu = Ppu::new(cartridge.chr_rom.clone());
        ppu.vertical_mirroring = cartridge.vertical_mirroring;
        Bus {
            ram: [0; RAM_SIZE],
            ppu,
            cartridge,
            apu: Apu::new(),
            joypad1: Joypad::new(),
            dma_cycles: 0,
        }
    }

    /// Reads a byte from the unified 16-bit address space.
    /// 
    /// # Memory Mapping Algorithm
    /// - **0x0000 - 0x1FFF**: CPU RAM (2KB). Mirrored every 2KB.
    /// - **0x2000 - 0x3FFF**: PPU Registers. Mirrored every 8 bytes.
    /// - **0x4000 - 0x4017**: APU and I/O Registers.
    /// - **0x8000 - 0xFFFF**: PRG-ROM (Cartridge). 
    ///   - If 16KB: Mirrored to fill the 32KB space.
    ///   - If 32KB: Mapped linearly.
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            RAM_RANGE_START..=RAM_RANGE_END => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            PPU_REG_RANGE_START..=PPU_REG_RANGE_END => self.ppu.read(addr & PPU_REG_MIRROR_MASK),
            ADDR_APU_STATUS => self.apu.read(addr),
            ADDR_JOYPAD1 => self.joypad1.read(),
            ADDR_JOYPAD2 => 0, // Joypad 2 not implemented
            PRG_ROM_START..=PRG_ROM_END => {
                let prg_len = self.cartridge.prg_rom.len() as u16;
                let mapped_addr = if prg_len == PRG_ROM_BANK_SIZE {
                    addr & PRG_ROM_MASK_16K
                } else {
                    addr & PRG_ROM_MASK_32K
                };
                self.cartridge.prg_rom.get(mapped_addr as usize).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            RAM_RANGE_START..=RAM_RANGE_END => {
                self.ram[(addr & RAM_MIRROR_MASK) as usize] = data;
            }
            PPU_REG_RANGE_START..=PPU_REG_RANGE_END => {
                self.ppu.write(addr & PPU_REG_MIRROR_MASK, data);
            }
            APU_REG_RANGE_START..=APU_REG_RANGE_END | ADDR_APU_STATUS | ADDR_JOYPAD2 => {
                self.apu.write(addr, data);
            }
            ADDR_OAM_DMA => {
                let mut buffer = [0; OAM_DATA_SIZE];
                let hi = u16::from(data) << 8;
                for i in 0..OAM_DATA_SIZE as u16 {
                    buffer[i as usize] = self.read(hi + i);
                }
                self.ppu.oam_data = buffer;
                self.dma_cycles += DMA_STALL_CYCLES;
            }
            ADDR_JOYPAD1 => {
                self.joypad1.write(data);
            }
            PRG_ROM_START..=PRG_ROM_END => {}
            _ => {}
        }
    }
}
