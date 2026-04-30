pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: u8,
    pub vertical_mirroring: bool,
}

/// Standard iNES header size (16 bytes)
pub const INES_HEADER_SIZE: usize = 16;
/// Size of a single PRG-ROM bank (16KB)
pub const PRG_BANK_SIZE: usize = 16384;
/// Size of a single CHR-ROM bank (8KB)
pub const CHR_BANK_SIZE: usize = 8192;
/// Size of the trainer block if present (512 bytes)
pub const TRAINER_SIZE: usize = 512;
/// Magic constant at start of iNES files ("NES" + 0x1A)
pub const INES_MAGIC: &[u8; 4] = b"NES\x1A";

// Header Flag Masks
pub const FLAG_VERTICAL_MIRROR: u8 = 0x01;
pub const FLAG_TRAINER_PRESENT: u8 = 0x04;

impl Cartridge {

    /// Parses a raw byte array as an iNES (.nes) file.
    /// 
    /// # iNES Parsing Algorithm
    /// 1. **Header Validation**: Checks for "NES" + EOF constant.
    /// 2. **Metadata Extraction**: Reads PRG/CHR bank counts and flags (Mirroring, Mapper ID).
    /// 3. **Trainer Skip**: If the Trainer flag is set, skips 512 bytes of compatibility data.
    /// 4. **Bank Loading**: Copies the specified number of 16KB PRG banks and 8KB CHR banks.
    /// 5. **CHR-RAM Support**: If CHR count is 0, initializes 8KB of writable CHR-RAM.
    pub fn load_rom(data: &[u8]) -> Result<Self, String> {
        if data.len() < INES_HEADER_SIZE {
            return Err("File too small to be a NES ROM".to_string());
        }

        // iNES header validation
        if &data[0..4] != INES_MAGIC {
            return Err("Invalid iNES header".to_string());
        }

        let prg_banks = data[4] as usize;
        let chr_banks = data[5] as usize;
        let mapper1 = data[6] >> 4;
        let mapper2 = data[7] >> 4;
        let mapper = (mapper2 << 4) | mapper1;

        if mapper != 0 {
            return Err(format!("Unsupported mapper: {}", mapper));
        }

        let prg_size = prg_banks * PRG_BANK_SIZE;
        let chr_size = chr_banks * CHR_BANK_SIZE;

        let mut offset = INES_HEADER_SIZE;
        
        // Skip trainer if present
        if data[6] & FLAG_TRAINER_PRESENT != 0 {
            offset += TRAINER_SIZE;
        }

        if data.len() < offset + prg_size + chr_size {
            return Err("ROM file is missing data based on header sizes".to_string());
        }

        let prg_rom = data[offset..(offset + prg_size)].to_vec();
        offset += prg_size;
        
        let chr_rom = if chr_size > 0 {
            data[offset..(offset + chr_size)].to_vec()
        } else {
            // CHR RAM - use default bank size
            vec![0; CHR_BANK_SIZE]
        };

        let vertical_mirroring = (data[6] & FLAG_VERTICAL_MIRROR) != 0;

        Ok(Cartridge {
            prg_rom,
            chr_rom,
            mapper,
            vertical_mirroring,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Objective**: Verify that the Cartridge correctly parses a standard iNES header, 
    /// extracts bank counts, and loads the corresponding memory regions.
    #[test]
    fn test_cartridge_load_nrom() {
        let mut data = vec![0; INES_HEADER_SIZE + PRG_BANK_SIZE + CHR_BANK_SIZE];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 1; // 1 PRG bank (16KB)
        data[5] = 1; // 1 CHR bank (8KB)
        data[6] = FLAG_VERTICAL_MIRROR; // Mapper 0, Vertical Mirroring
        
        // Fill some data
        data[INES_HEADER_SIZE] = 0xDE; // Start of PRG
        data[INES_HEADER_SIZE + PRG_BANK_SIZE] = 0xAD; // Start of CHR
        
        let cart = Cartridge::load_rom(&data).unwrap();
        
        assert_eq!(cart.prg_rom.len(), PRG_BANK_SIZE);
        assert_eq!(cart.chr_rom.len(), CHR_BANK_SIZE);
        assert_eq!(cart.prg_rom[0], 0xDE);
        assert_eq!(cart.chr_rom[0], 0xAD);
        assert!(cart.vertical_mirroring);
        assert_eq!(cart.mapper, 0);
    }

    /// **Objective**: Verify that the Cartridge correctly handles the "Trainer" block 
    /// by skipping the 512-byte compatibility region.
    #[test]
    fn test_cartridge_with_trainer() {
        let mut data = vec![0; INES_HEADER_SIZE + TRAINER_SIZE + PRG_BANK_SIZE];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 1; // 1 PRG bank
        data[6] = FLAG_TRAINER_PRESENT;
        
        // Data after trainer
        data[INES_HEADER_SIZE + TRAINER_SIZE] = 0xBE;
        
        let cart = Cartridge::load_rom(&data).unwrap();
        assert_eq!(cart.prg_rom[0], 0xBE);
    }

    /// **Objective**: Verify that the horizontal and vertical mirroring flags 
    /// are correctly parsed from the iNES header.
    #[test]
    fn test_cartridge_mirroring_flags() {
        let mut data = vec![0; INES_HEADER_SIZE + PRG_BANK_SIZE];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 1;
        
        // Vertical mirroring (Bit 0 set)
        data[6] = 0x01;
        let cart_v = Cartridge::load_rom(&data).unwrap();
        assert_eq!(cart_v.vertical_mirroring, true);
        
        // Horizontal mirroring (Bit 0 clear)
        data[6] = 0x00;
        let cart_h = Cartridge::load_rom(&data).unwrap();
        assert_eq!(cart_h.vertical_mirroring, false);
    }

    /// **Objective**: Verify that 32KB PRG-ROM (2 banks) is correctly loaded 
    /// and stored as a single contiguous buffer.
    #[test]
    fn test_cartridge_32k_prg() {
        let mut data = vec![0; INES_HEADER_SIZE + PRG_BANK_SIZE * 2];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 2; // 2 PRG banks = 32KB
        
        data[INES_HEADER_SIZE] = 0x11;
        data[INES_HEADER_SIZE + PRG_BANK_SIZE] = 0x22;
        
        let cart = Cartridge::load_rom(&data).unwrap();
        assert_eq!(cart.prg_rom.len(), 32768);
        assert_eq!(cart.prg_rom[0], 0x11);
        assert_eq!(cart.prg_rom[PRG_BANK_SIZE], 0x22);
    }

    /// **Objective**: Verify that the Cartridge loader correctly rejects 
    /// files with an invalid iNES magic header.
    #[test]
    fn test_cartridge_invalid_magic() {
        let mut data = vec![0; 100];
        data[0..4].copy_from_slice(b"NOTN");
        let result = Cartridge::load_rom(&data);
        assert!(result.is_err());
    }

    /// **Objective**: Verify that the Cartridge loader correctly rejects 
    /// mappers that are not currently implemented (only Mapper 0 is supported).
    #[test]
    fn test_cartridge_unsupported_mapper() {
        let mut data = vec![0; INES_HEADER_SIZE + PRG_BANK_SIZE];
        data[0..4].copy_from_slice(INES_MAGIC);
        data[4] = 1;
        data[6] = 0x10; // Mapper 1 (upper nibble of flag 6)
        
        let result = Cartridge::load_rom(&data);
        assert!(result.is_err());
    }
}
