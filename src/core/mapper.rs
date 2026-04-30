//! # AINES Mappers
//!
//! This module implements the memory mapping hardware found in NES cartridges.
//! Mappers allow the NES to address more than 32KB of PRG-ROM and 8KB of CHR-ROM
//! by switching different "banks" of memory into the CPU/PPU address space.
//!
//! ## References
//! - [NESdev Wiki: Mapper](https://www.nesdev.org/wiki/Mapper)
//! - [NESdev Wiki: NROM (Mapper 0)](https://www.nesdev.org/wiki/NROM)
//! - [NESdev Wiki: MMC1 (Mapper 1)](https://www.nesdev.org/wiki/MMC1)

pub const PRG_ROM_START: u16 = 0x8000;
pub const PRG_ROM_UPPER_START: u16 = 0xC000;
pub const PRG_BANK_SIZE_16K: usize = 16384;
pub const CHR_BANK_SIZE_8K: usize = 8192;
pub const CHR_BANK_SIZE_4K: usize = 4096;

/// Represents the different nametable mirroring modes supported by mappers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLower,
    SingleScreenUpper,
    #[allow(dead_code)]
    FourScreen,
}

/// The `Mapper` trait defines the interface for all NES memory mappers.
///
/// Hardware mappers intercept CPU and PPU memory requests to perform bank switching
/// and control hardware-specific features like scanline counters or mirroring.
pub trait Mapper: Send + Sync {
    /// Read from PRG-ROM at the given CPU address ($8000-$FFFF).
    fn prg_read(&self, addr: u16) -> u8;
    
    /// Write to the mapper's registers ($8000-$FFFF).
    /// Note: PRG-ROM is read-only; writes are used for mapper configuration.
    fn prg_write(&mut self, addr: u16, data: u8);
    
    /// Read from CHR-ROM/RAM at the given PPU address ($0000-$1FFF).
    fn chr_read(&self, addr: u16) -> u8;
    
    /// Write to CHR-ROM/RAM at the given PPU address ($0000-$1FFF).
    fn chr_write(&mut self, addr: u16, data: u8);
    
    /// Returns the current nametable mirroring mode enforced by the mapper.
    fn mirroring(&self) -> Mirroring;
    
    /// Debug function to force-write to PRG-ROM (used for unit tests and vectors).
    fn prg_write_debug(&mut self, addr: u16, data: u8);
}

// ── Mapper 0 (NROM) ──────────────────────────────────────────────────────────

/// Mapper 0 (NROM) is the simplest NES mapper, used by early games like Super Mario Bros.
/// It supports up to 32KB of PRG-ROM and 8KB of CHR-ROM with no bank switching.
///
/// Reference: <https://www.nesdev.org/wiki/NROM>
pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>, // Also used for CHR-RAM if empty in ROM
    mirroring: Mirroring,
}

impl Mapper0 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Mapper0 { prg_rom, chr_rom, mirroring }
    }
}

impl Mapper for Mapper0 {
    fn prg_read(&self, addr: u16) -> u8 {
        let addr = addr - PRG_ROM_START;
        if self.prg_rom.len() == PRG_BANK_SIZE_16K {
            self.prg_rom[(addr & 0x3FFF) as usize]
        } else {
            self.prg_rom[(addr & 0x7FFF) as usize]
        }
    }

    fn prg_write(&mut self, _addr: u16, _data: u8) {
        // NROM doesn't handle writes to PRG range
    }

    fn chr_read(&self, addr: u16) -> u8 {
        self.chr_rom[addr as usize]
    }

