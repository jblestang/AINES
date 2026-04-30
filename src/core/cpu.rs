use super::bus::Bus;

#[derive(Debug)]
pub enum AddressingMode {
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    IndirectX,
    IndirectY,
    NoneAddressing,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CpuFlags: u8 {
        const CARRY             = 0b0000_0001;
        const ZERO              = 0b0000_0010;
        const INTERRUPT_DISABLE = 0b0000_0100;
        const DECIMAL_MODE      = 0b0000_1000;
        const BREAK             = 0b0001_0000;
        const BREAK2            = 0b0010_0000;
        const OVERFLOW          = 0b0100_0000;
        const NEGATIVE          = 0b1000_0000;
    }
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub pc: u16,
    pub sp: u8,
    pub status: CpuFlags,
}

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: 0xFD,
            status: CpuFlags::from_bits_truncate(0b100100),
        }
    }

    pub fn reset(&mut self, bus: &mut Bus) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.status = CpuFlags::from_bits_truncate(0b100100);

        self.pc = self.mem_read_u16(bus, 0xFFFC);
    }

    pub fn nmi(&mut self, bus: &mut Bus) {
        self.stack_push_u16(bus, self.pc);
        let mut flags = self.status.clone();
        flags.remove(CpuFlags::BREAK);
        flags.insert(CpuFlags::BREAK2);
        self.stack_push(bus, flags.bits());
        self.status.insert(CpuFlags::INTERRUPT_DISABLE);

        self.pc = self.mem_read_u16(bus, 0xFFFA);
    }

    fn mem_read(&mut self, bus: &mut Bus, addr: u16) -> u8 {
        bus.read(addr)
    }

    fn mem_write(&mut self, bus: &mut Bus, addr: u16, data: u8) {
        bus.write(addr, data);
    }

    fn mem_read_u16(&mut self, bus: &mut Bus, pos: u16) -> u16 {
        let lo = self.mem_read(bus, pos) as u16;
        let hi = self.mem_read(bus, pos + 1) as u16;
        (hi << 8) | (lo as u16)
    }

    fn mem_write_u16(&mut self, bus: &mut Bus, pos: u16, data: u16) {
        let hi = (data >> 8) as u8;
        let lo = (data & 0xff) as u8;
        self.mem_write(bus, pos, lo);
        self.mem_write(bus, pos + 1, hi);
    }

    fn stack_push(&mut self, bus: &mut Bus, data: u8) {
        self.mem_write(bus, (0x0100 as u16) + self.sp as u16, data);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn stack_pop(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.mem_read(bus, (0x0100 as u16) + self.sp as u16)
    }

    fn stack_push_u16(&mut self, bus: &mut Bus, data: u16) {
        let hi = (data >> 8) as u8;
        let lo = (data & 0xff) as u8;
        self.stack_push(bus, hi);
        self.stack_push(bus, lo);
    }

    fn stack_pop_u16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.stack_pop(bus) as u16;
        let hi = self.stack_pop(bus) as u16;
        hi << 8 | lo
    }

    fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.status.insert(CpuFlags::ZERO);
        } else {
            self.status.remove(CpuFlags::ZERO);
        }

        if result & 0b1000_0000 != 0 {
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
        }
    }

    fn get_operand_address(&mut self, bus: &mut Bus, mode: &AddressingMode) -> (u16, bool) {
        match mode {
            AddressingMode::Immediate => (self.pc, false),
            AddressingMode::ZeroPage => (self.mem_read(bus, self.pc) as u16, false),
            AddressingMode::Absolute => (self.mem_read_u16(bus, self.pc), false),
            AddressingMode::ZeroPageX => {
                let pos = self.mem_read(bus, self.pc);
                (pos.wrapping_add(self.x) as u16, false)
            }
            AddressingMode::ZeroPageY => {
                let pos = self.mem_read(bus, self.pc);
                (pos.wrapping_add(self.y) as u16, false)
            }
            AddressingMode::AbsoluteX => {
                let base = self.mem_read_u16(bus, self.pc);
                let addr = base.wrapping_add(self.x as u16);
                (addr, self.page_crossed(base, addr))
            }
            AddressingMode::AbsoluteY => {
                let base = self.mem_read_u16(bus, self.pc);
                let addr = base.wrapping_add(self.y as u16);
                (addr, self.page_crossed(base, addr))
            }
            AddressingMode::IndirectX => {
                let base = self.mem_read(bus, self.pc);
                let ptr = base.wrapping_add(self.x);
                let lo = self.mem_read(bus, ptr as u16);
                let hi = self.mem_read(bus, ptr.wrapping_add(1) as u16);
                (((hi as u16) << 8) | (lo as u16), false)
            }
            AddressingMode::IndirectY => {
                let base = self.mem_read(bus, self.pc);
                let lo = self.mem_read(bus, base as u16);
                let hi = self.mem_read(bus, base.wrapping_add(1) as u16);
                let deref_base = ((hi as u16) << 8) | (lo as u16);
                let addr = deref_base.wrapping_add(self.y as u16);
                (addr, self.page_crossed(deref_base, addr))
            }
            AddressingMode::NoneAddressing => panic!("mode {:?} is not supported", mode),
        }
    }

    fn page_crossed(&self, addr1: u16, addr2: u16) -> bool {
        (addr1 & 0xFF00) != (addr2 & 0xFF00)
    }

    fn get_operand_address_for_jmp_indirect(&mut self, bus: &mut Bus) -> u16 {
        let addr = self.mem_read_u16(bus, self.pc);
        
        let indirect_ref = if addr & 0x00FF == 0x00FF {
            let lo = self.mem_read(bus, addr);
            let hi = self.mem_read(bus, addr & 0xFF00);
            ((hi as u16) << 8) | (lo as u16)
        } else {
            self.mem_read_u16(bus, addr)
        };

        indirect_ref
    }

    fn branch(&mut self, bus: &mut Bus, condition: bool) -> u32 {
        let jump: i8 = self.mem_read(bus, self.pc) as i8;
        self.pc = self.pc.wrapping_add(1);
        if condition {
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(jump as u16);
            if self.page_crossed(old_pc, self.pc) {
                return 2;
            }
            return 1;
        }
        0
    }

    fn lda(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.a = value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    fn ldx(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.x = value;
        self.update_zero_and_negative_flags(self.x);
        page_crossed
    }

    fn ldy(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.y = value;
        self.update_zero_and_negative_flags(self.y);
        page_crossed
    }

    fn sta(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        self.mem_write(bus, addr, self.a);
    }

    fn stx(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        self.mem_write(bus, addr, self.x);
    }

    fn sty(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        self.mem_write(bus, addr, self.y);
    }

    fn add_to_register_a(&mut self, data: u8) {
        let sum = self.a as u16 
            + data as u16 
            + (if self.status.contains(CpuFlags::CARRY) { 1 } else { 0 }) as u16;

        let carry = sum > 0xFF;

        if carry {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        let result = sum as u8;

        if (data ^ result) & (result ^ self.a) & 0x80 != 0 {
            self.status.insert(CpuFlags::OVERFLOW);
        } else {
            self.status.remove(CpuFlags::OVERFLOW);
        }

        self.a = result;
        self.update_zero_and_negative_flags(self.a);
    }

    fn adc(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.add_to_register_a(value);
        page_crossed
    }

    fn sbc(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.add_to_register_a(((value as i8).wrapping_neg().wrapping_sub(1)) as u8);
        page_crossed
    }

    fn and(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.a &= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    fn eor(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.a ^= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    fn ora(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = self.mem_read(bus, addr);
        self.a |= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    fn asl_a(&mut self) {
        let mut data = self.a;
        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data << 1;
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    fn asl(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data << 1;
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn lsr_a(&mut self) {
        let mut data = self.a;
        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data >> 1;
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    fn lsr(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data >> 1;
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn rol_a(&mut self) {
        let mut data = self.a;
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data << 1;
        if old_carry {
            data = data | 1;
        }
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    fn rol(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data << 1;
        if old_carry {
            data = data | 1;
        }
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn ror_a(&mut self) {
        let mut data = self.a;
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data >> 1;
        if old_carry {
            data = data | 0b1000_0000;
        }
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    fn ror(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data = data >> 1;
        if old_carry {
            data = data | 0b1000_0000;
        }
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn inc(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        data = data.wrapping_add(1);
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn dec(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = self.mem_read(bus, addr);
        data = data.wrapping_sub(1);
        self.mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.update_zero_and_negative_flags(self.x);
    }

    fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.update_zero_and_negative_flags(self.y);
    }

    fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.x);
    }

    fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.y);
    }

    fn cmp_base(&mut self, mode: &AddressingMode, compare_with: u8, bus: &mut Bus) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let data = self.mem_read(bus, addr);
        if data <= compare_with {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        self.update_zero_and_negative_flags(compare_with.wrapping_sub(data));
        page_crossed
    }

    fn cmp(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.a, bus)
    }

    fn cpx(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.x, bus)
    }

    fn cpy(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.y, bus)
    }

    fn bit(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let data = self.mem_read(bus, addr);
        let and = self.a & data;
        if and == 0 {
            self.status.insert(CpuFlags::ZERO);
        } else {
            self.status.remove(CpuFlags::ZERO);
        }

        if data & 0b1000_0000 > 0 {
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
        }

        if data & 0b0100_0000 > 0 {
            self.status.insert(CpuFlags::OVERFLOW);
        } else {
            self.status.remove(CpuFlags::OVERFLOW);
        }
        page_crossed
    }

    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        if bus.ppu.nmi_interrupt {
            bus.ppu.nmi_interrupt = false;
            self.nmi(bus);
            return 7; // NMI takes 7 cycles
        }
        
        let opcode = self.mem_read(bus, self.pc);
        self.pc += 1;

        let cycles = match opcode {
            // LDA
            0xA9 => { self.lda(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA5 => { self.lda(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB5 => { self.lda(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xAD => { self.lda(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBD => { let pc = self.lda(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xB9 => { let pc = self.lda(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xA1 => { self.lda(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xB1 => { let pc = self.lda(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            // LDX
            0xA2 => { self.ldx(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA6 => { self.ldx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB6 => { self.ldx(bus, &AddressingMode::ZeroPageY); self.pc += 1; 4 }
            0xAE => { self.ldx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBE => { let pc = self.ldx(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            // LDY
            0xA0 => { self.ldy(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA4 => { self.ldy(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB4 => { self.ldy(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xAC => { self.ldy(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBC => { let pc = self.ldy(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            // STA
            0x85 => { self.sta(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x95 => { self.sta(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x8D => { self.sta(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x9D => { self.sta(bus, &AddressingMode::AbsoluteX); self.pc += 2; 5 }
            0x99 => { self.sta(bus, &AddressingMode::AbsoluteY); self.pc += 2; 5 }
            0x81 => { self.sta(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x91 => { self.sta(bus, &AddressingMode::IndirectY); self.pc += 1; 6 }
            // STX
            0x86 => { self.stx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x96 => { self.stx(bus, &AddressingMode::ZeroPageY); self.pc += 1; 4 }
            0x8E => { self.stx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            // STY
            0x84 => { self.sty(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x94 => { self.sty(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x8C => { self.sty(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // ADC
            0x69 => { self.adc(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x65 => { self.adc(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x75 => { self.adc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x6D => { self.adc(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x7D => { let pc = self.adc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x79 => { let pc = self.adc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x61 => { self.adc(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x71 => { let pc = self.adc(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            // SBC
            0xE9 => { self.sbc(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xE5 => { self.sbc(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xF5 => { self.sbc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xED => { self.sbc(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xFD => { let pc = self.sbc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xF9 => { let pc = self.sbc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xE1 => { self.sbc(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xF1 => { let pc = self.sbc(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            // AND
            0x29 => { self.and(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x25 => { self.and(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x35 => { self.and(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x2D => { self.and(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x3D => { let pc = self.and(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x39 => { let pc = self.and(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x21 => { self.and(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x31 => { let pc = self.and(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            // EOR
            0x49 => { self.eor(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x45 => { self.eor(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x55 => { self.eor(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x4D => { self.eor(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x5D => { let pc = self.eor(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x59 => { let pc = self.eor(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x41 => { self.eor(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x51 => { let pc = self.eor(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            // ORA
            0x09 => { self.ora(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x05 => { self.ora(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x15 => { self.ora(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x0D => { self.ora(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x1D => { let pc = self.ora(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x19 => { let pc = self.ora(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0x01 => { self.ora(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x11 => { let pc = self.ora(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }

            // ASL
            0x0A => { self.asl_a(); 2 }
            0x06 => { self.asl(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x16 => { self.asl(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x0E => { self.asl(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x1E => { self.asl(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // LSR
            0x4A => { self.lsr_a(); 2 }
            0x46 => { self.lsr(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x56 => { self.lsr(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x4E => { self.lsr(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x5E => { self.lsr(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // ROL
            0x2A => { self.rol_a(); 2 }
            0x26 => { self.rol(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x36 => { self.rol(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x2E => { self.rol(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x3E => { self.rol(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // ROR
            0x6A => { self.ror_a(); 2 }
            0x66 => { self.ror(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x76 => { self.ror(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x6E => { self.ror(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x7E => { self.ror(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // INC
            0xE6 => { self.inc(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0xF6 => { self.inc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xEE => { self.inc(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0xFE => { self.inc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // DEC
            0xC6 => { self.dec(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0xD6 => { self.dec(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xCE => { self.dec(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0xDE => { self.dec(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // INX / INY / DEX / DEY
            0xE8 => { self.inx(); 2 }
            0xC8 => { self.iny(); 2 }
            0xCA => { self.dex(); 2 }
            0x88 => { self.dey(); 2 }

            // CMP / CPX / CPY
            0xC9 => { self.cmp(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xC5 => { self.cmp(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xD5 => { self.cmp(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xCD => { self.cmp(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xDD => { let pc = self.cmp(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xD9 => { let pc = self.cmp(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + if pc { 1 } else { 0 } }
            0xC1 => { self.cmp(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xD1 => { let pc = self.cmp(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + if pc { 1 } else { 0 } }
            
            0xE0 => { self.cpx(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xE4 => { self.cpx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xEC => { self.cpx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            
            0xC0 => { self.cpy(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xC4 => { self.cpy(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xCC => { self.cpy(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // BIT
            0x24 => { self.bit(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x2C => { self.bit(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // Jumps & Calls
            0x4C => {
                self.pc = self.mem_read_u16(bus, self.pc);
                3
            }
            0x6C => {
                self.pc = self.get_operand_address_for_jmp_indirect(bus);
                5
            }
            0x20 => {
                self.stack_push_u16(bus, self.pc + 2 - 1);
                self.pc = self.mem_read_u16(bus, self.pc);
                6
            }
            0x60 => {
                self.pc = self.stack_pop_u16(bus) + 1;
                6
            }
            0x40 => {
                self.status = CpuFlags::from_bits_truncate(self.stack_pop(bus));
                self.status.remove(CpuFlags::BREAK);
                self.status.insert(CpuFlags::BREAK2);
                self.pc = self.stack_pop_u16(bus);
                6
            }

            // Branches
            0x90 => { // BCC
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::CARRY));
                2 + cycles
            }
            0xB0 => { // BCS
                let cycles = self.branch(bus, self.status.contains(CpuFlags::CARRY));
                2 + cycles
            }
            0xF0 => { // BEQ
                let cycles = self.branch(bus, self.status.contains(CpuFlags::ZERO));
                2 + cycles
            }
            0x30 => { // BMI
                let cycles = self.branch(bus, self.status.contains(CpuFlags::NEGATIVE));
                2 + cycles
            }
            0xD0 => { // BNE
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::ZERO));
                2 + cycles
            }
            0x10 => { // BPL
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::NEGATIVE));
                2 + cycles
            }
            0x50 => { // BVC
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::OVERFLOW));
                2 + cycles
            }
            0x70 => { // BVS
                let cycles = self.branch(bus, self.status.contains(CpuFlags::OVERFLOW));
                2 + cycles
            }

            // Status Flag Changes
            0x38 => { self.status.insert(CpuFlags::CARRY); 2 }
            0xF8 => { self.status.insert(CpuFlags::DECIMAL_MODE); 2 }
            0x78 => { self.status.insert(CpuFlags::INTERRUPT_DISABLE); 2 }
            0x18 => { self.status.remove(CpuFlags::CARRY); 2 }
            0xD8 => { self.status.remove(CpuFlags::DECIMAL_MODE); 2 }
            0x58 => { self.status.remove(CpuFlags::INTERRUPT_DISABLE); 2 }
            0xB8 => { self.status.remove(CpuFlags::OVERFLOW); 2 }

            // Register Transfers
            0xAA => { self.x = self.a; self.update_zero_and_negative_flags(self.x); 2 }
            0xA8 => { self.y = self.a; self.update_zero_and_negative_flags(self.y); 2 }
            0xBA => { self.x = self.sp; self.update_zero_and_negative_flags(self.x); 2 }
            0x8A => { self.a = self.x; self.update_zero_and_negative_flags(self.a); 2 }
            0x9A => { self.sp = self.x; 2 }
            0x98 => { self.a = self.y; self.update_zero_and_negative_flags(self.a); 2 }

            // Stack Operations
            0x48 => { self.stack_push(bus, self.a); 3 }
            0x08 => {
                let mut flags = self.status.clone();
                flags.insert(CpuFlags::BREAK);
                flags.insert(CpuFlags::BREAK2);
                self.stack_push(bus, flags.bits());
                3
            }
            0x68 => {
                self.a = self.stack_pop(bus);
                self.update_zero_and_negative_flags(self.a);
                4
            }
            0x28 => {
                self.status = CpuFlags::from_bits_truncate(self.stack_pop(bus));
                self.status.remove(CpuFlags::BREAK);
                self.status.insert(CpuFlags::BREAK2);
                4
            }

            // System
            0x00 => { // BRK
                // On real NES, BRK pushes PC+2 and flags to stack, then jumps to IRQ vector
                7
            }
            0xEA => { // NOP
                2
            }

            _ => {
                println!("Unimplemented Opcode: {:#X}", opcode);
                2
            }
        };
        let dma = bus.dma_cycles;
        bus.dma_cycles = 0;
        cycles + dma
    }
}
