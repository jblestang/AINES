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
    pub fn new() -> Self {
        Cartridge {
            prg_rom: vec![0; PRG_BANK_SIZE * 2], // Default 32KB
            chr_rom: vec![0; CHR_BANK_SIZE],     // Default 8KB
            mapper: 0,
            vertical_mirroring: true,
        }
    }

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
