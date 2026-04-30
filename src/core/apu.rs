pub struct Envelope {
    pub start_flag: bool,
    pub divider_count: u8,
    pub decay_count: u8,
    pub loop_flag: bool,
    pub constant_volume_flag: bool,
    pub volume_parameter: u8,
}

impl Envelope {
    pub fn new() -> Self {
        Envelope {
            start_flag: false,
            divider_count: 0,
            decay_count: 0,
            loop_flag: false,
            constant_volume_flag: false,
            volume_parameter: 0,
        }
    }

    pub fn step(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.decay_count = 15;
            self.divider_count = self.volume_parameter;
        } else {
            if self.divider_count > 0 {
                self.divider_count -= 1;
            } else {
                self.divider_count = self.volume_parameter;
                if self.decay_count > 0 {
                    self.decay_count -= 1;
                } else if self.loop_flag {
                    self.decay_count = 15;
                }
            }
        }
    }

    pub fn volume(&self) -> u8 {
        if self.constant_volume_flag {
            self.volume_parameter
        } else {
            self.decay_count
        }
    }
}

pub struct Sweep {
    pub enabled: bool,
    pub period: u8,
    pub negate: bool,
    pub shift: u8,
    pub reload: bool,
    pub divider: u8,
}

impl Sweep {
    pub fn new() -> Self {
        Sweep {
            enabled: false,
            period: 0,
            negate: false,
            shift: 0,
            reload: false,
            divider: 0,
        }
    }
}

pub struct PulseChannel {
    pub enabled: bool,
    pub duty: u8,
    pub length_counter_halt: bool,
    pub timer_reload: u16,
    pub timer_value: u16,
    pub length_counter: u8,
    pub duty_pos: u8,
    pub envelope: Envelope,
    pub sweep: Sweep,
    pub pulse2: bool,
}

impl PulseChannel {
    pub fn new(pulse2: bool) -> Self {
        PulseChannel {
            enabled: false,
            duty: 0,
            length_counter_halt: false,
            timer_reload: 0,
            timer_value: 0,
            length_counter: 0,
            duty_pos: 0,
            envelope: Envelope::new(),
            sweep: Sweep::new(),
            pulse2,
        }
    }

    pub fn step_sweep(&mut self) {
        if self.sweep.divider == 0 && self.sweep.enabled && self.sweep.shift > 0 && self.timer_reload >= 8 {
            let delta = self.timer_reload >> self.sweep.shift;
            if self.sweep.negate {
                self.timer_reload -= delta;
                if !self.pulse2 && self.timer_reload > 0 {
                    self.timer_reload -= 1;
                }
            } else {
                self.timer_reload += delta;
            }
        }
        
        if self.sweep.divider == 0 || self.sweep.reload {
            self.sweep.divider = self.sweep.period;
            self.sweep.reload = false;
        } else {
            self.sweep.divider -= 1;
        }
    }