    fn chr_write(&mut self, addr: u16, data: u8) {
        // Only writable if it's CHR-RAM (handled by the Vec size/init in Cartridge)
        self.chr_rom[addr as usize] = data;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn prg_write_debug(&mut self, addr: u16, data: u8) {
        let addr = addr % PRG_ROM_START;
        if (addr as usize) < self.prg_rom.len() {
            self.prg_rom[addr as usize] = data;
        }
    }
}

// ── Mapper 1 (MMC1) ──────────────────────────────────────────────────────────

/// Mapper 1 (MMC1) was the most popular mapper, used in Zelda, Metroid, and Mega Man 2.
/// It features a serial interface for configuration and supports:
/// - Switching 16KB PRG-ROM banks or 32KB PRG-ROM banks.
/// - Switching 4KB or 8KB CHR-ROM banks.
/// - Selectable mirroring (Horizontal, Vertical, Single-Screen).
///
/// Reference: <https://www.nesdev.org/wiki/MMC1>
pub struct Mapper1 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    
    /// Serial shift register used to communicate with the mapper.
    /// Bit 4 is used as a sentinel to detect when 5 bits have been shifted in.
    shift_register: u8,
    
    /// Control register ($8000-$9FFF):
    /// Bits 0-1: Mirroring (0: one-screen, lower; 1: one-screen, upper; 2: vertical; 3: horizontal)
    /// Bits 2-3: PRG ROM bank mode (0, 1: switch 32 KB at $8000; 2: fix first bank at $8000, switch 16 KB at $C000; 3: switch 16 KB at $8000, fix last bank at $C000)
    /// Bit 4: CHR ROM bank mode (0: switch 8 KB at a time; 1: switch two separate 4 KB banks)
    control: u8,
    
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
    
    prg_banks_count: usize,
}

impl Mapper1 {
    const SR_INIT: u8 = 0x10;
    const CTRL_INIT: u8 = 0x0C;

    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _base_mirroring: Mirroring) -> Self {
        let prg_banks_count = prg_rom.len() / PRG_BANK_SIZE_16K;
        
        Mapper1 {
            prg_rom,
            chr_rom,
            shift_register: Self::SR_INIT, // Bit 4 set to track 5th write
            control: Self::CTRL_INIT,      // PRG ROM: fix first bank, switch 16KB second bank
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
            prg_banks_count,
        }
    }

    /// Updates the internal MMC1 registers after 5 bits have been shifted into the shift register.
    fn write_register(&mut self, addr: u16, data: u8) {
        match addr {
            0x8000..=0x9FFF => self.control = data,
            0xA000..=0xBFFF => self.chr_bank0 = data,
            0xC000..=0xDFFF => self.chr_bank1 = data,
            0xE000..=0xFFFF => self.prg_bank = data & 0x0F,
            _ => {}
        }
    }
}

impl Mapper for Mapper1 {
    fn prg_read(&self, addr: u16) -> u8 {
        // PRG ROM bank mode (control bits 2-3)
        let prg_mode = (self.control >> 2) & 0x03;
        match prg_mode {
            0 | 1 => {
                // Mode 0, 1: Switch 32KB at $8000, ignoring low bit of bank number
                let bank = (self.prg_bank & 0x0E) as usize;
                let offset = (addr as usize - PRG_ROM_START as usize) + (bank * PRG_BANK_SIZE_16K);
                self.prg_rom[offset]
            }
            2 => {
                // Mode 2: Fix first bank at $8000, switch 16KB bank at $C000
                if addr < PRG_ROM_UPPER_START {
                    self.prg_rom[addr as usize - PRG_ROM_START as usize]
                } else {
                    let bank = self.prg_bank as usize;
                    let offset = (addr as usize - PRG_ROM_UPPER_START as usize) + (bank * PRG_BANK_SIZE_16K);
                    self.prg_rom[offset]
                }
            }
            3 => {
                // Mode 3: Switch 16KB bank at $8000, fix last bank at $C000
                if addr < PRG_ROM_UPPER_START {
                    let bank = self.prg_bank as usize;
                    let offset = (addr as usize - PRG_ROM_START as usize) + (bank * PRG_BANK_SIZE_16K);
                    self.prg_rom[offset]
                } else {
                    let last_bank_offset = (self.prg_banks_count - 1) * PRG_BANK_SIZE_16K;
                    self.prg_rom[(addr as usize - PRG_ROM_UPPER_START as usize) + last_bank_offset]
                }
            }
            _ => unreachable!(),
        }
    }

