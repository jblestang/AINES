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

pub const PRG_ROM_START: u16 = 0x8000;
pub const PRG_ROM_END: u16 = 0xFFFF;

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
        // Create Ppu with a reference/copy of CHR data if needed, 
        // but Ppu should ideally use the mapper too.
        // For now, we'll give it dummy data and update Ppu next.
        let ppu = Ppu::new(vec![0; 8192]); 
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
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            RAM_RANGE_START..=RAM_RANGE_END => self.ram[(addr & RAM_MIRROR_MASK) as usize],
            PPU_REG_RANGE_START..=PPU_REG_RANGE_END => self.ppu.read(addr & PPU_REG_MIRROR_MASK, &*self.cartridge.mapper),
            ADDR_APU_STATUS => self.apu.read(addr),
            ADDR_JOYPAD1 => self.joypad1.read(),
            ADDR_JOYPAD2 => 0, // Joypad 2 not implemented
            PRG_ROM_START..=PRG_ROM_END => {
                self.cartridge.mapper.prg_read(addr)
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
                self.ppu.write(addr & PPU_REG_MIRROR_MASK, data, &mut *self.cartridge.mapper);
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
            PRG_ROM_START..=PRG_ROM_END => {
                self.cartridge.mapper.prg_write(addr, data);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cartridge::{Cartridge, INES_HEADER_SIZE, PRG_BANK_SIZE, CHR_BANK_SIZE, INES_MAGIC, FLAG_VERTICAL_MIRROR};

    fn create_mock_cartridge(prg_val: u8) -> Cartridge {
        let mut data = vec![0; INES_HEADER_SIZE + PRG_BANK_SIZE + CHR_BANK_SIZE];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 1; 
        data[5] = 1;
        data[6] = FLAG_VERTICAL_MIRROR;
        for i in 0..PRG_BANK_SIZE { data[INES_HEADER_SIZE + i] = prg_val; }
        Cartridge::load_rom(&data).unwrap()
    }

    #[test]
    fn test_bus_prg_mirroring_16k() {
        let cartridge = create_mock_cartridge(0xAA);
        let mut bus = Bus::new(cartridge);
        
        assert_eq!(bus.read(0x8000), 0xAA);
        assert_eq!(bus.read(0xC000), 0xAA);
    }

    #[test]
    fn test_bus_ram_mirroring() {
        let cartridge = create_mock_cartridge(0);
        let mut bus = Bus::new(cartridge);
        
        bus.write(0x0005, 0x55);
        assert_eq!(bus.read(0x0805), 0x55);
        assert_eq!(bus.read(0x1005), 0x55);
        assert_eq!(bus.read(0x1805), 0x55);
    }

    #[test]
    fn test_bus_ppu_mirroring() {
        let cartridge = create_mock_cartridge(0);
        let mut bus = Bus::new(cartridge);
        
        bus.write(0x2000, 0b1000_0000); // PPUCTRL
        assert_eq!(bus.ppu.ctrl, 0b1000_0000);
        
        bus.write(0x2008, 0b0000_0000); // Mirror of $2000
        assert_eq!(bus.ppu.ctrl, 0b0000_0000);
    }

    #[test]
    fn test_bus_oam_dma() {
        let cartridge = create_mock_cartridge(0);
        let mut bus = Bus::new(cartridge);
        
        for i in 0..256 {
            bus.write(0x0200 + i as u16, i as u8);
        }
        
        bus.write(0x4014, 0x02);
        
        for i in 0..256 {
            assert_eq!(bus.ppu.oam_data[i], i as u8);
        }
        assert!(bus.dma_cycles > 500); 
    }

    #[test]
    fn test_bus_routing_edge_cases() {
        let cartridge = create_mock_cartridge(0);
        let mut bus = Bus::new(cartridge);
        
        bus.write(0x4000, 0x55); 
        assert_eq!(bus.read(0x4015), 0); 
        
        bus.write(0x4016, 1); 
        bus.write(0x4016, 0);
        assert_eq!(bus.read(0x4016), 0); 
        assert_eq!(bus.read(0x4017), 0); 
        
        bus.write(0x8000, 0xFF);
        
        bus.write(0x5000, 0xFF);
        assert_eq!(bus.read(0x5000), 0);
    }
}
