pub struct Apu {
    // Basic APU state skeleton
    // The APU registers are mapped from $4000 to $4017
    pulse1: [u8; 4],
    pulse2: [u8; 4],
    triangle: [u8; 4],
    noise: [u8; 4],
    dmc: [u8; 4],
    status: u8,
    frame_counter: u8,
}

impl Apu {
    pub fn new() -> Self {
        Apu {
            pulse1: [0; 4],
            pulse2: [0; 4],
            triangle: [0; 4],
            noise: [0; 4],
            dmc: [0; 4],
            status: 0,
            frame_counter: 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1[(addr - 0x4000) as usize] = data,
            0x4004..=0x4007 => self.pulse2[(addr - 0x4004) as usize] = data,
            0x4008..=0x400B => self.triangle[(addr - 0x4008) as usize] = data,
            0x400C..=0x400F => self.noise[(addr - 0x400C) as usize] = data,
            0x4010..=0x4013 => self.dmc[(addr - 0x4010) as usize] = data,
            0x4015 => self.status = data,
            0x4017 => self.frame_counter = data,
            _ => {}
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => self.status, // Very simplified
            _ => 0,
        }
    }

    pub fn step(&mut self) {
        // Step the APU frame counter and audio channels
        // Generating actual samples requires maintaining an internal buffer
        // which would then be read by Bevy audio system.
    }
}