    fn prg_write(&mut self, addr: u16, data: u8) {
        // MMC1 registers are accessed via a serial interface.
        // A write with bit 7 set resets the shift register.
        if data & 0x80 != 0 {
            self.shift_register = Self::SR_INIT;
            self.control |= Self::CTRL_INIT;
        } else {
            // Otherwise, shift in one bit.
            let complete = self.shift_register & 0x01 != 0;
            self.shift_register >>= 1;
            self.shift_register |= (data & 0x01) << 4;
            
            // After 5 writes, the shift register is full and we write to the internal register.
            if complete {
                self.write_register(addr, self.shift_register);
                self.shift_register = Self::SR_INIT;
            }
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
        let chr_mode = (self.control >> 4) & 0x01;
        if chr_mode == 0 {
            // Switch 8KB at a time
            let bank = (self.chr_bank0 & 0x1E) as usize;
            let offset = (addr as usize) + (bank * CHR_BANK_SIZE_4K);
            self.chr_rom[offset]
        } else {
            // Switch two separate 4KB banks
            if addr < 0x1000 {
                let bank = self.chr_bank0 as usize;
                let offset = (addr as usize) + (bank * CHR_BANK_SIZE_4K);
                self.chr_rom[offset]
            } else {
                let bank = self.chr_bank1 as usize;
                let offset = (addr as usize - 0x1000) + (bank * CHR_BANK_SIZE_4K);
                self.chr_rom[offset]
            }
        }
    }

    fn chr_write(&mut self, addr: u16, data: u8) {
        // Note: Some MMC1 cartridges have CHR-RAM instead of CHR-ROM.
        // We'll treat the chr_rom buffer as writable if it's behaving like RAM.
        let chr_mode = (self.control >> 4) & 0x01;
        let offset = if chr_mode == 0 {
            let bank = (self.chr_bank0 & 0x1E) as usize;
            (addr as usize) + (bank * CHR_BANK_SIZE_4K)
        } else if addr < 0x1000 {
            let bank = self.chr_bank0 as usize;
            (addr as usize) + (bank * CHR_BANK_SIZE_4K)
        } else {
            let bank = self.chr_bank1 as usize;
            (addr as usize - 0x1000) + (bank * CHR_BANK_SIZE_4K)
        };
        
        if offset < self.chr_rom.len() {
            self.chr_rom[offset] = data;
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }

    fn prg_write_debug(&mut self, addr: u16, data: u8) {
        // Simple mapping for debug - write to whatever is at that address
        // This is tricky for banking, but for vectors it works if they are in the last bank
        let addr = addr as usize;
        if addr >= PRG_ROM_START as usize {
            // Map to physical address based on current banking
            // For simplicity in tests, we'll just write to the end of PRG ROM
            let phys_addr = (self.prg_rom.len() - (0x10000 - addr)) % self.prg_rom.len();
            self.prg_rom[phys_addr] = data;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper0_prg_read() {
        let mut prg_rom = vec![0; PRG_BANK_SIZE_16K];
        prg_rom[0x0000] = 0xA9;
        prg_rom[PRG_BANK_SIZE_16K - 1] = 0x00;
        let mapper = Mapper0::new(prg_rom, vec![0; CHR_BANK_SIZE_8K], Mirroring::Vertical);
        
        assert_eq!(mapper.prg_read(PRG_ROM_START), 0xA9);
        assert_eq!(mapper.prg_read(PRG_ROM_START + PRG_BANK_SIZE_16K as u16 - 1), 0x00);
        assert_eq!(mapper.prg_read(PRG_ROM_UPPER_START), 0xA9); // Mirrored for 16KB
    }

    #[test]
    fn test_mapper1_shift_register() {
        let mut prg_rom = vec![0; 128 * 1024]; // 128KB
        for (i, val) in prg_rom.iter_mut().enumerate() { *val = (i / PRG_BANK_SIZE_16K) as u8; }
        let mut mapper = Mapper1::new(prg_rom, vec![0; CHR_BANK_SIZE_8K], Mirroring::Vertical);
        
        // Write 0x03 to PRG bank register ($E000-$FFFF)
        // 0x03 = 0b00011
        // Serial writes: LSB first
        mapper.prg_write(0xE000, 1); // bit 0
        mapper.prg_write(0xE000, 1); // bit 1
        mapper.prg_write(0xE000, 0); // bit 2
        mapper.prg_write(0xE000, 0); // bit 3
        mapper.prg_write(0xE000, 0); // bit 4 (complete)
        
        assert_eq!(mapper.prg_bank, 0x03);
    }

    #[test]
    fn test_mapper1_prg_banking() {
        let mut prg_rom = vec![0; 128 * 1024]; // 8 banks of 16KB
        for (i, val) in prg_rom.iter_mut().enumerate() { *val = (i / PRG_BANK_SIZE_16K) as u8; }
        let mut mapper = Mapper1::new(prg_rom, vec![0; CHR_BANK_SIZE_8K], Mirroring::Vertical);

        // Default mode (Mode 3): switch 16KB at $8000, fix last bank at $C000
        assert_eq!(mapper.prg_read(0x8000), 0);
        assert_eq!(mapper.prg_read(0xC000), 7);

        // Switch $8000 to bank 4
        mapper.prg_write(0xE000, 0); // bit 0
        mapper.prg_write(0xE000, 0); // bit 1
        mapper.prg_write(0xE000, 1); // bit 2 (value 4)
        mapper.prg_write(0xE000, 0); // bit 3
        mapper.prg_write(0xE000, 0); // bit 4 (complete)
        
        assert_eq!(mapper.prg_read(0x8000), 4);
        assert_eq!(mapper.prg_read(0xC000), 7);

        // Switch to Mode 2: fix first bank at $8000, switch 16KB at $C000
        // Control register: 0b010xx (Mode 2)
        mapper.prg_write(0x8000, 0); // bit 0
        mapper.prg_write(0x8000, 0); // bit 1
        mapper.prg_write(0x8000, 0); // bit 2 (Mode 2)
        mapper.prg_write(0x8000, 1); // bit 3 (Mode 2)
        mapper.prg_write(0x8000, 0); // bit 4 (complete)

        assert_eq!(mapper.prg_read(0x8000), 0);
        assert_eq!(mapper.prg_read(0xC000), 4); // Still bank 4 from before
    }

    #[test]
    fn test_mapper1_chr_banking() {
        let mut chr_rom = vec![0; 32 * 1024]; // 8 banks of 4KB
        for (i, val) in chr_rom.iter_mut().enumerate() { *val = (i / CHR_BANK_SIZE_4K) as u8; }
        let mut mapper = Mapper1::new(vec![0; 16384], chr_rom, Mirroring::Vertical);

        // Mode 0: switch 8KB at a time
        // Set CHR bank 0 to bank 2 (4KB unit, so 8KB bank 1)
        mapper.prg_write(0xA000, 0); // bit 0
        mapper.prg_write(0xA000, 1); // bit 1 (value 2)
        mapper.prg_write(0xA000, 0);
        mapper.prg_write(0xA000, 0);
        mapper.prg_write(0xA000, 0);
        
        assert_eq!(mapper.chr_read(0x0000), 2);
        assert_eq!(mapper.chr_read(0x1000), 3);

        // Mode 1: switch two 4KB banks
        // Set mode to 4KB (bit 4 of control)
        mapper.prg_write(0x8000, 0);
        mapper.prg_write(0x8000, 0);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1); // Bit 4 set

        // Set CHR bank 1 to bank 5
        mapper.prg_write(0xC000, 1); // bit 0
        mapper.prg_write(0xC000, 0); // bit 1
        mapper.prg_write(0xC000, 1); // bit 2 (value 5)
        mapper.prg_write(0xC000, 0);
        mapper.prg_write(0xC000, 0);

        assert_eq!(mapper.chr_read(0x0000), 2);
        assert_eq!(mapper.chr_read(0x1000), 5);
    }

    #[test]
    fn test_mapper1_mirroring() {
        let mut mapper = Mapper1::new(vec![0; 16384], vec![0; 8192], Mirroring::Vertical);

        // Horizontal mirroring (value 3 in control bits 0-1)
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 0);
        
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        // Single screen upper (value 1)
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 0);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 1);
        mapper.prg_write(0x8000, 0);
        
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);
    }
}
