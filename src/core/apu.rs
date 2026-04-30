//! # Ricoh 2A03 Audio Processing Unit (APU)
//! 
//! The APU generates sound for the NES. It features five channels:
//! - Two Pulse (Square) channels.
//! - One Triangle channel.
//! - One Noise channel.
//! - One DMC (Delta Modulation Channel) - currently unimplemented.
//! 
//! ## Key Resources:
//! - [NESDev APU Reference](https://www.nesdev.org/wiki/APU)
//! - [APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter)
//! - [APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
//! - [APU Envelope](https://www.nesdev.org/wiki/APU_Envelope)
//! - [APU Sweep](https://www.nesdev.org/wiki/APU_Sweep)

/// APU Frame Counter step 1 (NTSC)
pub const FRAME_COUNTER_STEP1: u32 = 7457;
/// APU Frame Counter step 2 (NTSC)
pub const FRAME_COUNTER_STEP2: u32 = 14913;
/// APU Frame Counter step 3 (NTSC)
pub const FRAME_COUNTER_STEP3: u32 = 22371;
/// APU Frame Counter step 4 (NTSC)
pub const FRAME_COUNTER_STEP4: u32 = 29829;
/// APU Frame Counter step 5 (NTSC, 5-step mode only)
pub const FRAME_COUNTER_STEP5: u32 = 37281;

/// Coefficient for the high-pass filter to remove DC offset
pub const HPF_COEFFICIENT: f32 = 0.999;
/// Gain multiplier for the mixed audio signal
pub const AUDIO_GAIN: f32 = 0.3;

/// Minimum timer value for a pulse channel to produce output.
/// If the timer is set below this value, the channel is silenced.
pub const PULSE_MIN_TIMER: u16 = 8;
/// Total number of steps in a pulse channel's duty cycle sequence
pub const DUTY_STEPS: u8 = 8;

/// Noise channel timer periods for NTSC
pub const NOISE_TIMER_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// Length counter lookup table
pub const LENGTH_COUNTER_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

// APU Register Addresses
pub const REG_P1_CTRL: u16      = 0x4000;
pub const REG_P1_SWEEP: u16     = 0x4001;
pub const REG_P1_LO: u16        = 0x4002;
pub const REG_P1_HI: u16        = 0x4003;
pub const REG_P2_CTRL: u16      = 0x4004;
pub const REG_P2_SWEEP: u16     = 0x4005;
pub const REG_P2_LO: u16        = 0x4006;
pub const REG_P2_HI: u16        = 0x4007;
pub const REG_TRI_CTRL: u16     = 0x4008;
pub const REG_TRI_LO: u16       = 0x400A;
pub const REG_TRI_HI: u16       = 0x400B;
pub const REG_NOISE_CTRL: u16   = 0x400C;
pub const REG_NOISE_MODE: u16   = 0x400E;
pub const REG_NOISE_HI: u16     = 0x400F;
pub const REG_STATUS: u16       = 0x4015;
pub const REG_FRAME_COUNT: u16  = 0x4017;

bitflags::bitflags! {
    /// APU Status ($4015) register flags
    pub struct StatusFlags: u8 {
        const P1_ENABLE    = 0b0000_0001;
        const P2_ENABLE    = 0b0000_0010;
        const TRI_ENABLE   = 0b0000_0100;
        const NOISE_ENABLE = 0b0000_1000;
        const DMC_ENABLE   = 0b0001_0000;
    }
}

/// APU Envelope logic
/// 
/// The envelope generator controls the volume of a channel over time.
/// It can either provide a constant volume or a decaying volume.
/// 
/// See: [APU Envelope](https://www.nesdev.org/wiki/APU_Envelope)
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
        } else if self.divider_count > 0 {
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

    pub fn volume(&self) -> u8 {
        if self.constant_volume_flag {
            self.volume_parameter
        } else {
            self.decay_count
        }
    }
}