    pub fn step_timer(&mut self) {
        if self.timer_value > 0 {
            self.timer_value -= 1;
        } else {
            self.timer_value = self.timer_reload;
            self.duty_pos = (self.duty_pos + 1) % 8;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.timer_reload < 8 {
            return 0;
        }
        let duty_table = [
            [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
            [0, 1, 1, 0, 0, 0, 0, 0], // 25%
            [0, 1, 1, 1, 1, 0, 0, 0], // 50%
            [1, 0, 0, 1, 1, 1, 1, 1], // 25% negated
        ];
        if duty_table[self.duty as usize][self.duty_pos as usize] == 1 {
            self.envelope.volume()
        } else {
            0
        }
    }
}

pub struct NoiseChannel {
    pub enabled: bool,
    pub length_counter_halt: bool,
    pub timer_reload: u16,
    pub timer_value: u16,
    pub length_counter: u8,
    pub shift_register: u16,
    pub mode: bool,
    pub envelope: Envelope,
}

impl NoiseChannel {
    pub fn new() -> Self {
        NoiseChannel {
            enabled: false,
            length_counter_halt: false,
            timer_reload: 0,
            timer_value: 0,
            length_counter: 0,
            shift_register: 1,
            mode: false,
            envelope: Envelope::new(),
        }
    }

    pub fn step_timer(&mut self) {
        if self.timer_value > 0 {
            self.timer_value -= 1;
        } else {
            self.timer_value = self.timer_reload;
            let bit_location = if self.mode { 6 } else { 1 };
            let b1 = self.shift_register & 1;
            let b2 = (self.shift_register >> bit_location) & 1;
            let feedback = b1 ^ b2;
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 1) == 1 {
            return 0;
        }
        self.envelope.volume()
    }
}

pub struct TriangleChannel {
    pub enabled: bool,
    pub length_counter_halt: bool,
    pub timer_reload: u16,
    pub timer_value: u16,
    pub length_counter: u8,
    pub step: u8,
    pub linear_counter: u8,
    pub linear_counter_reload: u8,
    pub control_flag: bool,
    pub reload_flag: bool,
}

impl TriangleChannel {
    pub fn new() -> Self {
        TriangleChannel {
            enabled: false,
            length_counter_halt: false,
            timer_reload: 0,
            timer_value: 0,
            length_counter: 0,
            step: 0,
            linear_counter: 0,
            linear_counter_reload: 0,
            control_flag: false,
            reload_flag: false,
        }
    }

    pub fn step_timer(&mut self) {
        if self.timer_value > 0 {
            self.timer_value -= 1;
        } else {
            self.timer_value = self.timer_reload;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.step = (self.step + 1) % 32;
            }
        }
    }

    pub fn output(&self) -> u8 {
        let table = [
            15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        ];
        table[self.step as usize]
    }
}

pub struct Apu {
    pub pulse1: PulseChannel,
    pub pulse2: PulseChannel,
    pub triangle: TriangleChannel,
    pub noise: NoiseChannel,
    pub cycles: usize,
    pub frame_counter: u8,
    pub frame_step: u8,
    pub last_sample: f32,
    pub last_filtered: f32,
}

impl Apu {
    pub fn new() -> Self {
        Apu {
            pulse1: PulseChannel::new(false),
            pulse2: PulseChannel::new(true),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            cycles: 0,
            frame_counter: 0,
            frame_step: 0,
            last_sample: 0.0,
            last_filtered: 0.0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            // Pulse 1
            0x4000 => {
                self.pulse1.duty = (data >> 6) & 0x03;
                self.pulse1.length_counter_halt = (data & 0x20) != 0;
                self.pulse1.envelope.loop_flag = (data & 0x20) != 0;
                self.pulse1.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.pulse1.envelope.volume_parameter = data & 0x0F;
            }
            0x4001 => {
                self.pulse1.sweep.enabled = (data & 0x80) != 0;
                self.pulse1.sweep.period = (data >> 4) & 0x07;
                self.pulse1.sweep.negate = (data & 0x08) != 0;
                self.pulse1.sweep.shift = data & 0x07;
                self.pulse1.sweep.reload = true;
            }
            0x4002 => {
                self.pulse1.timer_reload = (self.pulse1.timer_reload & 0x0700) | (data as u16);
            }
            0x4003 => {
                self.pulse1.timer_reload = (self.pulse1.timer_reload & 0x00FF) | ((data as u16 & 0x07) << 8);
                self.pulse1.length_counter = self.get_length_counter(data >> 3);
                self.pulse1.duty_pos = 0;
                self.pulse1.envelope.start_flag = true;
            }
            // Pulse 2
            0x4004 => {
                self.pulse2.duty = (data >> 6) & 0x03;
                self.pulse2.length_counter_halt = (data & 0x20) != 0;
                self.pulse2.envelope.loop_flag = (data & 0x20) != 0;
                self.pulse2.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.pulse2.envelope.volume_parameter = data & 0x0F;
            }
            0x4005 => {
                self.pulse2.sweep.enabled = (data & 0x80) != 0;
                self.pulse2.sweep.period = (data >> 4) & 0x07;
                self.pulse2.sweep.negate = (data & 0x08) != 0;
                self.pulse2.sweep.shift = data & 0x07;
                self.pulse2.sweep.reload = true;
            }
            0x4006 => {
                self.pulse2.timer_reload = (self.pulse2.timer_reload & 0x0700) | (data as u16);
            }
            0x4007 => {
                self.pulse2.timer_reload = (self.pulse2.timer_reload & 0x00FF) | ((data as u16 & 0x07) << 8);
                self.pulse2.length_counter = self.get_length_counter(data >> 3);
                self.pulse2.duty_pos = 0;
                self.pulse2.envelope.start_flag = true;
            }
            // Triangle
            0x4008 => {
                self.triangle.control_flag = (data & 0x80) != 0;
                self.triangle.length_counter_halt = self.triangle.control_flag;
                self.triangle.linear_counter_reload = data & 0x7F;
            }
            0x400A => {
                self.triangle.timer_reload = (self.triangle.timer_reload & 0x0700) | (data as u16);
            }
            0x400B => {
                self.triangle.timer_reload = (self.triangle.timer_reload & 0x00FF) | ((data as u16 & 0x07) << 8);
                self.triangle.length_counter = self.get_length_counter(data >> 3);
                self.triangle.reload_flag = true;
            }
            // Noise
            0x400C => {
                self.noise.length_counter_halt = (data & 0x20) != 0;
                self.noise.envelope.loop_flag = (data & 0x20) != 0;
                self.noise.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.noise.envelope.volume_parameter = data & 0x0F;
            }
            0x400E => {
                self.noise.mode = (data & 0x80) != 0;
                let timer_table = [
                    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
                ];
                self.noise.timer_reload = timer_table[(data & 0x0F) as usize];
            }
            0x400F => {
                self.noise.length_counter = self.get_length_counter(data >> 3);
                self.noise.envelope.start_flag = true;
            }
            0x4015 => {
                self.pulse1.enabled = (data & 0x01) != 0;
                self.pulse2.enabled = (data & 0x02) != 0;
                self.triangle.enabled = (data & 0x04) != 0;
                self.noise.enabled = (data & 0x08) != 0;
                if !self.pulse1.enabled { self.pulse1.length_counter = 0; }
                if !self.pulse2.enabled { self.pulse2.length_counter = 0; }
                if !self.triangle.enabled { self.triangle.length_counter = 0; }
                if !self.noise.enabled { self.noise.length_counter = 0; }
            }
            0x4017 => {
                self.frame_counter = data;
                self.cycles = 0;
            }
            _ => {}
        }
    }

