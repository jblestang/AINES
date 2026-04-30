bitflags::bitflags! {
    // https://www.nesdev.org/wiki/Standard_controller
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct JoypadButton: u8 {
        const RIGHT  = 0b1000_0000;
        const LEFT   = 0b0100_0000;
        const DOWN   = 0b0010_0000;
        const UP     = 0b0001_0000;
        const START  = 0b0000_1000;
        const SELECT = 0b0000_0100;
        const BUTTON_B = 0b0000_0010;
        const BUTTON_A = 0b0000_0001;
    }
}

/// Total number of buttons on a standard NES controller
pub const BUTTON_COUNT: u8 = 8;

pub struct Joypad {
    strobe: bool,
    button_index: u8,
    button_status: JoypadButton,
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            strobe: false,
            button_index: 0,
            button_status: JoypadButton::from_bits_truncate(0),
        }
    }

    pub fn write(&mut self, data: u8) {
        self.strobe = data & 1 == 1;
        if self.strobe {
            self.button_index = 0;
        }
    }

    /// Reads the status of the next button in the sequence.
    /// 
    /// # Polling Algorithm
    /// The NES polls controllers by first setting 'strobe' to 1 (resetting the shift register),
    /// then setting it to 0. Subsequent reads from $4016/$4017 shift the register, 
    /// returning the status of each button in order (A, B, Select, Start, Up, Down, Left, Right).
    pub fn read(&mut self) -> u8 {
        if self.button_index >= BUTTON_COUNT {
            return 1;
        }
        
        let response = (self.button_status.bits() & (1 << self.button_index)) >> self.button_index;
        
        if (!self.strobe) && self.button_index < BUTTON_COUNT {
            self.button_index += 1;
        }
        
        response
    }

    pub fn set_button_pressed_status(&mut self, button: JoypadButton, pressed: bool) {
        if pressed {
            self.button_status.insert(button);
        } else {
            self.button_status.remove(button);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Objective**: Verify that the Joypad correctly latches button states when 
    /// strobe is active and rotates through buttons during subsequent reads.
    #[test]
    fn test_joypad_polling() {
        let mut joypad = Joypad::new();
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, true);
        joypad.set_button_pressed_status(JoypadButton::SELECT, true);
        
        // Strobe ON then OFF to reset position
        joypad.write(1);
        joypad.write(0);
        
        assert_eq!(joypad.read(), 1); // A
        assert_eq!(joypad.read(), 0); // B
        assert_eq!(joypad.read(), 1); // Select
        assert_eq!(joypad.read(), 0); // Start
    }

    /// **Objective**: Verify that the Joypad continues to return 'A' status while 
    /// strobe is held active (standard NES hardware behavior).
    #[test]
    fn test_joypad_strobe_latch() {
        let mut joypad = Joypad::new();
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, true);
        
        joypad.write(1);
        assert_eq!(joypad.read(), 1);
        assert_eq!(joypad.read(), 1);
        assert_eq!(joypad.read(), 1);
    }

    /// **Objective**: Verify that releasing a joypad button correctly updates 
    /// the internal state to reflect a non-pressed status.
    #[test]
    fn test_joypad_button_release() {
        let mut joypad = Joypad::new();
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, true);
        joypad.set_button_pressed_status(JoypadButton::BUTTON_A, false);
        
        joypad.write(1);
        joypad.write(0);
        assert_eq!(joypad.read(), 0);
    }
}