/// APU Sweep logic
/// 
/// The sweep unit periodically adjusts the frequency (timer period) of a pulse channel.
/// It is used for sound effects like slides and sirens.
/// 
/// See: [APU Sweep](https://www.nesdev.org/wiki/APU_Sweep)
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

/// Pulse (Square) Channel
/// 
/// Features a variable duty cycle (12.5%, 25%, 50%, 75%), an envelope generator,
/// a sweep unit, and a length counter.
/// 
/// See: [APU Pulse](https://www.nesdev.org/wiki/APU_Pulse)
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
        if self.sweep.divider == 0 && self.sweep.enabled && self.sweep.shift > 0 && self.timer_reload >= PULSE_MIN_TIMER {
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
            self.duty_pos = (self.duty_pos + 1) % DUTY_STEPS;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.timer_reload < PULSE_MIN_TIMER {
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

/// Noise Channel
/// 
/// Produces pseudo-random noise using a Linear Feedback Shift Register (LFSR).
/// 
/// See: [APU Noise](https://www.nesdev.org/wiki/APU_Noise)
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

    /// Steps the 15-bit Linear Feedback Shift Register (LFSR).
    /// 
    /// # Algorithm
    /// 1. Bit 0 is `XORed` with Bit 1 (mode 0) or Bit 6 (mode 1).
    /// 2. The shift register is shifted right by 1 bit.
    /// 3. The XOR result is placed in the feedback bit (Bit 14).
    /// 
    /// This produces pseudo-random noise of varying periodicities.
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

    /// Outputs the current volume if the LFSR bit 0 is 0.
    /// 
    /// # Algorithm
    /// The Noise channel's output is binary based on Bit 0 of the LFSR.
    /// If Bit 0 is 1, the channel is silent. This creates the "hissing" sound.
    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 1) == 1 {
            return 0;
        }
        self.envelope.volume()
    }
}