    fn get_length_counter(&self, index: u8) -> u8 {
        let table = [
            10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
            12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
        ];
        table[index as usize]
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let mut res = 0;
                if self.pulse1.length_counter > 0 { res |= 0x01; }
                if self.pulse2.length_counter > 0 { res |= 0x02; }
                if self.triangle.length_counter > 0 { res |= 0x04; }
                if self.noise.length_counter > 0 { res |= 0x08; }
                res
            }
            _ => 0,
        }
    }

    pub fn step(&mut self) {
        // Timers step every CPU cycle
        self.pulse1.step_timer();
        self.pulse2.step_timer();
        self.triangle.step_timer();
        self.noise.step_timer();

        self.cycles += 1;
        
        // APU Frame Counter (Mode 0: 4-step)
        // Step 1: 7457
        // Step 2: 14913
        // Step 3: 22371
        // Step 4: 29829
        if self.cycles == 7457 || self.cycles == 14913 || self.cycles == 22371 || self.cycles == 29829 {
            self.frame_step = (self.frame_step + 1) % 4;
            
            // Linear counter clock (every step)
            if self.triangle.reload_flag {
                self.triangle.linear_counter = self.triangle.linear_counter_reload;
            } else if self.triangle.linear_counter > 0 {
                self.triangle.linear_counter -= 1;
            }
            if !self.triangle.control_flag {
                self.triangle.reload_flag = false;
            }

            // Envelopes clock (every step)
            self.pulse1.envelope.step();
            self.pulse2.envelope.step();
            self.noise.envelope.step();

            // Half-frame steps (Step 2 and 4): Clock length counters and sweep units
            if self.frame_step == 1 || self.frame_step == 3 {
                if !self.pulse1.length_counter_halt && self.pulse1.length_counter > 0 {
                    self.pulse1.length_counter -= 1;
                }
                if !self.pulse2.length_counter_halt && self.pulse2.length_counter > 0 {
                    self.pulse2.length_counter -= 1;
                }
                if !self.triangle.length_counter_halt && self.triangle.length_counter > 0 {
                    self.triangle.length_counter -= 1;
                }
                if !self.noise.length_counter_halt && self.noise.length_counter > 0 {
                    self.noise.length_counter -= 1;
                }
                
                self.pulse1.step_sweep();
                self.pulse2.step_sweep();
            }

            if self.cycles == 29829 {
                self.cycles = 0;
            }
        }
    }

    pub fn output(&mut self) -> f32 {
        let p1 = self.pulse1.output();
        let p2 = self.pulse2.output();
        let tri = self.triangle.output();
        let n = self.noise.output();
        
        // Reduced gain to prevent saturation and clipping
        let pulse_out = if p1 == 0 && p2 == 0 {
            0.0
        } else {
            (95.88 / (8128.0 / (p1 as f32 + p2 as f32) + 100.0)) * 0.3
        };
        
        let tnd_out = if tri == 0 && n == 0 {
            0.0
        } else {
            (159.79 / (1.0 / (tri as f32 / 8227.0 + n as f32 / 12241.0) + 100.0)) * 0.3
        };

        let raw = pulse_out + tnd_out;
        
        // Simple High-pass filter to remove DC offset
        let filtered = raw - self.last_sample + 0.999 * self.last_filtered;
        self.last_sample = raw;
        self.last_filtered = filtered;
        
        filtered
    }
}
