pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: u8,
    pub vertical_mirroring: bool,
}

impl Cartridge {
    pub fn new() -> Self {
        Cartridge {
            prg_rom: vec![0; 32768],
            chr_rom: vec![0; 8192],
            mapper: 0,
            vertical_mirroring: true,
        }
    }

    pub fn load_rom(data: &[u8]) -> Result<Self, String> {
        if data.len() < 16 {
            return Err("File too small to be a NES ROM".to_string());
        }

        // iNES header
        if &data[0..4] != b"NES\x1A" {
            return Err("Invalid iNES header".to_string());
        }

        let prg_banks = data[4] as usize;
        let chr_banks = data[5] as usize;
        let mapper1 = data[6] >> 4;
        let mapper2 = data[7] >> 4;
        let mapper = (mapper2 << 4) | mapper1;

        let prg_size = prg_banks * 16384;
        let chr_size = chr_banks * 8192;

        let mut offset = 16;
        
        // Skip trainer if present
        if data[6] & 0b0000_0100 != 0 {
            offset += 512;
        }

        if data.len() < offset + prg_size + chr_size {
            return Err("ROM file is missing data based on header sizes".to_string());
        }

        let prg_rom = data[offset..(offset + prg_size)].to_vec();
        offset += prg_size;
        
        let chr_rom = if chr_size > 0 {
            data[offset..(offset + chr_size)].to_vec()
        } else {
            // CHR RAM
            vec![0; 8192]
        };

        let vertical_mirroring = (data[6] & 0x01) != 0;

        Ok(Cartridge {
            prg_rom,
            chr_rom,
            mapper,
            vertical_mirroring,
        })
    }
}