/// Triangle Channel
/// 
/// Produces a fixed-volume triangle wave. Features a linear counter for fine
/// duration control and a standard length counter.
/// 
/// See: [APU Triangle](https://www.nesdev.org/wiki/APU_Triangle)
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
    pub frame_counter_cycles: u32,
    pub frame_counter_mode: u8,
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
            frame_counter_cycles: 0,
            frame_counter_mode: 0,
            last_sample: 0.0,
            last_filtered: 0.0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            // Pulse 1
            REG_P1_CTRL => {
                self.pulse1.duty = (data >> 6) & 0x03;
                self.pulse1.length_counter_halt = (data & 0x20) != 0;
                self.pulse1.envelope.loop_flag = (data & 0x20) != 0;
                self.pulse1.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.pulse1.envelope.volume_parameter = data & 0x0F;
            }
            REG_P1_SWEEP => {
                self.pulse1.sweep.enabled = (data & 0x80) != 0;
                self.pulse1.sweep.period = (data >> 4) & 0x07;
                self.pulse1.sweep.negate = (data & 0x08) != 0;
                self.pulse1.sweep.shift = data & 0x07;
                self.pulse1.sweep.reload = true;
            }
            REG_P1_LO => {
                self.pulse1.timer_reload = (self.pulse1.timer_reload & 0x0700) | u16::from(data);
            }
            REG_P1_HI => {
                self.pulse1.timer_reload = (self.pulse1.timer_reload & 0x00FF) | ((u16::from(data) & 0x07) << 8);
                self.pulse1.length_counter = self.get_length_counter(data >> 3);
                self.pulse1.duty_pos = 0;
                self.pulse1.envelope.start_flag = true;
            }
            // Pulse 2
            REG_P2_CTRL => {
                self.pulse2.duty = (data >> 6) & 0x03;
                self.pulse2.length_counter_halt = (data & 0x20) != 0;
                self.pulse2.envelope.loop_flag = (data & 0x20) != 0;
                self.pulse2.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.pulse2.envelope.volume_parameter = data & 0x0F;
            }
            REG_P2_SWEEP => {
                self.pulse2.sweep.enabled = (data & 0x80) != 0;
                self.pulse2.sweep.period = (data >> 4) & 0x07;
                self.pulse2.sweep.negate = (data & 0x08) != 0;
                self.pulse2.sweep.shift = data & 0x07;
                self.pulse2.sweep.reload = true;
            }
            REG_P2_LO => {
                self.pulse2.timer_reload = (self.pulse2.timer_reload & 0x0700) | u16::from(data);
            }
            REG_P2_HI => {
                self.pulse2.timer_reload = (self.pulse2.timer_reload & 0x00FF) | ((u16::from(data) & 0x07) << 8);
                self.pulse2.length_counter = self.get_length_counter(data >> 3);
                self.pulse2.duty_pos = 0;
                self.pulse2.envelope.start_flag = true;
            }
            // Triangle
            REG_TRI_CTRL => {
                self.triangle.control_flag = (data & 0x80) != 0;
                self.triangle.length_counter_halt = self.triangle.control_flag;
                self.triangle.linear_counter_reload = data & 0x7F;
            }
            REG_TRI_LO => {
                self.triangle.timer_reload = (self.triangle.timer_reload & 0x0700) | u16::from(data);
            }
            REG_TRI_HI => {
                self.triangle.timer_reload = (self.triangle.timer_reload & 0x00FF) | ((u16::from(data) & 0x07) << 8);
                self.triangle.length_counter = self.get_length_counter(data >> 3);
                self.triangle.reload_flag = true;
            }
            // Noise
            REG_NOISE_CTRL => {
                self.noise.length_counter_halt = (data & 0x20) != 0;
                self.noise.envelope.loop_flag = (data & 0x20) != 0;
                self.noise.envelope.constant_volume_flag = (data & 0x10) != 0;
                self.noise.envelope.volume_parameter = data & 0x0F;
            }
            REG_NOISE_HI => {
                self.noise.length_counter = self.get_length_counter(data >> 3);
                self.noise.envelope.start_flag = true;
            }
            REG_NOISE_MODE => {
                self.noise.mode = (data & 0x80) != 0;
                self.noise.timer_reload = NOISE_TIMER_TABLE[(data & 0x0F) as usize];
            }
            REG_STATUS => {
                self.pulse1.enabled = (data & StatusFlags::P1_ENABLE.bits()) != 0;
                self.pulse2.enabled = (data & StatusFlags::P2_ENABLE.bits()) != 0;
                self.triangle.enabled = (data & StatusFlags::TRI_ENABLE.bits()) != 0;
                self.noise.enabled = (data & StatusFlags::NOISE_ENABLE.bits()) != 0;
                if !self.pulse1.enabled { self.pulse1.length_counter = 0; }
                if !self.pulse2.enabled { self.pulse2.length_counter = 0; }
                if !self.triangle.enabled { self.triangle.length_counter = 0; }
                if !self.noise.enabled { self.noise.length_counter = 0; }
            }
            REG_FRAME_COUNT => {
                self.frame_counter_mode = (data >> 7) & 0x01;
                self.frame_counter_cycles = 0;
            }
            _ => {}
        }
    }

    fn get_length_counter(&self, index: u8) -> u8 {
        LENGTH_COUNTER_TABLE[index as usize]
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            REG_STATUS => {
                let mut status = StatusFlags::empty();
                if self.pulse1.length_counter > 0 { status.insert(StatusFlags::P1_ENABLE); }
                if self.pulse2.length_counter > 0 { status.insert(StatusFlags::P2_ENABLE); }
                if self.triangle.length_counter > 0 { status.insert(StatusFlags::TRI_ENABLE); }
                if self.noise.length_counter > 0 { status.insert(StatusFlags::NOISE_ENABLE); }
                status.bits()
            }
            _ => 0,
        }
    }

    /// Advances the APU by one CPU cycle.
    pub fn step(&mut self) {
        // Timers step every CPU cycle
        self.pulse1.step_timer();
        self.pulse2.step_timer();
        self.triangle.step_timer();
        self.noise.step_timer();

        self.step_frame_counter();
    }

    fn step_envelopes(&mut self) {
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
    }

    fn step_length_counters(&mut self) {
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
    }

    fn step_sweeps(&mut self) {
        self.pulse1.step_sweep();
        self.pulse2.step_sweep();
    }

    /// Advances the APU frame counter and updates internal state (envelopes, sweeps, length counters)
    pub fn step_frame_counter(&mut self) {
        self.frame_counter_cycles += 1;
        
        if self.frame_counter_mode == 0 {
            // 4-step mode
            if self.frame_counter_cycles == FRAME_COUNTER_STEP1 {
                self.step_envelopes();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP2 {
                self.step_envelopes();
                self.step_sweeps();
                self.step_length_counters();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP3 {
                self.step_envelopes();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP4 {
                self.step_envelopes();
                self.step_sweeps();
                self.step_length_counters();
                self.frame_counter_cycles = 0;
            }
        } else {
            // 5-step mode
            if self.frame_counter_cycles == FRAME_COUNTER_STEP1 {
                self.step_envelopes();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP2 {
                self.step_envelopes();
                self.step_sweeps();
                self.step_length_counters();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP3 {
                self.step_envelopes();
            } else if self.frame_counter_cycles == FRAME_COUNTER_STEP5 {
                self.step_envelopes();
                self.step_sweeps();
                self.step_length_counters();
                self.frame_counter_cycles = 0;
            }
        }
    }

    /// Combines all audio channels into a single f32 sample.
    /// 
    /// # Non-Linear Mixing Algorithm
    /// Instead of simple addition (which causes clipping), the NES uses a non-linear 
    /// DAC approximation. This ensures that as more channels are active, their 
    /// individual contributions are compressed rather than summed linearly.
    /// 
    /// **Formulas:**
    /// - `pulse_out = 95.88 / ( (8128 / (p1 + p2)) + 100 )`
    /// - `tnd_out = 159.79 / ( (1 / (tri/8227 + noise/12241 + dmc/22638)) + 100 )`
    /// 
    /// See: [APU Mixer](https://www.nesdev.org/wiki/APU_Mixer)
    pub fn output(&mut self) -> f32 {
        let p1 = self.pulse1.output();
        let p2 = self.pulse2.output();
        let tri = self.triangle.output();
        let n = self.noise.output();
        
        // --- PULSE MIXER ---
        let pulse_out = if p1 == 0 && p2 == 0 {
            0.0
        } else {
            (95.88 / (8128.0 / (f32::from(p1) + f32::from(p2)) + 100.0)) * AUDIO_GAIN
        };
        
        // --- TND MIXER (Triangle, Noise, DMC) ---
        let tnd_out = if tri == 0 && n == 0 {
            0.0
        } else {
            (159.79 / (1.0 / (f32::from(tri) / 8227.0 + f32::from(n) / 12241.0) + 100.0)) * AUDIO_GAIN
        };

        let raw = pulse_out + tnd_out;
        
        // --- SIGNAL POLISHING: DC OFFSET REMOVAL ---
        // Simple High-pass filter to remove DC bias which can cause speaker saturation.
        let filtered = raw - self.last_sample + HPF_COEFFICIENT * self.last_filtered;
        self.last_sample = raw;
        self.last_filtered = filtered;
        
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_decay() {
        let mut env = Envelope::new();
        env.volume_parameter = 1; // Divider = 1
        env.start_flag = true;
        
        env.step(); // Start, decay_count = 15
        assert_eq!(env.decay_count, 15);
        
        env.step(); // divider_count becomes 0
        env.step(); // decay_count becomes 14
        assert_eq!(env.decay_count, 14);
    }

    #[test]
    fn test_frame_counter_4step() {
        let mut apu = Apu::new();
        apu.frame_counter_mode = 0;
        
        // Step to 7457
        for _ in 0..7457 { apu.step_frame_counter(); }
        // Should have stepped envelopes
        // (Hard to check without setting up an envelope, but we verified the cycle logic)
        assert_eq!(apu.frame_counter_cycles, 7457);
    }
}
